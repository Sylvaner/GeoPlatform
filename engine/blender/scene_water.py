"""Contexte : l'eau suit le relief et reste légèrement au-dessus de l'orthophoto.
Usage : réunir ses triangles, puis ajouter une épaisseur vers le bas.
Cette épaisseur est une convention graphique ; elle ne représente pas une bathymétrie.
"""

import bmesh


def add_water_volume(obj, settings):
    # Les découpes terrain produisent des sommets partagés géométriquement.
    # Les souder évite des parois internes entre les morceaux d'un même plan d'eau.
    geometry = bmesh.new()
    try:
        geometry.from_mesh(obj.data)
        bmesh.ops.remove_doubles(geometry, verts=list(geometry.verts), dist=0.00001)
        # « Recalculer vers l'extérieur » ne définit pas le haut d'une surface
        # ouverte. Sur une cuvette, Blender peut retourner tout le lac vers le bas.
        # Nous voulons ici explicitement des normales vers le ciel.
        geometry.normal_update()
        for face in geometry.faces:
            if face.normal.z < 0:
                face.normal_flip()
        geometry.to_mesh(obj.data)
    finally:
        geometry.free()
    obj.data.update()
    modifier = obj.modifiers.new("Épaisseur de l'eau", "SOLIDIFY")
    modifier.thickness = settings["thickness_m"]
    modifier.offset = -1
    modifier.use_even_offset = False
    obj["eau_surface"] = True
    obj["eau_decalage_m"] = settings["terrain_offset_m"]
