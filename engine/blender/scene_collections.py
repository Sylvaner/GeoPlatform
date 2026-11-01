"""Contexte : une couche de données devient une collection affichable dans Blender.
Usage : masquer avec l'œil du View Layer, puis retirer les collections sans géométrie.
Le bouton global « désactiver dans les vues » reste disponible, sans être imposé à l'export.
"""

import bpy


def collection(name, visible=True):
    result = bpy.data.collections.new(name)
    bpy.context.scene.collection.children.link(result)
    result.hide_render = not visible
    # hide_viewport sur Collection désactive globalement ses objets : l'œil
    # ne permet alors pas de les réafficher. LayerCollection utilise bien l'œil.
    result.hide_viewport = False
    for view_layer in bpy.context.scene.view_layers:
        view_layer.update()
        view_layer.layer_collection.children[name].hide_viewport = not visible
    return result


def has_geometry(obj):
    """Un objet portant un nom ne suffit pas à constituer une couche de données."""
    if obj.type == "MESH":
        return bool(obj.data.polygons)
    if obj.type == "CURVE":
        return any(len(s.points) > 1 or len(s.bezier_points) > 1 for s in obj.data.splines)
    return obj.type in {"CAMERA", "LIGHT"}


def remove_empty_collections():
    for obj in list(bpy.data.objects):
        if not has_geometry(obj):
            bpy.data.objects.remove(obj, do_unlink=True)
    # Les collections vides peuvent être imbriquées : les feuilles passent d'abord.
    for target in reversed(list(bpy.data.collections)):
        if not target.all_objects:
            bpy.data.collections.remove(target)
