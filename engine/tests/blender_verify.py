"""Contexte : contrôle indépendant du .blend réellement enregistré.
Usage : blender --background fichier.blend --python tests/blender_verify.py --
        --report fichier.json [--render apercu.png].
La vérification rouvre la scène ; les fichiers temporaires de l'export peuvent avoir disparu.
"""

import argparse
import hashlib
import json
from pathlib import Path
import sys
import bpy
from bpy_extras.object_utils import world_to_camera_view


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--report",required=True)
    parser.add_argument("--render")
    parser.add_argument("--require-trees", action="store_true")
    args = parser.parse_args(sys.argv[sys.argv.index("--")+1:])
    scene = bpy.context.scene
    config = json.loads(bpy.data.texts["Configuration interne.json"].as_string())
    assert "Configuration export.toml" in bpy.data.texts
    assert scene.camera is not None
    assert any(obj.type=="LIGHT" and obj.data.type=="SUN" for obj in scene.objects)
    assert not bpy.data.libraries
    expected = set(bpy.data.collections.keys()) & set(config["layers"])
    for target in bpy.data.collections:
        assert target.all_objects, "Collection vide : " + target.name
        assert any(obj.type in {"CAMERA", "LIGHT"} or
                   (obj.type == "MESH" and len(obj.data.polygons) > 0) or
                   (obj.type == "CURVE" and any(len(s.points)>1 for s in obj.data.splines))
                   for obj in target.all_objects), "Collection sans données : " + target.name
    assert all(config["layers"][name]["enabled"] for name in expected)
    packed = []
    for image in bpy.data.images:
        if image.source=="FILE":
            assert image.packed_file is not None, image.name
            if "source_sha256" in image:
                assert hashlib.sha256(image.packed_file.data).hexdigest() == image["source_sha256"]
                assert tuple(image.size) == (image["native_width"], image["native_height"])
            packed.append(image.name)
    if config["layers"]["orthophoto"]["enabled"]:
        assert packed, "Orthophoto non intégrée"
    buildings = bpy.data.collections.get("bd_topo.batiment")
    if buildings and config["buildings"]["texture_roofs"]:
        for obj in buildings.objects:
            assert obj.data.uv_layers, "Toit sans UV : " + obj.name
            textured_slots = {index for index, mat in enumerate(obj.data.materials)
                              if mat and mat.use_nodes and any(node.type=="TEX_IMAGE" for node in mat.node_tree.nodes)}
            assert any(p.material_index in textured_slots for p in obj.data.polygons), "Toit sans JPEG : " + obj.name
            assert any(p.material_index == 0 for p in obj.data.polygons), "Murs sans matériau simple : " + obj.name
    assert not scene.render.use_simplify, "Limitation de textures activée dans le rendu"
    source_report = json.loads(bpy.data.texts["Compte rendu export.json"].as_string())
    lidar = source_report.get("lidar", {})
    count_trees = lidar.get("trees_exported", 0)
    if args.require_trees:
        assert count_trees > 0, "Aucun arbre annoncé dans le rapport"
    if count_trees:
        vegetation = bpy.data.collections.get("vegetation")
        assert vegetation is not None, "Collection végétation absente"
        assert len(vegetation.objects) == count_trees*2, "Troncs ou houppiers manquants"
        assert len([obj for obj in vegetation.objects if obj.name.startswith("Houppier ")]) == count_trees
        if config["layers"]["vegetation"]["visible"]:
            assert not vegetation.hide_viewport and not vegetation.hide_render
            crowns = [obj for obj in vegetation.objects if obj.name.startswith("Houppier ")]
            assert any(obj.visible_get() and not obj.hide_render for obj in crowns), "Houppiers masqués"
            # Un objet présent mais hors champ ne résout pas un problème de visibilité.
            positions = [world_to_camera_view(scene, scene.camera, obj.location) for obj in crowns]
            assert any(0 <= p.x <= 1 and 0 <= p.y <= 1 and
                       scene.camera.data.clip_start < p.z < scene.camera.data.clip_end
                       for p in positions), "Tous les arbres sont hors du cadre"
        assert source_report["fichiers_lidar"], "Arbres sans source LiDAR enregistrée"
    for name in expected:
        target = bpy.data.collections[name]
        visible = config["layers"][name]["visible"]
        if name=="terrain" and config["view"]["hide_terrain_with_orthophoto"] and "orthophoto" in expected:
            visible = False
        assert target.hide_render == (not visible)
        assert not target.hide_viewport, "Collection désactivée globalement : " + name
        assert bpy.context.view_layer.layer_collection.children[name].hide_viewport == (not visible)
    # Un terrain caché doit rester réactivable avec l'œil, sans changer un autre réglage.
    if "terrain" in expected:
        layer = bpy.context.view_layer.layer_collection.children["terrain"]
        hidden = layer.hide_viewport
        layer.hide_viewport = False
        bpy.context.view_layer.update()
        assert any(obj.visible_get() for obj in bpy.data.collections["terrain"].objects), "Terrain impossible à réafficher"
        layer.hide_viewport = hidden
    water_count = 0
    for obj in bpy.data.objects:
        if obj.get("eau_surface"):
            water_count += 1
            modifier = next((m for m in obj.modifiers if m.type == "SOLIDIFY"), None)
            assert modifier and modifier.offset == -1 and modifier.thickness > 0
            assert all(p.normal.z > 0 for p in obj.data.polygons), "Eau orientée vers le bas"
    view_count = 0
    for screen in bpy.data.screens:
        for area in screen.areas:
            for space in area.spaces:
                if space.type=="VIEW_3D":
                    assert abs(space.clip_end-config["view"]["clip_end_m"]) < .01
                    view_count += 1
                elif space.type=="OUTLINER" and config["view"]["collapse_outliner"]:
                    assert space.display_mode=="VIEW_LAYER", "Outliner hors du mode View Layer"
    assert view_count > 0
    terrain_name = "terrain" if "terrain" in expected else "orthophoto"
    terrain = bpy.data.collections[terrain_name].objects[0]
    assert terrain.type=="MESH"
    assert terrain.data.uv_layers
    for vertex in terrain.data.vertices:
        projected = world_to_camera_view(scene,scene.camera,terrain.matrix_world @ vertex.co)
        assert -.0001 <= projected.x <= 1.0001 and -.0001 <= projected.y <= 1.0001, "Terrain hors cadre"
    result = {"blender_version":bpy.app.version_string,"collections":list(bpy.data.collections.keys()),
              "packed_images":packed,"terrain_vertices":len(terrain.data.vertices),
              "objects":len(bpy.data.objects),"camera":scene.camera.name,"water_objects":water_count,
              "trees":count_trees,"source_report":source_report}
    if args.render:
        # Petite image de contrôle CPU : ne dépend pas de la disponibilité d'un GPU.
        scene.render.engine="CYCLES"
        scene.cycles.device="CPU"
        scene.cycles.samples=8
        scene.render.resolution_x=800
        scene.render.resolution_y=600
        scene.render.resolution_percentage=100
        scene.render.threads_mode="FIXED"
        scene.render.threads=4
        scene.render.filepath=str(Path(args.render).resolve())
        scene.render.image_settings.file_format="PNG"
        bpy.ops.render.render(write_still=True)
        result["preview"]=scene.render.filepath
    Path(args.report).write_text(json.dumps(result,ensure_ascii=False,indent=2),encoding="utf-8")
    print("VERIFICATION_BLEND_OK",args.report)


if __name__=="__main__":
    main()
