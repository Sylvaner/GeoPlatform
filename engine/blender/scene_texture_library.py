"""Contexte : conserver les JPEG BD ORTHO originaux dans le fichier .blend.
Usage : une bibliothèque partage les matériaux entre terrain et bâtiments.
Le hachage des octets intégrés permet de vérifier qu'aucune recompression n'a eu lieu.
"""

import hashlib
from pathlib import Path
import bpy
from scene_materials import material


class TextureLibrary:
    def __init__(self, plan):
        self.tiles = {(tile["col"], tile["row"]): tile
                      for tile in (plan.get("ortho") or {}).get("tiles", [])}
        self.materials = {}
        self.interpolation = plan["config"]["ortho"]["interpolation"]

    def get(self, key):
        if key not in self.materials:
            if key not in self.tiles:
                raise RuntimeError(f"Tuile native manquante pour une face : {key}")
            tile = self.tiles[key]
            image = bpy.data.images.load(tile["path"], check_existing=True)
            if tuple(image.size) != (tile["width"], tile["height"]):
                raise RuntimeError("La définition du JPEG a changé depuis la préparation")
            # pack lit le fichier source, sans modifier ses pixels ni le réenregistrer.
            image.pack()
            source_hash = hashlib.sha256(Path(tile["path"]).read_bytes()).hexdigest()
            if hashlib.sha256(image.packed_file.data).hexdigest() != source_hash:
                raise RuntimeError("Les octets du JPEG intégré diffèrent du cache")
            image["source_sha256"] = source_hash
            image["native_width"] = tile["width"]
            image["native_height"] = tile["height"]
            image["source_cache"] = tile["path"]
            result = material(f"BD ORTHO {key[0]} {key[1]}", (1, 1, 1, 1))
            node = result.node_tree.nodes.new("ShaderNodeTexImage")
            node.image = image
            node.interpolation = "Closest" if self.interpolation == "nearest" else "Linear"
            node.extension = "EXTEND"
            result.node_tree.links.new(node.outputs["Color"],
                                      result.node_tree.nodes["Principled BSDF"].inputs["Base Color"])
            self.materials[key] = result
        return self.materials[key]
