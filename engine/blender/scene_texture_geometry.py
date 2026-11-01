"""Contexte : une face peut traverser plusieurs JPEG dans la grille d'origine.
Usage : découper le maillage selon ses UV et utiliser chaque JPEG sans le redimensionner.
L'interpolation concerne les positions des sommets créés ; aucun pixel source n'est recalculé.
"""

import math


def clip_edge(polygon, axis, boundary, keep_greater):
    """Découpe simple d'un polygone convexe : conserve positions ET coordonnées UV."""
    if not polygon:
        return []
    result = []
    previous = polygon[-1]
    previous_inside = (previous[1][axis] >= boundary if keep_greater
                       else previous[1][axis] <= boundary)
    for current in polygon:
        inside = (current[1][axis] >= boundary if keep_greater
                  else current[1][axis] <= boundary)
        if inside != previous_inside:
            fraction = ((boundary - previous[1][axis]) /
                        (current[1][axis] - previous[1][axis]))
            position = tuple(a + (b-a)*fraction for a, b in zip(previous[0], current[0]))
            uv = tuple(a + (b-a)*fraction for a, b in zip(previous[1], current[1]))
            result.append((position, uv))
        if inside:
            result.append(current)
        previous, previous_inside = current, inside
    return result


def tile_parts(polygon):
    """Rend les morceaux de cette face avec le numéro du JPEG correspondant."""
    umin, umax = min(p[1][0] for p in polygon), max(p[1][0] for p in polygon)
    vmin, vmax = min(p[1][1] for p in polygon), max(p[1][1] for p in polygon)
    for row in range(math.floor(vmin), math.floor(vmax)+1):
        for col in range(math.floor(umin), math.floor(umax)+1):
            piece = polygon
            for axis, boundary, greater in [(0, col, True), (0, col+1, False),
                                             (1, row, True), (1, row+1, False)]:
                piece = clip_edge(piece, axis, boundary, greater)
            # Une simple arête sur une frontière n'est pas une face utile.
            origin = piece[0][1] if piece else (0, 0)
            area = sum((a[1][0]-origin[0])*(b[1][1]-origin[1]) -
                       (b[1][0]-origin[0])*(a[1][1]-origin[1])
                       for a, b in zip(piece, piece[1:] + piece[:1]))
            if len(piece) >= 3 and abs(area) > 1e-12:
                yield (col, row), piece


def textured_mesh(name, vertices, faces, uv, textured_faces, target, base_material, library):
    # L'import local évite un cycle : scene_geometry appelle aussi cette fonction.
    from scene_geometry import mesh
    points, polygons, loop_uv, indices = [], [], [], []
    materials = [base_material]
    slots = {}
    textured_faces = set(textured_faces)
    for face_index, face in enumerate(faces):
        # Trianguler explicitement garantit une interpolation identique en 3D et en UV,
        # même sur un terrain en pente. Les murs non texturés restent des quadrilatères.
        source_faces = [face]
        if face_index in textured_faces and len(face) > 3:
            source_faces = [(face[0], face[k], face[k+1]) for k in range(1, len(face)-1)]
        for source in source_faces:
            if face_index in textured_faces:
                pieces = tile_parts([(vertices[i], uv[i]) for i in source])
            else:
                pieces = [(None, [(vertices[i], (0, 0)) for i in source])]
            for key, piece in pieces:
                start = len(points)
                points.extend(p[0] for p in piece)
                polygons.append(tuple(range(start, start+len(piece))))
                if key is None:
                    indices.append(0)
                    loop_uv.extend((0, 0) for _ in piece)
                else:
                    if key not in slots:
                        slots[key] = len(materials)
                        materials.append(library.get(key))
                    indices.append(slots[key])
                    loop_uv.extend((p[1][0]-key[0], 1-(p[1][1]-key[1])) for p in piece)
    obj = mesh(name, points, polygons, target, base_material)
    for mat in materials[1:]:
        obj.data.materials.append(mat)
    destination = obj.data.uv_layers.new(name="BD ORTHO pixels natifs")
    for loop, value in zip(destination.data, loop_uv):
        loop.uv = value
    for polygon, slot in zip(obj.data.polygons, indices):
        polygon.material_index = slot
    obj["texture_mapping"] = "JPEG natifs ; UV reprojetés ; toits uniquement sur les bâtiments"
    return obj
