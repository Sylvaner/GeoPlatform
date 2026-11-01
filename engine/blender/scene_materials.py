"""Contexte : matériaux simples, communs aux objets d'une couche.
Usage : les couleurs viennent du TOML ; scene_texture_library gère les JPEG natifs.
Les arbres utilisent du bruit procédural, sans texture ou bibliothèque externe.
"""

import bpy


def material(name, color, natural=False, variation=0.2):
    result = bpy.data.materials.new(name)
    result.diffuse_color = color
    result.use_nodes = True
    nodes = result.node_tree.nodes
    shader = nodes.get("Principled BSDF")
    shader.inputs["Base Color"].default_value = color
    shader.inputs["Roughness"].default_value = 0.85
    if natural:
        # Le bruit colore un seul shader : aucun feuillage individuel ni transparence coûteuse.
        noise = nodes.new("ShaderNodeTexNoise")
        noise.inputs["Scale"].default_value = 4.0
        noise.inputs["Detail"].default_value = 2.0
        ramp = nodes.new("ShaderNodeValToRGB")
        ramp.color_ramp.elements[0].color = tuple(max(0, c * (1-variation)) for c in color[:3]) + (1,)
        ramp.color_ramp.elements[1].color = tuple(min(1, c * (1+variation)) for c in color[:3]) + (1,)
        result.node_tree.links.new(noise.outputs["Fac"], ramp.inputs["Fac"])
        result.node_tree.links.new(ramp.outputs["Color"], shader.inputs["Base Color"])
    return result
