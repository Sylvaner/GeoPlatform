"""Contexte : Blender reçoit un plan local et enregistre une scène autonome.
Usage : lancé par geo-export en arrière-plan, jamais besoin d'une interface ouverte.
Les textures sont empaquetées ; configuration et compte rendu sont intégrés comme textes.
"""

import json
from pathlib import Path
import sys
import bpy

sys.path.insert(0,str(Path(__file__).resolve().parent))
from scene_geometry import collection, terrain, vector_layer
from scene_materials import material
from scene_texture_library import TextureLibrary
from scene_trees import build_trees, build_points
from scene_setup import setup
from scene_view import setup_view, terrain_visible
from scene_collections import remove_empty_collections
from scene_outliner import finalize_outliner


def main():
    if bpy.app.version < (4,2,0):
        raise RuntimeError("Blender 4.2 ou plus récent est nécessaire")
    plan_path = Path(sys.argv[sys.argv.index("--")+1])
    plan = json.loads(plan_path.read_text(encoding="utf-8"))
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.context.preferences.filepaths.save_version = 0
    config = plan["config"]
    library = TextureLibrary(plan)
    for name in ["terrain","orthophoto"]:
        style = config["layers"][name]
        if not style["enabled"] or (name=="orthophoto" and not plan["ortho"]):
            continue
        visible = terrain_visible(config, bool(plan["ortho"])) if name=="terrain" else style["visible"]
        target = collection(name,visible)
        mat = material("Sol naturel",style["color"],name=="terrain")
        terrain(plan,target,mat,style["z_offset_m"],library if name=="orthophoto" else None)
    for layer in plan["layers"]:
        vector_layer(layer,material(layer["name"],layer["style"]["color"]),library)
    build_trees(plan)
    build_points(plan)
    setup(plan)
    setup_view(plan)
    # Le JSON sert au contrôle automatisé ; l'utilisateur retrouve les TOML dans Blender.
    text = bpy.data.texts.new("Configuration interne.json")
    text.write(json.dumps(config,ensure_ascii=False,indent=2))
    bpy.data.texts.new("Configuration export.toml").write(plan["configuration_source"])
    bpy.data.texts.new("Configuration résolue.toml").write(plan["configuration_toml"])
    # Garde-fou final : aucune collection vide, même si un futur importeur en crée une.
    remove_empty_collections()
    bpy.context.scene.render.use_simplify = False
    text = bpy.data.texts.new("Compte rendu export.json")
    text.write(json.dumps({"terrain":plan["terrain"]["source"],"avertissements":plan["warnings"],
                          "fichiers_lidar":plan["lidar_files"],
                          "lidar":plan["lidar_report"],
                          "couches":[{"nom":p["name"],"sources":p["sources"],"partiel":p["partial"],"objets":len(p["objects"])} for p in plan["layers"]]},ensure_ascii=False,indent=2))
    bpy.ops.file.pack_all()
    for image in bpy.data.images:
        if image.source == "FILE" and image.packed_file is None:
            raise RuntimeError("Image externe non intégrée : "+image.name)
    if bpy.data.libraries:
        raise RuntimeError("La scène contient une bibliothèque externe")
    output = Path(plan["output"])
    output.parent.mkdir(parents=True,exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=str(output),compress=config["blender"]["compress_file"])
    if config["render"]["preview"]:
        preview = Path(plan["preview"])
        preview.parent.mkdir(parents=True,exist_ok=True)
        bpy.context.scene.render.filepath = str(preview)
        bpy.context.scene.render.image_settings.file_format = "PNG"
        bpy.ops.render.render(write_still=True)
    finalize_outliner(output, config)
    print("EXPORT_BLEND_OK",output)


if __name__ == "__main__":
    main()
