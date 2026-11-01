"""Contexte : transformer le plan préparé par Rust en objets et collections Blender.
Usage : les mètres sont locaux au centre de la zone ; aucune projection n'est faite ici.
Les attributs GeoJSON sont intégrés à chaque objet pour rester consultables.
"""

import json
import bpy
from scene_collections import collection


def mesh(name, vertices, faces, target, material):
    data = bpy.data.meshes.new(name)
    data.from_pydata(vertices, [], faces)
    data.update()
    obj = bpy.data.objects.new(name, data)
    target.objects.link(obj)
    obj.data.materials.append(material)
    return obj


def terrain(plan, target, material, offset=0, library=None):
    data = plan["terrain"]
    count = data["count"]
    step = data["side"] / (count-1)
    origin_z = data["origin"][2]
    half = data["side"] / 2
    vertices = [(-half+col*step, -half+row*step,
                 (data["heights"][row*count+col]-origin_z)*data["exaggeration"]+offset)
                for row in range(count) for col in range(count)]
    # Diagonale fixe sud-ouest / nord-est, identique à TerrainGrid.mesh_height.
    # Le terrain nu, la photo et l'eau partagent ainsi exactement la même surface.
    faces = []
    for row in range(count-1):
        for col in range(count-1):
            sw = row*count+col
            se, ne, nw = sw+1, sw+count+1, sw+count
            faces.extend([(sw, se, ne), (sw, ne, nw)])
    if library is not None:
        from scene_texture_geometry import textured_mesh
        return textured_mesh("Orthophoto native", vertices, faces, plan["ortho"]["terrain_uv"],
                             range(len(faces)), target, material, library)
    obj = mesh("Terrain " + data["source"], vertices, faces, target, material)
    uv = obj.data.uv_layers.new(name="Orthophoto Lambert-93")
    for face in obj.data.polygons:
        for index in face.loop_indices:
            point = vertices[obj.data.loops[index].vertex_index]
            uv.data[index].uv = ((point[0]+half)/data["side"], (point[1]+half)/data["side"])
        face.use_smooth = True
    return obj


def curve(name, lines, target, material, width):
    data = bpy.data.curves.new(name, "CURVE")
    data.dimensions = "3D"
    data.resolution_u = 1
    data.bevel_depth = width / 2
    data.bevel_resolution = 0
    for points in lines:
        spline = data.splines.new("POLY")
        spline.points.add(len(points)-1)
        for destination, point in zip(spline.points, points):
            destination.co = (*point, 1)
    obj = bpy.data.objects.new(name, data)
    target.objects.link(obj)
    obj.data.materials.append(material)
    return obj


def markers(name, points, target, material, size, height):
    vertices, faces = [], []
    for x,y,z in points:
        start = len(vertices)
        vertices.extend([(x-size,y-size,z),(x+size,y-size,z),
                         (x+size,y+size,z),(x-size,y+size,z),
                         (x,y,z+max(height,size))])
        faces.extend([(start,start+1,start+4),(start+1,start+2,start+4),
                      (start+2,start+3,start+4),(start+3,start,start+4)])
    return mesh(name,vertices,faces,target,material)


def vector_layer(layer, material, library=None):
    if not layer["objects"]:
        return None
    target = collection(layer["name"], layer["style"]["visible"])
    target["cache_sources"] = json.dumps(layer["sources"],ensure_ascii=False)
    target["cache_partiel"] = layer["partial"]
    target["objet_count"] = len(layer["objects"])
    style = layer["style"]
    for item in layer["objects"]:
        objects = []
        if item["kind"] == "points":
            objects.append(markers(item["name"], item["vertices"], target, material,
                                   style["line_width_m"], style["fallback_height_m"]))
        elif item["faces"]:
            if library is not None and item["roof_faces"] and item["uv"]:
                from scene_texture_geometry import textured_mesh
                objects.append(textured_mesh(item["name"], item["vertices"], item["faces"],
                                             item["uv"], item["roof_faces"], target, material, library))
            else:
                objects.append(mesh(item["name"],item["vertices"],item["faces"],target,material))
        if item["lines"]:
            objects.append(curve(item["name"],item["lines"],target,material,style["line_width_m"]))
        for obj in objects:
            if (item["properties"] or {}).get("export_eau") and obj.type == "MESH":
                from scene_water import add_water_volume
                add_water_volume(obj, item["properties"]["export_eau"])
            obj["attributs_source"] = json.dumps(item["properties"],ensure_ascii=False)
            obj["couche_source"] = layer["name"]
    return target
