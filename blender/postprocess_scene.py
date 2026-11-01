"""Applique les choix visuels que le moteur historique ne distingue pas encore."""

import json
from array import array
from datetime import datetime
from math import ceil, floor, sqrt
from pathlib import Path
import re
from statistics import median
import struct
import sys
from time import perf_counter

import bpy
import bmesh
from mathutils import Matrix, Vector
from mathutils.bvhtree import BVHTree


def log_event(message, started_at=None):
    """Écrit une étape horodatée et, si possible, sa durée réelle dans le journal du worker."""
    timestamp = datetime.now().astimezone().isoformat(timespec="seconds")
    duration = "" if started_at is None else f" · {perf_counter() - started_at:.2f} s"
    print(f"[{timestamp}] [BLENDER] {message}{duration}", flush=True)


def log_scene_stats(label):
    """Résume les éléments qui chargent réellement la mémoire et le viewport de Blender."""
    meshes = [obj for obj in bpy.data.objects if obj.type == "MESH"]
    vertices = sum(len(obj.data.vertices) for obj in meshes)
    polygons = sum(len(obj.data.polygons) for obj in meshes)
    decoded_bytes = sum(
        image.size[0] * image.size[1] * (16 if image.is_float else 4)
        for image in bpy.data.images
        if image.size[0] and image.size[1]
    )
    log_event(
        f"Diagnostic {label} : {len(meshes)} objets maillés, {vertices:,} sommets, "
        f"{polygons:,} faces, {len(bpy.data.materials):,} matériaux, "
        f"{len(bpy.data.images):,} images (≈ {decoded_bytes / 1_048_576:.0f} Mio décodés au minimum)"
    )


def varied_color(color, factor):
    """Éclaircit ou assombrit une couleur RGBA sans dépasser les bornes Blender."""
    return tuple(min(1.0, max(0.0, component * factor)) for component in color[:3]) + (1.0,)


def get_or_create_material(name, color, procedural=False):
    """Crée un matériau uni ou un bruit procédural léger sans image externe."""
    result = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    result.diffuse_color = color
    result.use_nodes = True
    nodes = result.node_tree.nodes
    links = result.node_tree.links
    nodes.clear()
    output = nodes.new("ShaderNodeOutputMaterial")
    shader = nodes.new("ShaderNodeBsdfPrincipled")
    shader.inputs["Base Color"].default_value = color
    shader.inputs["Roughness"].default_value = 0.85
    links.new(shader.outputs["BSDF"], output.inputs["Surface"])
    if procedural:
        coordinates = nodes.new("ShaderNodeTexCoord")
        noise = nodes.new("ShaderNodeTexNoise")
        noise.inputs["Scale"].default_value = 3.5
        noise.inputs["Detail"].default_value = 2.0
        noise.inputs["Roughness"].default_value = 0.55
        ramp = nodes.new("ShaderNodeValToRGB")
        ramp.color_ramp.elements[0].color = varied_color(color, 0.88)
        ramp.color_ramp.elements[1].color = varied_color(color, 1.10)
        links.new(coordinates.outputs["Generated"], noise.inputs["Vector"])
        links.new(noise.outputs["Fac"], ramp.inputs["Fac"])
        links.new(ramp.outputs["Color"], shader.inputs["Base Color"])
    return result


def wall_procedural_material(settings):
    """Crée un enduit légèrement irrégulier, distinct de la photographie des toits."""
    color = settings["wall_color"]
    name = "Murs · enduit légèrement texturé"
    result = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    result.diffuse_color = color
    result.use_nodes = True
    nodes = result.node_tree.nodes
    links = result.node_tree.links
    nodes.clear()
    output = nodes.new("ShaderNodeOutputMaterial")
    shader = nodes.new("ShaderNodeBsdfPrincipled")
    coordinates = nodes.new("ShaderNodeTexCoord")
    noise = nodes.new("ShaderNodeTexNoise")
    ramp = nodes.new("ShaderNodeValToRGB")
    bump = nodes.new("ShaderNodeBump")
    noise.inputs["Scale"].default_value = 7.0
    noise.inputs["Detail"].default_value = 2.0
    noise.inputs["Roughness"].default_value = 0.52
    ramp.color_ramp.elements[0].color = varied_color(color, 0.93)
    ramp.color_ramp.elements[1].color = varied_color(color, 1.04)
    bump.inputs["Strength"].default_value = 0.08
    bump.inputs["Distance"].default_value = 0.12
    shader.inputs["Roughness"].default_value = 0.82
    links.new(coordinates.outputs["Generated"], noise.inputs["Vector"])
    links.new(noise.outputs["Fac"], ramp.inputs["Fac"])
    links.new(noise.outputs["Fac"], bump.inputs["Height"])
    links.new(ramp.outputs["Color"], shader.inputs["Base Color"])
    links.new(bump.outputs["Normal"], shader.inputs["Normal"])
    links.new(shader.outputs["BSDF"], output.inputs["Surface"])
    return result


def forest_procedural_material(settings):
    """Crée la rampe de deux verts demandée, avec des nœuds lisibles dans Blender."""
    name = "Forêt LiDAR · rampe procédurale"
    result = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    result.diffuse_color = settings["forest_color_high"]
    result.use_nodes = True
    nodes = result.node_tree.nodes
    links = result.node_tree.links
    nodes.clear()
    output = nodes.new("ShaderNodeOutputMaterial")
    shader = nodes.new("ShaderNodeBsdfPrincipled")
    coordinates = nodes.new("ShaderNodeTexCoord")
    noise = nodes.new("ShaderNodeTexNoise")
    noise.noise_dimensions = "2D"
    noise.inputs["Scale"].default_value = 12.0
    noise.inputs["Detail"].default_value = 2.0
    noise.inputs["Roughness"].default_value = 0.5
    ramp = nodes.new("ShaderNodeValToRGB")
    ramp.color_ramp.elements[0].color = settings["forest_color_low"]
    ramp.color_ramp.elements[1].color = settings["forest_color_high"]
    shader.inputs["Roughness"].default_value = 0.85
    links.new(coordinates.outputs["Generated"], noise.inputs["Vector"])
    links.new(noise.outputs["Fac"], ramp.inputs["Fac"])
    links.new(ramp.outputs["Color"], shader.inputs["Base Color"])
    links.new(shader.outputs["BSDF"], output.inputs["Surface"])
    return result


def orthophoto_image(source, polygon):
    """Retrouve l'image réellement utilisée par le matériau d'une face orthophoto."""
    if polygon.material_index >= len(source.data.materials):
        return None
    material = source.data.materials[polygon.material_index]
    if material is None or not material.use_nodes or material.node_tree is None:
        return None
    return next(
        (
            node.image
            for node in material.node_tree.nodes
            if node.type == "TEX_IMAGE" and node.image is not None
        ),
        None,
    )


def image_from_material(material):
    """Retourne la première image réellement branchée dans un matériau Blender."""
    if material is None or not material.use_nodes or material.node_tree is None:
        return None
    return next(
        (
            node.image
            for node in material.node_tree.nodes
            if node.type == "TEX_IMAGE" and node.image is not None
        ),
        None,
    )


def tile_key_from_material(material):
    """Lit les coordonnées officielles de dalle conservées dans le nom du matériau."""
    if material is None:
        return None
    match = re.match(r"^BD ORTHO\s+(-?\d+)\s+(-?\d+)(?:\.\d+)?$", material.name)
    return (int(match.group(1)), int(match.group(2))) if match else None


def atlas_material(image):
    """Crée le matériau unique utilisé par l'orthophoto allégée."""
    material = bpy.data.materials.get("Orthophoto · atlas optimisé") or bpy.data.materials.new(
        "Orthophoto · atlas optimisé"
    )
    material.use_nodes = True
    nodes = material.node_tree.nodes
    links = material.node_tree.links
    nodes.clear()
    output = nodes.new("ShaderNodeOutputMaterial")
    shader = nodes.new("ShaderNodeBsdfPrincipled")
    texture = nodes.new("ShaderNodeTexImage")
    texture.image = image
    texture.interpolation = "Linear"
    texture.extension = "EXTEND"
    shader.inputs["Roughness"].default_value = 0.9
    links.new(texture.outputs["Color"], shader.inputs["Base Color"])
    links.new(shader.outputs["BSDF"], output.inputs["Surface"])
    return material


def optimize_orthophoto(settings):
    """Regroupe les milliers de JPEG natifs dans un atlas borné et remappe les UV."""
    projection_requested = settings.get("texture_roofs", False) or (
        settings.get("forest_material_mode") == "texture"
        and settings.get("tree_shape") == "canopy"
    )
    if settings.get("orthophoto_storage", "atlas") != "atlas" and not projection_requested:
        log_event("Orthophoto : tuiles natives conservées")
        return
    if settings.get("orthophoto_storage", "atlas") != "atlas":
        log_event(
            "Orthophoto : création d'un atlas unique nécessaire à la projection verticale"
        )
    source = next(
        (
            obj
            for obj in bpy.data.objects
            if obj.type == "MESH" and obj.name.startswith("Orthophoto native")
        ),
        None,
    )
    if source is None or not source.data.materials or source.data.uv_layers.active is None:
        log_event("Orthophoto : aucune surface texturée à regrouper")
        return
    used_materials = sorted({polygon.material_index for polygon in source.data.polygons})
    if not used_materials:
        return
    # Le nom du matériau conserve les vraies coordonnées (colonne, ligne) de la
    # matrice BD ORTHO. Elles sont plus fiables que l'emprise XY Lambert du morceau :
    # la reprojection peut faire pivoter légèrement les dalles et fausser leur tri.
    tile_keys = {}
    for material_index in used_materials:
        material = (
            source.data.materials[material_index]
            if material_index < len(source.data.materials)
            else None
        )
        key = tile_key_from_material(material)
        if key is not None:
            tile_keys[material_index] = key
    exact_tile_layout = len(tile_keys) == len(used_materials) and len(
        set(tile_keys.values())
    ) == len(used_materials)
    material_bounds = {}
    for polygon in source.data.polygons:
        points = [source.data.vertices[index].co for index in polygon.vertices]
        xmin = min(point.x for point in points)
        ymin = min(point.y for point in points)
        xmax = max(point.x for point in points)
        ymax = max(point.y for point in points)
        previous = material_bounds.get(polygon.material_index)
        material_bounds[polygon.material_index] = (
            min(previous[0], xmin) if previous else xmin,
            min(previous[1], ymin) if previous else ymin,
            max(previous[2], xmax) if previous else xmax,
            max(previous[3], ymax) if previous else ymax,
        )
    if exact_tile_layout:
        minimum_column = min(key[0] for key in tile_keys.values())
        maximum_column = max(key[0] for key in tile_keys.values())
        minimum_row = min(key[1] for key in tile_keys.values())
        maximum_row = max(key[1] for key in tile_keys.values())
        columns = maximum_column - minimum_column + 1
        rows = maximum_row - minimum_row + 1
        # Les lignes WMTS augmentent vers le sud, alors que V augmente vers le nord.
        material_cells = {
            material_index: (
                key[0] - minimum_column,
                maximum_row - key[1],
            )
            for material_index, key in tile_keys.items()
        }
    else:
        centers = {
            material_index: (
                round((bounds[0] + bounds[2]) * 0.5, 3),
                round((bounds[1] + bounds[3]) * 0.5, 3),
            )
            for material_index, bounds in material_bounds.items()
        }
        x_positions = sorted({center[0] for center in centers.values()})
        y_positions = sorted({center[1] for center in centers.values()})
        columns = len(x_positions)
        rows = len(y_positions)
        spatial_layout = bool(columns and rows) and columns * rows <= len(used_materials) * 4
        if spatial_layout:
            x_index = {value: index for index, value in enumerate(x_positions)}
            y_index = {value: index for index, value in enumerate(y_positions)}
            material_cells = {
                material_index: (x_index[center[0]], y_index[center[1]])
                for material_index, center in centers.items()
            }
        else:
            columns = max(1, ceil(sqrt(len(used_materials))))
            rows = ceil(len(used_materials) / columns)
            material_cells = {
                material_index: (atlas_index % columns, atlas_index // columns)
                for atlas_index, material_index in enumerate(used_materials)
            }
    requested = max(512, min(8192, int(settings.get("orthophoto_resolution_px", 2048))))
    cell_size = max(2, requested // max(columns, rows))
    width = columns * cell_size
    height = rows * cell_size
    fallback = tuple(settings.get("water_color", (0.05, 0.42, 0.68, 1.0)))
    atlas_pixels = array("f", fallback) * (width * height)
    source_images = set()
    missing_materials = set()
    report_interval = max(1, len(used_materials) // 10)
    log_event(f"Atlas orthophoto : 0/{len(used_materials):,} tuiles")
    for atlas_index, material_index in enumerate(used_materials):
        column, row = material_cells[material_index]
        material = (
            source.data.materials[material_index]
            if material_index < len(source.data.materials)
            else None
        )
        image = image_from_material(material)
        if image is None or image.size[0] <= 0 or image.size[1] <= 0:
            missing_materials.add(material_index)
            continue
        source_images.add(image)
        # Blender redimensionne l'image en code natif. Copier ensuite des lignes
        # complètes évite plusieurs millions d'accès Python à image.pixels.
        if tuple(image.size) != (cell_size, cell_size):
            image.scale(cell_size, cell_size)
        cell_pixels = array("f", [0.0]) * (cell_size * cell_size * 4)
        image.pixels.foreach_get(cell_pixels)
        row_width = cell_size * 4
        for target_y in range(cell_size):
            source_start = target_y * row_width
            target_start = (
                (row * cell_size + target_y) * width + column * cell_size
            ) * 4
            atlas_pixels[target_start : target_start + row_width] = cell_pixels[
                source_start : source_start + row_width
            ]
        completed = atlas_index + 1
        if completed % report_interval == 0 or completed == len(used_materials):
            log_event(f"Atlas orthophoto : {completed:,}/{len(used_materials):,} tuiles")
    atlas = bpy.data.images.new("Orthophoto · atlas optimisé", width=width, height=height, alpha=False)
    atlas.pixels.foreach_set(atlas_pixels)
    atlas.update()
    atlas.pack()
    uv_data = source.data.uv_layers.active.data
    water_vertices = set()
    for polygon in source.data.polygons:
        if polygon.material_index in missing_materials:
            water_vertices.update(polygon.vertices)
        column, row = material_cells[polygon.material_index]
        for loop_index in polygon.loop_indices:
            uv = uv_data[loop_index].uv
            uv.x = (column * cell_size + 0.5 + uv.x * (cell_size - 1)) / width
            uv.y = (row * cell_size + 0.5 + uv.y * (cell_size - 1)) / height
    if water_vertices:
        sea_level = min(vertex.co.z for vertex in source.data.vertices)
        for vertex_index in water_vertices:
            source.data.vertices[vertex_index].co.z = sea_level
    previous_materials = list(source.data.materials)
    source.data.materials.clear()
    source.data.materials.append(atlas_material(atlas))
    for polygon in source.data.polygons:
        polygon.material_index = 0
    unused_materials = [
        material
        for material in previous_materials
        if material is not None and material.users == 0
    ]
    if unused_materials:
        bpy.data.batch_remove(ids=unused_materials)
    unused_images = [image for image in source_images if image.users == 0]
    if unused_images:
        bpy.data.batch_remove(ids=unused_images)
    source["orthophoto_storage"] = "atlas"
    source["orthophoto_resolution"] = (width, height)
    source["orthophoto_projection_mode"] = (
        "grille_wmts_exacte" if exact_tile_layout else "emprises_geometriques"
    )
    if "orthophoto_spatial_bounds" in source:
        del source["orthophoto_spatial_bounds"]
    if "orthophoto_spatial_layout" in source:
        del source["orthophoto_spatial_layout"]
    source["orthophoto_missing_materials"] = len(missing_materials)
    source["orthophoto_water_vertices"] = len(water_vertices)
    log_event(
        f"Orthophoto : {len(used_materials):,} tuiles regroupées dans un atlas "
        f"{width} × {height} px, disposition "
        f"{'WMTS exacte' if exact_tile_layout else 'géométrique'} ; "
        f"{len(missing_materials):,} tuile(s) absente(s) colorée(s) en eau"
    )


def sample_image(image, uv):
    """Échantillonne une couleur RGBA en bornant proprement les coordonnées de bord."""
    width, height = image.size
    if width <= 0 or height <= 0:
        return None
    x = round(min(1.0, max(0.0, uv.x)) * (width - 1))
    y = round(min(1.0, max(0.0, uv.y)) * (height - 1))
    offset = (y * width + x) * 4
    try:
        return tuple(float(image.pixels[offset + channel]) for channel in range(4))
    except (IndexError, RuntimeError):
        return None


def polygon_uv_at_xy(source, polygon, point):
    """Interpole les UV dans le plan XY, sans lancer de rayon dans tout le maillage."""
    uv_layer = source.data.uv_layers.active
    if uv_layer is None or len(polygon.loop_indices) < 3:
        return None
    loops = list(polygon.loop_indices)
    anchor = loops[0]
    point_a_3d = source.data.vertices[source.data.loops[anchor].vertex_index].co
    point_a = Vector((point_a_3d.x, point_a_3d.y))
    sample = Vector((point.x, point.y))
    uv_a = uv_layer.data[anchor].uv.copy()
    for index in range(1, len(loops) - 1):
        loop_b = loops[index]
        loop_c = loops[index + 1]
        point_b_3d = source.data.vertices[source.data.loops[loop_b].vertex_index].co
        point_c_3d = source.data.vertices[source.data.loops[loop_c].vertex_index].co
        point_b = Vector((point_b_3d.x, point_b_3d.y))
        point_c = Vector((point_c_3d.x, point_c_3d.y))
        vector_0 = point_b - point_a
        vector_1 = point_c - point_a
        vector_2 = sample - point_a
        dot_00 = vector_0.dot(vector_0)
        dot_01 = vector_0.dot(vector_1)
        dot_11 = vector_1.dot(vector_1)
        dot_20 = vector_2.dot(vector_0)
        dot_21 = vector_2.dot(vector_1)
        denominator = dot_00 * dot_11 - dot_01 * dot_01
        if abs(denominator) <= 1e-12:
            continue
        weight_b = (dot_11 * dot_20 - dot_01 * dot_21) / denominator
        weight_c = (dot_00 * dot_21 - dot_01 * dot_20) / denominator
        weight_a = 1.0 - weight_b - weight_c
        if min(weight_a, weight_b, weight_c) < -0.001:
            continue
        uv_b = uv_layer.data[loop_b].uv
        uv_c = uv_layer.data[loop_c].uv
        return uv_a * weight_a + uv_b * weight_b + uv_c * weight_c
    return None


class OrthophotoSampler:
    """Indexe les dalles par leur emprise XY pour échantillonner en temps quasi constant."""

    def __init__(self, source):
        self.source = source
        self.inverse = source.matrix_world.inverted()
        self.bounds = {}
        sizes = []
        for polygon in source.data.polygons:
            points = [source.data.vertices[index].co for index in polygon.vertices]
            xmin = min(point.x for point in points)
            ymin = min(point.y for point in points)
            xmax = max(point.x for point in points)
            ymax = max(point.y for point in points)
            self.bounds[polygon.index] = (xmin, ymin, xmax, ymax)
            width = xmax - xmin
            height = ymax - ymin
            if width > 0.001 and height > 0.001:
                sizes.append(max(width, height))
        self.cell_size = max(1.0, median(sizes) if sizes else 100.0)
        self.cells = {}
        for polygon_index, (xmin, ymin, xmax, ymax) in self.bounds.items():
            for cell_x in range(floor(xmin / self.cell_size), floor(xmax / self.cell_size) + 1):
                for cell_y in range(floor(ymin / self.cell_size), floor(ymax / self.cell_size) + 1):
                    self.cells.setdefault((cell_x, cell_y), []).append(polygon_index)
        self.atlas_image = None
        self.atlas_pixels = None
        if len(source.data.materials) == 1 and source.data.polygons:
            self.atlas_image = orthophoto_image(source, source.data.polygons[0])
            if self.atlas_image is not None:
                pixel_count = self.atlas_image.size[0] * self.atlas_image.size[1] * 4
                self.atlas_pixels = array("f", [0.0]) * pixel_count
                self.atlas_image.pixels.foreach_get(self.atlas_pixels)

    def color(self, world_point):
        """Retourne la couleur du polygone situé sous un point monde."""
        point = self.inverse @ world_point
        key = (floor(point.x / self.cell_size), floor(point.y / self.cell_size))
        for polygon_index in self.cells.get(key, ()):
            xmin, ymin, xmax, ymax = self.bounds[polygon_index]
            inside_x = xmin - 0.001 <= point.x <= xmax + 0.001
            inside_y = ymin - 0.001 <= point.y <= ymax + 0.001
            if not (inside_x and inside_y):
                continue
            polygon = self.source.data.polygons[polygon_index]
            uv = polygon_uv_at_xy(self.source, polygon, point)
            if uv is None:
                continue
            image = orthophoto_image(self.source, polygon)
            if image is None:
                continue
            if image == self.atlas_image and self.atlas_pixels is not None:
                width, height = image.size
                x = round(min(1.0, max(0.0, uv.x)) * (width - 1))
                y = round(min(1.0, max(0.0, uv.y)) * (height - 1))
                offset = (y * width + x) * 4
                return tuple(self.atlas_pixels[offset + channel] for channel in range(4))
            return sample_image(image, uv)
        return None

    def uv(self, world_point):
        """Projette verticalement un point monde dans les UV exactes de l'orthophoto."""
        point = self.inverse @ world_point
        key = (floor(point.x / self.cell_size), floor(point.y / self.cell_size))
        for polygon_index in self.cells.get(key, ()):
            xmin, ymin, xmax, ymax = self.bounds[polygon_index]
            if not (
                xmin - 0.001 <= point.x <= xmax + 0.001
                and ymin - 0.001 <= point.y <= ymax + 0.001
            ):
                continue
            uv = polygon_uv_at_xy(
                self.source, self.source.data.polygons[polygon_index], point
            )
            if uv is not None:
                return uv
        return None


_ORTHOPHOTO_SAMPLERS = {}


def orthophoto_sampler(source):
    """Réutilise le même index et le même tampon de pixels pour les toits et la forêt."""
    key = source.as_pointer()
    if key not in _ORTHOPHOTO_SAMPLERS:
        _ORTHOPHOTO_SAMPLERS[key] = OrthophotoSampler(source)
    return _ORTHOPHOTO_SAMPLERS[key]


def project_orthophoto_material(target, settings, polygon_indices=None):
    """Plaque directement l'atlas orthophoto sur les faces demandées via leurs UV."""
    started_at = perf_counter()
    source = next(
        (
            obj
            for obj in bpy.data.objects
            if obj.type == "MESH" and obj.name.startswith("Orthophoto native")
        ),
        None,
    )
    if source is None or not source.data.materials or source.data.uv_layers.active is None:
        return False
    if len(source.data.materials) != 1:
        log_event("Projection orthophoto impossible : l'atlas unique est absent")
        return False
    material = source.data.materials[0]
    if material is None or image_from_material(material) is None:
        return False
    uv_layer = target.data.uv_layers.get("Projection orthophoto")
    if uv_layer is None:
        uv_layer = target.data.uv_layers.new(name="Projection orthophoto")
    target.data.uv_layers.active = uv_layer
    uv_layer.active_render = True
    sampler = orthophoto_sampler(source)
    material_index = next(
        (
            index
            for index, candidate in enumerate(target.data.materials)
            if candidate == material
        ),
        None,
    )
    if material_index is None:
        target.data.materials.append(material)
        material_index = len(target.data.materials) - 1
    selected = (
        list(target.data.polygons)
        if polygon_indices is None
        else [target.data.polygons[index] for index in polygon_indices]
    )
    projected_faces = 0
    projected_loops = 0
    for polygon in selected:
        projected = []
        for loop_index in polygon.loop_indices:
            vertex = target.data.vertices[target.data.loops[loop_index].vertex_index]
            uv = sampler.uv(target.matrix_world @ vertex.co)
            if uv is None:
                projected = []
                break
            projected.append((loop_index, uv))
        if not projected:
            continue
        for loop_index, uv in projected:
            uv_layer.data[loop_index].uv = uv
        polygon.material_index = material_index
        projected_faces += 1
        projected_loops += len(projected)
    target["projection_orthophoto_faces"] = projected_faces
    target["projection_orthophoto_couverture"] = projected_faces / max(1, len(selected))
    target["projection_orthophoto_mode"] = source.get(
        "orthophoto_projection_mode", "dalles_indexees"
    )
    log_event(
        f"Projection orthophoto UV : {projected_faces:,}/{len(selected):,} faces, "
        f"{projected_loops:,} coordonnées UV, 1 atlas",
        started_at,
    )
    return projected_faces > 0


def forest_side_color_material(attribute_name):
    """Affiche sur les lisières une couleur échantillonnée, sans étirer l'image verticale."""
    name = "Lisières · couleurs de l'orthophoto"
    result = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    result.use_nodes = True
    nodes = result.node_tree.nodes
    links = result.node_tree.links
    nodes.clear()
    output = nodes.new("ShaderNodeOutputMaterial")
    shader = nodes.new("ShaderNodeBsdfPrincipled")
    try:
        colors = nodes.new("ShaderNodeVertexColor")
        colors.layer_name = attribute_name
    except RuntimeError:
        colors = nodes.new("ShaderNodeAttribute")
        colors.attribute_name = attribute_name
    shader.inputs["Roughness"].default_value = 0.9
    links.new(colors.outputs["Color"], shader.inputs["Base Color"])
    links.new(shader.outputs["BSDF"], output.inputs["Surface"])
    return result


def color_forest_sides(target, polygon_indices):
    """Échantillonne l'orthophoto aux bords et assombrit légèrement les faces verticales."""
    source = next(
        (
            obj
            for obj in bpy.data.objects
            if obj.type == "MESH" and obj.name.startswith("Orthophoto native")
        ),
        None,
    )
    if source is None or not source.data.materials or source.data.uv_layers.active is None:
        return None
    attribute_name = "Couleur orthophoto des lisières"
    previous = target.data.color_attributes.get(attribute_name)
    if previous is not None:
        target.data.color_attributes.remove(previous)
    colors = target.data.color_attributes.new(
        name=attribute_name,
        type="BYTE_COLOR",
        domain="CORNER",
    )
    sampler = orthophoto_sampler(source)
    material = forest_side_color_material(attribute_name)
    target.data.materials.append(material)
    material_index = len(target.data.materials) - 1
    colored = 0
    for polygon_index in polygon_indices:
        polygon = target.data.polygons[polygon_index]
        projected = []
        for loop_index in polygon.loop_indices:
            vertex = target.data.vertices[target.data.loops[loop_index].vertex_index]
            color = sampler.color(target.matrix_world @ vertex.co)
            if color is None:
                projected = []
                break
            projected.append((loop_index, varied_color(color, 0.5)))
        if not projected:
            continue
        for loop_index, color in projected:
            colors.data[loop_index].color = color
        polygon.material_index = material_index
        colored += 1
    log_event(
        f"Lisières : {colored:,}/{len(polygon_indices):,} faces colorées depuis l'orthophoto"
    )
    return material_index if colored else None


def terrain_surface_object():
    """Retrouve le maillage altimétrique sans confondre terrain et orthophoto."""
    collection = bpy.data.collections.get("terrain")
    candidates = (
        [obj for obj in collection.all_objects if obj.type == "MESH"]
        if collection is not None
        else []
    )
    if not candidates:
        candidates = [
            obj
            for obj in bpy.data.objects
            if obj.type == "MESH" and obj.name.lower().startswith("terrain")
        ]
    candidates = [obj for obj in candidates if obj.data.polygons]
    if not candidates:
        return None
    return max(
        candidates,
        key=lambda obj: (
            obj.name.lower().startswith("terrain alti"),
            len(obj.data.polygons),
        ),
    )


def drape_roads(settings):
    """Plaque chaque sommet routier sur le relief puis applique le faible décalage choisi."""
    elevation = max(0.0, float(settings.get("road_elevation_m", 0.0)))
    target = bpy.data.collections.get("bd_topo.troncon_de_route")
    if target is None:
        return
    roads = [obj for obj in target.all_objects if obj.type == "MESH"]
    terrain = terrain_surface_object()
    if terrain is None:
        log_event("Routes : terrain altimétrique introuvable, géométrie conservée")
        return
    depsgraph = bpy.context.evaluated_depsgraph_get()
    terrain_tree = BVHTree.FromObject(terrain, depsgraph)
    if terrain_tree is None:
        log_event("Routes : index du terrain indisponible, géométrie conservée")
        return
    terrain_inverse = terrain.matrix_world.inverted()
    terrain_top = max(
        (terrain.matrix_world @ Vector(corner)).z for corner in terrain.bound_box
    )
    ray_top = terrain_top + max(1_000.0, terrain.dimensions.z * 2.0)
    local_direction = terrain_inverse.to_3x3() @ Vector((0.0, 0.0, -1.0))
    local_direction.normalize()
    projected_vertices = 0
    missed_vertices = 0
    for road in roads:
        if road.data.users > 1:
            road.data = road.data.copy()
        road_inverse = road.matrix_world.inverted()
        world_points = [road.matrix_world @ vertex.co for vertex in road.data.vertices]
        groups = {}
        for vertex_index, point in enumerate(world_points):
            groups.setdefault((round(point.x, 6), round(point.y, 6)), []).append(vertex_index)
        for indices in groups.values():
            reference = world_points[indices[0]]
            origin_world = Vector((reference.x, reference.y, ray_top))
            origin_local = terrain_inverse @ origin_world
            hit, _normal, _face_index, _distance = terrain_tree.ray_cast(
                origin_local, local_direction, 10_000_000.0
            )
            if hit is None:
                missed_vertices += len(indices)
                continue
            surface_z = (terrain.matrix_world @ hit).z
            original_top = max(world_points[index].z for index in indices)
            for vertex_index in indices:
                point = world_points[vertex_index]
                new_world = Vector(
                    (
                        point.x,
                        point.y,
                        surface_z + elevation - (original_top - point.z),
                    )
                )
                road.data.vertices[vertex_index].co = road_inverse @ new_world
                projected_vertices += 1
        road.data.update()
    log_event(
        f"Routes : {projected_vertices:,} sommet(s) plaqué(s) sur le terrain, "
        f"décalage {elevation:.2f} m, {missed_vertices:,} hors terrain"
    )


def apply_building_colors(settings):
    """Pose l'orthophoto sur les toits et un enduit bruité léger sur les murs."""
    target = bpy.data.collections.get("bd_topo.batiment")
    if target is None:
        return
    wall = wall_procedural_material(settings)
    roof = get_or_create_material("Toits personnalisés", settings["roof_color"], False)
    source = next(
        (
            obj
            for obj in bpy.data.objects
            if obj.type == "MESH" and obj.name.startswith("Orthophoto native")
        ),
        None,
    )
    projected_roofs = (
        settings.get("texture_roofs", False)
        and source is not None
        and len(source.data.materials) == 1
        and image_from_material(source.data.materials[0]) is not None
    )
    if projected_roofs:
        log_event("Toits : projection verticale de l'atlas orthophoto")
    released_materials = set()
    released_images = set()
    mesh_objects = [obj for obj in target.all_objects if obj.type == "MESH"]
    report_interval = max(1, len(mesh_objects) // 10)
    log_event(f"Bâtiments : 0/{len(mesh_objects):,} objets")
    for object_index, obj in enumerate(mesh_objects):
        for material in obj.data.materials:
            if material is None:
                continue
            released_materials.add(material)
            image = image_from_material(material)
            if image is not None:
                released_images.add(image)
        obj.data.materials.clear()
        obj.data.materials.append(wall)
        obj.data.materials.append(roof)
        roof_polygons = []
        normal_matrix = obj.matrix_world.to_3x3()
        for polygon in obj.data.polygons:
            world_normal = normal_matrix @ polygon.normal
            if world_normal.z >= 0.45:
                polygon.material_index = 1
                roof_polygons.append(polygon.index)
            else:
                polygon.material_index = 0
        if projected_roofs and roof_polygons:
            project_orthophoto_material(obj, settings, roof_polygons)
        completed = object_index + 1
        if completed % report_interval == 0 or completed == len(mesh_objects):
            log_event(f"Bâtiments : {completed:,}/{len(mesh_objects):,} objets")
    unused_materials = [material for material in released_materials if material.users == 0]
    if unused_materials:
        cleanup_started_at = perf_counter()
        log_event(f"Bâtiments : suppression groupée de {len(unused_materials):,} matériaux")
        bpy.data.batch_remove(ids=unused_materials)
        log_event("Bâtiments : anciens matériaux supprimés", cleanup_started_at)
    unused_images = [image for image in released_images if image.users == 0]
    if unused_images:
        cleanup_started_at = perf_counter()
        log_event(f"Bâtiments : suppression groupée de {len(unused_images):,} images")
        bpy.data.batch_remove(ids=unused_images)
        log_event("Bâtiments : anciennes images supprimées", cleanup_started_at)


def create_conifer_mesh(material):
    """Construit un feuillage conique étagé partagé par tous les sapins."""
    mesh = bpy.data.meshes.new("Feuillage conifère")
    builder = bmesh.new()
    for radius, center in [(1.0, -0.45), (0.78, 0.05), (0.56, 0.55)]:
        bmesh.ops.create_cone(
            builder,
            cap_ends=True,
            cap_tris=False,
            segments=12,
            radius1=radius,
            radius2=0.0,
            depth=1.1,
            matrix=Matrix.Translation((0.0, 0.0, center)),
        )
    builder.to_mesh(mesh)
    builder.free()
    mesh.materials.append(material)
    return mesh


def estimate_canopy_cell_size(crowns):
    """Déduit une maille robuste depuis les voisins, même si les arbres ne sont pas alignés."""
    points = [crown.matrix_world.translation.xy for crown in crowns]
    if len(points) < 2:
        return None
    stride = max(1, len(points) // 320)
    distances = []
    for index in range(0, len(points), stride):
        origin = points[index]
        nearest = min(
            (origin - candidate).length
            for candidate_index, candidate in enumerate(points)
            if candidate_index != index
        )
        if nearest > 0.001:
            distances.append(nearest)
    return median(distances) * 1.2 if distances else None


def crown_top(crown):
    """Lit l'altitude supérieure réelle d'un houppier transformé."""
    return max((crown.matrix_world @ Vector(corner)).z for corner in crown.bound_box)


def scene_road_segments():
    """Lit les axes ou arêtes de la collection routière dans le repère local de la scène."""
    target = bpy.data.collections.get("bd_topo.troncon_de_route")
    if target is None:
        return []
    segments = []
    for obj in target.all_objects:
        transform = obj.matrix_world
        if obj.type == "CURVE":
            for spline in obj.data.splines:
                points = [transform @ Vector(point.co[:3]) for point in spline.points]
                segments.extend(zip(points, points[1:]))
        elif obj.type == "MESH":
            for edge in obj.data.edges:
                start = transform @ obj.data.vertices[edge.vertices[0]].co
                end = transform @ obj.data.vertices[edge.vertices[1]].co
                segments.append((start, end))
    return segments


def distance_to_segment_2d(point, start, end):
    """Retourne la distance plane entre un centre de cellule et un segment routier."""
    delta = Vector((end.x - start.x, end.y - start.y))
    length_squared = delta.length_squared
    if length_squared <= 1e-12:
        return (point - Vector((start.x, start.y))).length
    ratio = max(
        0.0,
        min(1.0, (point - Vector((start.x, start.y))).dot(delta) / length_squared),
    )
    nearest = Vector((start.x, start.y)) + delta * ratio
    return (point - nearest).length


def carve_active_cells_at_roads(
    active, origin_x, origin_y, cell_size, excluded_tree_cells=None
):
    """Crée un corridor vide dans la forêt autour des routes visibles de la scène."""
    segments = scene_road_segments()
    if not segments:
        return 0
    before = len(active)
    clearance = max(3.0, cell_size * 0.72)
    candidates = set()
    for start, end in segments:
        minimum_column = int((min(start.x, end.x) - clearance - origin_x) // cell_size)
        maximum_column = int((max(start.x, end.x) + clearance - origin_x) // cell_size)
        minimum_row = int((min(start.y, end.y) - clearance - origin_y) // cell_size)
        maximum_row = int((max(start.y, end.y) + clearance - origin_y) // cell_size)
        for column in range(minimum_column, maximum_column + 1):
            for row in range(minimum_row, maximum_row + 1):
                center = Vector(
                    (
                        origin_x + (column + 0.5) * cell_size,
                        origin_y + (row + 0.5) * cell_size,
                    )
                )
                if distance_to_segment_2d(center, start, end) <= clearance:
                    candidates.add((column, row))
    if excluded_tree_cells is not None:
        excluded_tree_cells.update(candidates)
    active.difference_update(candidates)
    removed = before - len(active)
    print("GEO_PLATFORM_ROAD_CUT_OK", removed, "cellules retirées")
    return removed


def read_lidar_canopy_cache(path):
    """Lit les vraies cellules végétales du cache GSTL, sans les confondre avec la surface."""
    with Path(path).open("rb") as source:
        if source.read(8) != b"GSTLC04\0":
            raise RuntimeError("signature du cache LiDAR inconnue")
        header_size = struct.unpack("<I", source.read(4))[0]
        header = json.loads(source.read(header_size).decode("utf-8"))
        key = header["key"]
        length = struct.unpack("<Q", source.read(8))[0]
        source.seek(length * 8 * 2, 1)
        ground = array("d")
        ground.frombytes(source.read(length * 8))
        if sys.byteorder != "little":
            ground.byteswap()
        point_count = struct.unpack("<Q", source.read(8))[0]
        payload = source.read(point_count * 24)
    if len(ground) != length or len(payload) != point_count * 24:
        raise RuntimeError("cache LiDAR tronqué")
    points = list(struct.iter_unpack("<ddd", payload))
    return key, ground, points


def grid_height(grid, count, xmin, ymin, side, x, y):
    """Interpole la grille de sol du cache à la position Lambert-93 demandée."""
    step = side / max(1, count - 1)
    u = min(count - 1.0, max(0.0, (x - xmin) / step))
    v = min(count - 1.0, max(0.0, (y - ymin) / step))
    column = min(count - 2, int(u))
    row = min(count - 2, int(v))
    a = u - column
    b = v - row
    return (
        grid[row * count + column] * (1.0 - a) * (1.0 - b)
        + grid[row * count + column + 1] * a * (1.0 - b)
        + grid[(row + 1) * count + column] * (1.0 - a) * b
        + grid[(row + 1) * count + column + 1] * a * b
    )


def remove_small_forest_patches(active, minimum_cells):
    """Écarte arbres isolés, jardins et haies trop petites pour former une forêt continue."""
    remaining = set(active)
    kept = set()
    removed = 0
    while remaining:
        seed = remaining.pop()
        component = {seed}
        stack = [seed]
        while stack:
            column, row = stack.pop()
            for neighbour in (
                (column + 1, row),
                (column - 1, row),
                (column, row + 1),
                (column, row - 1),
            ):
                if neighbour in remaining:
                    remaining.remove(neighbour)
                    component.add(neighbour)
                    stack.append(neighbour)
        if len(component) >= minimum_cells:
            kept.update(component)
        else:
            removed += len(component)
    return kept, removed


def carve_active_cells_at_buildings(
    active, origin_x, origin_y, cell_size, clearance, excluded_tree_cells=None
):
    """Retire la canopée sur et autour du bâti pour ne pas transformer un village en forêt."""
    target = bpy.data.collections.get("bd_topo.batiment")
    if target is None:
        return 0
    before = len(active)
    for obj in target.all_objects:
        if obj.type != "MESH":
            continue
        corners = [obj.matrix_world @ Vector(corner) for corner in obj.bound_box]
        xmin = min(point.x for point in corners) - clearance
        ymin = min(point.y for point in corners) - clearance
        xmax = max(point.x for point in corners) + clearance
        ymax = max(point.y for point in corners) + clearance
        minimum_column = floor((xmin - origin_x) / cell_size)
        maximum_column = floor((xmax - origin_x) / cell_size)
        minimum_row = floor((ymin - origin_y) / cell_size)
        maximum_row = floor((ymax - origin_y) / cell_size)
        for column in range(minimum_column, maximum_column + 1):
            for row in range(minimum_row, maximum_row + 1):
                if excluded_tree_cells is not None:
                    excluded_tree_cells.add((column, row))
                active.discard((column, row))
    return before - len(active)


def infer_altitude_origin(crowns, samples, half_side, cell_size, exaggeration, fallback):
    """Retrouve l'origine verticale Blender depuis les houppiers déjà correctement placés."""
    estimates = []
    for crown in crowns[:80]:
        position = crown.matrix_world.translation
        key = (
            floor((position.x + half_side) / cell_size),
            floor((position.y + half_side) / cell_size),
        )
        sample = samples.get(key)
        if sample is not None:
            estimates.append(sample[2] - crown_top(crown) / exaggeration)
    return median(estimates) if estimates else fallback


def isolated_tree_objects(
    settings, crowns, active, excluded_tree_cells, half_side, cell_size
):
    """Conserve sur demande les arbres ponctuels qui ne sont pas couverts par un massif."""
    if not settings.get("keep_isolated_trees", False):
        return set(), set()
    preserved = set()
    occupied = set()
    for crown in crowns:
        position = crown.matrix_world.translation
        key = (
            floor((position.x + half_side) / cell_size),
            floor((position.y + half_side) / cell_size),
        )
        if key in active or key in excluded_tree_cells:
            continue
        preserved.add(crown)
        occupied.add(key)
        trunk = bpy.data.objects.get(crown.name.replace("Houppier", "Tronc", 1))
        if trunk is not None:
            preserved.add(trunk)
    return preserved, occupied


def add_missing_isolated_trees(
    settings,
    target,
    crowns,
    preserved,
    occupied,
    raw_samples,
    active,
    excluded_tree_cells,
    xmin,
    ymin,
    side,
    altitude_origin,
    exaggeration,
):
    """Complète les arbres épars directement depuis le LiDAR quand l'échantillon est incomplet."""
    if not settings.get("keep_isolated_trees", False) or not crowns:
        return 0
    prototype_crown = crowns[0]
    prototype_trunk = bpy.data.objects.get(
        prototype_crown.name.replace("Houppier", "Tronc", 1)
    )
    if prototype_trunk is None:
        return 0
    maximum = 800
    existing = sum(obj.name.startswith("Houppier") for obj in preserved)
    budget = max(0, maximum - existing)
    candidates = sorted(
        sample_key
        for sample_key in raw_samples
        if sample_key not in active
        and sample_key not in excluded_tree_cells
        and sample_key not in occupied
    )
    if not candidates or budget == 0:
        return 0
    stride = max(1, ceil(len(candidates) / budget))
    selected = candidates[::stride][:budget]
    for index, sample_key in enumerate(selected):
        x, y, top, height = raw_samples[sample_key]
        local_x = x - xmin - side / 2.0
        local_y = y - ymin - side / 2.0
        scaled_height = height * exaggeration
        base = (top - height - altitude_origin) * exaggeration
        suffix = f"LiDAR {index:05}"
        trunk = bpy.data.objects.new(f"Tronc isolé {suffix}", prototype_trunk.data)
        trunk.location = (local_x, local_y, base + scaled_height * 0.25)
        trunk_radius = max(0.18, scaled_height * 0.025)
        trunk.scale = (trunk_radius, trunk_radius, scaled_height * 0.5)
        target.objects.link(trunk)
        crown = bpy.data.objects.new(f"Houppier isolé {suffix}", prototype_crown.data)
        crown.location = (local_x, local_y, base + scaled_height * 0.65)
        crown_radius = max(1.5, scaled_height * 0.25)
        crown.scale = (crown_radius, crown_radius * 0.9, scaled_height * 0.35)
        target.objects.link(crown)
        preserved.update((trunk, crown))
        occupied.add(sample_key)
    return len(selected)


def smooth_canopy_outline(
    vertices,
    side_faces,
    smoothing,
    quality,
    ground,
    count,
    xmin,
    ymin,
    side,
    altitude_origin,
    exaggeration,
):
    """Transforme le contour en polygone adouci plutôt qu'en escalier de cellules."""
    if smoothing <= 0.0 or not side_faces:
        return 0
    neighbours = {}
    bottom_for_top = {}
    for top_a, top_b, bottom_b, bottom_a in side_faces:
        neighbours.setdefault(top_a, set()).add(top_b)
        neighbours.setdefault(top_b, set()).add(top_a)
        bottom_for_top[top_a] = bottom_a
        bottom_for_top[top_b] = bottom_b
    passes = {"draft": 2, "standard": 3, "high": 4}.get(quality, 3)
    strength = 0.28 + smoothing * 0.14
    moved = 0
    for _ in range(passes):
        updates = {}
        for vertex_index, adjacent in neighbours.items():
            if len(adjacent) != 2:
                continue
            first, second = (vertices[index] for index in adjacent)
            current = vertices[vertex_index]
            target_x = (first[0] + second[0]) * 0.5
            target_y = (first[1] + second[1]) * 0.5
            updates[vertex_index] = (
                current[0] * (1.0 - strength) + target_x * strength,
                current[1] * (1.0 - strength) + target_y * strength,
                current[2],
            )
        for vertex_index, position in updates.items():
            vertices[vertex_index] = position
            bottom_index = bottom_for_top[vertex_index]
            bottom = vertices[bottom_index]
            lambert_x = xmin + position[0] + side / 2.0
            lambert_y = ymin + position[1] + side / 2.0
            terrain = grid_height(
                ground, count, xmin, ymin, side, lambert_x, lambert_y
            )
            vertices[bottom_index] = (
                position[0],
                position[1],
                (terrain - altitude_origin) * exaggeration,
            )
            moved += 1
    log_event(
        f"Lisières : {len(neighbours):,} sommets de contour lissés sur {passes} passes"
    )
    return moved


def apply_forest_canopy(settings, target, crowns):
    """Construit uniquement les massifs denses depuis les cellules végétales classées du LiDAR."""
    cache_path = settings.get("canopy_cache")
    if not cache_path:
        raise RuntimeError("cache LiDAR complet absent de la configuration Blender")
    key, ground, points = read_lidar_canopy_cache(cache_path)
    log_event(
        f"Forêt LiDAR : {len(points):,} cellules de végétation haute classées lues"
    )
    xmin = float(key["xmin"])
    ymin = float(key["ymin"])
    side = float(key["side_m"])
    count = int(key["count"])
    cell_size = float(key.get("vegetation_cell_size_m", 12.0))
    exaggeration = max(0.01, float(settings.get("vertical_exaggeration", 1.0)))
    raw_samples = {}
    for x, y, z in points:
        terrain = grid_height(ground, count, xmin, ymin, side, x, y)
        height = z - terrain
        if not 3.0 <= height <= 45.0:
            continue
        sample_key = (floor((x - xmin) / cell_size), floor((y - ymin) / cell_size))
        previous = raw_samples.get(sample_key)
        if previous is None or z > previous[2]:
            raw_samples[sample_key] = (x, y, z, height)
    quality = settings.get("forest_quality", "standard")
    # Une cellule ne devient forêt que si son voisinage est réellement continu.
    # La qualité modifie le détail du contour, jamais la nature de l'occupation du sol.
    density_needed = 7
    active = {
        sample_key
        for sample_key in raw_samples
        if sum(
            (sample_key[0] + dx, sample_key[1] + dy) in raw_samples
            for dx in (-1, 0, 1)
            for dy in (-1, 0, 1)
        )
        >= density_needed
    }
    smoothing = min(1.0, max(0.0, float(settings.get("canopy_smoothing", 0.7))))
    origin_x = -side / 2.0
    origin_y = -side / 2.0
    building_clearance = max(12.0, cell_size * 1.5)
    excluded_tree_cells = set()
    buildings_removed = carve_active_cells_at_buildings(
        active,
        origin_x,
        origin_y,
        cell_size,
        building_clearance,
        excluded_tree_cells,
    )
    if settings.get("carve_forest_roads"):
        carve_active_cells_at_roads(
            active, origin_x, origin_y, cell_size, excluded_tree_cells
        )
    minimum_patch = {"draft": 50, "standard": 35, "high": 25}.get(quality, 35)
    active, isolated_removed = remove_small_forest_patches(active, minimum_patch)
    log_event(
        f"Forêt LiDAR : {len(active):,} cellules denses conservées, "
        f"{isolated_removed:,} cellules isolées retirées, "
        f"{buildings_removed:,} cellules retirées sur les bâtiments"
    )
    fallback_origin = min(ground) if ground else 0.0
    altitude_origin = infer_altitude_origin(
        crowns, raw_samples, side / 2.0, cell_size, exaggeration, fallback_origin
    )
    sample_tops = {
        sample_key: (sample[2] - altitude_origin) * exaggeration
        for sample_key, sample in raw_samples.items()
    }
    if smoothing > 0.0:
        original_tops = sample_tops.copy()
        blend = 0.15 + smoothing * 0.3
        for column, row in active:
            neighbours = [
                original_tops[(column + dx, row + dy)]
                for dx in (-1, 0, 1)
                for dy in (-1, 0, 1)
                if (column + dx, row + dy) in active
                and (column + dx, row + dy) in original_tops
            ]
            if neighbours:
                sample_tops[(column, row)] = (
                    original_tops[(column, row)] * (1.0 - blend)
                    + median(neighbours) * blend
                )
    vertices = []
    top_faces = []
    bottom_faces = []
    side_faces = []
    vertical_boundary_edges = set()
    top_vertex_index = {}
    bottom_vertex_index = {}

    def height_at(column, row):
        """Utilise la médiane des cellules végétales qui touchent le sommet."""
        candidates = [
            sample_tops[(column + dx, row + dy)]
            for dx in (-1, 0)
            for dy in (-1, 0)
            if (column + dx, row + dy) in sample_tops
        ]
        return median(candidates) if candidates else None

    def top_vertex(column, row):
        """Crée un sommet supérieur partagé sans prolonger la forêt hors de ses mesures."""
        vertex_key = (column, row)
        if vertex_key in top_vertex_index:
            return top_vertex_index[vertex_key]
        height = height_at(column, row)
        if height is None:
            return None
        top_vertex_index[vertex_key] = len(vertices)
        vertices.append(
            (origin_x + column * cell_size, origin_y + row * cell_size, height)
        )
        return top_vertex_index[vertex_key]

    def bottom_vertex(column, row):
        """Place le pied du volume forestier exactement sur le terrain LiDAR."""
        vertex_key = (column, row)
        if vertex_key in bottom_vertex_index:
            return bottom_vertex_index[vertex_key]
        lambert_x = xmin + column * cell_size
        lambert_y = ymin + row * cell_size
        terrain = grid_height(
            ground, count, xmin, ymin, side, lambert_x, lambert_y
        )
        height = (terrain - altitude_origin) * exaggeration
        bottom_vertex_index[vertex_key] = len(vertices)
        vertices.append(
            (origin_x + column * cell_size, origin_y + row * cell_size, height)
        )
        return bottom_vertex_index[vertex_key]

    for column, row in sorted(active):
        top = [
            top_vertex(column, row),
            top_vertex(column + 1, row),
            top_vertex(column + 1, row + 1),
            top_vertex(column, row + 1),
        ]
        if not all(index is not None for index in top):
            continue
        bottom = [
            bottom_vertex(column, row),
            bottom_vertex(column + 1, row),
            bottom_vertex(column + 1, row + 1),
            bottom_vertex(column, row + 1),
        ]
        top_faces.append(tuple(top))
        bottom_faces.append(tuple(reversed(bottom)))
        boundary_edges = [
            ((column, row - 1), (top[0], top[1], bottom[1], bottom[0])),
            ((column + 1, row), (top[1], top[2], bottom[2], bottom[1])),
            ((column, row + 1), (top[2], top[3], bottom[3], bottom[2])),
            ((column - 1, row), (top[3], top[0], bottom[0], bottom[3])),
        ]
        for neighbour, face in boundary_edges:
            if neighbour not in active:
                side_faces.append(face)
                vertical_boundary_edges.add(tuple(sorted((face[0], face[3]))))
                vertical_boundary_edges.add(tuple(sorted((face[1], face[2]))))
    smooth_canopy_outline(
        vertices,
        side_faces,
        smoothing,
        quality,
        ground,
        count,
        xmin,
        ymin,
        side,
        altitude_origin,
        exaggeration,
    )
    preserved_trees, occupied_tree_cells = isolated_tree_objects(
        settings,
        crowns,
        active,
        excluded_tree_cells,
        side / 2.0,
        cell_size,
    )
    added_isolated_trees = add_missing_isolated_trees(
        settings,
        target,
        crowns,
        preserved_trees,
        occupied_tree_cells,
        raw_samples,
        active,
        excluded_tree_cells,
        xmin,
        ymin,
        side,
        altitude_origin,
        exaggeration,
    )
    if settings.get("keep_isolated_trees", False):
        log_event(
            "Forêt LiDAR : "
            f"{sum(obj.name.startswith('Houppier') for obj in preserved_trees):,} "
            f"arbres isolés conservés, dont {added_isolated_trees:,} complétés depuis le LiDAR"
        )
    if not top_faces:
        for obj in list(target.all_objects):
            if obj not in preserved_trees:
                bpy.data.objects.remove(obj, do_unlink=True)
        target["interpretation"] = (
            "Aucun massif forestier dense détecté dans la classe LiDAR 5."
        )
        target["cellules_vegetation_lidar"] = len(raw_samples)
        target["arbres_isoles_conserves"] = sum(
            obj.name.startswith("Houppier") for obj in preserved_trees
        )
        print("GEO_PLATFORM_CANOPY_OK 0 massif forestier dense")
        return True
    material = forest_procedural_material(settings)
    mesh = bpy.data.meshes.new("Canopée LiDAR continue")
    mesh.from_pydata(vertices, [], top_faces + bottom_faces + side_faces)
    mesh.materials.append(material)
    bevel_weights = mesh.attributes.new(
        name="bevel_weight_edge", type="FLOAT", domain="EDGE"
    )
    for edge in mesh.edges:
        if tuple(sorted(edge.vertices)) in vertical_boundary_edges:
            bevel_weights.data[edge.index].value = 1.0
    for polygon in mesh.polygons[: len(top_faces)]:
        polygon.use_smooth = True
    canopy = bpy.data.objects.new("Forêt continue LiDAR", mesh)
    target.objects.link(canopy)
    if settings.get("forest_material_mode") == "texture":
        try:
            if not project_orthophoto_material(
                canopy, settings, range(len(top_faces))
            ):
                canopy["avertissement_materiau"] = (
                    "Orthophoto absente : rampe procédurale utilisée en remplacement."
                )
        except Exception as error:
            canopy["avertissement_materiau"] = (
                "Projection orthophoto impossible, rampe procédurale conservée : " + str(error)
            )
    side_start = len(top_faces) + len(bottom_faces)
    side_material_index = None
    if settings.get("forest_material_mode") == "texture":
        side_material_index = color_forest_sides(
            canopy, range(side_start, side_start + len(side_faces))
        )
    if smoothing > 0.0:
        bevel = canopy.modifiers.new("Lisières arrondies", "BEVEL")
        bevel.width = cell_size * (0.05 + smoothing * 0.08)
        bevel.segments = max(2, round(2 + smoothing * 3))
        bevel.affect = "EDGES"
        bevel.limit_method = "WEIGHT"
        bevel.miter_outer = "MITER_ARC"
        if side_material_index is not None:
            bevel.material = side_material_index
    for obj in list(target.all_objects):
        if obj != canopy and obj not in preserved_trees:
            bpy.data.objects.remove(obj, do_unlink=True)
    target["interpretation"] = (
        "Massifs denses reconstruits depuis la classe LiDAR 5 ; "
        "arbres isolés, végétation basse et bâtiments exclus."
    )
    target["echantillons_lidar"] = len(raw_samples)
    target["cellules_forestieres"] = len(active)
    target["cellules_batiments_retirees"] = buildings_removed
    target["cellules_isolees_retirees"] = isolated_removed
    target["maille_canopée_m"] = cell_size
    target["volume_joint_au_sol"] = True
    target["distance_batiments_m"] = building_clearance
    target["arbres_isoles_conserves"] = sum(
        obj.name.startswith("Houppier") for obj in preserved_trees
    )
    print(
        "GEO_PLATFORM_CANOPY_OK",
        len(raw_samples),
        "cellules végétales classées,",
        len(active),
        "cellules forestières denses,",
        len(top_faces),
        "faces supérieures,",
        len(side_faces),
        "faces de lisière jusqu'au sol, maille",
        round(cell_size, 3),
        "m",
    )
    return True


def apply_tree_shape(settings):
    """Applique une canopée continue ou le prototype d'arbre choisi."""
    target = bpy.data.collections.get("vegetation")
    if target is None:
        return
    crowns = [
        obj
        for obj in target.all_objects
        if obj.type == "MESH" and obj.name.startswith("Houppier")
    ]
    if settings["tree_shape"] == "canopy":
        if not apply_forest_canopy(settings, target, crowns):
            raise RuntimeError("la canopée LiDAR demandée n'a produit aucune surface continue")
        return
    if not crowns:
        return
    if settings["tree_shape"] != "conifer":
        return
    material = (
        crowns[0].data.materials[0]
        if crowns[0].data.materials
        else get_or_create_material(
            "Feuillage naturel",
            (0.12, 0.28, 0.065, 1.0),
            settings.get("procedural_materials", True),
        )
    )
    conifer = create_conifer_mesh(material)
    for crown in crowns:
        crown.data = conifer


def main():
    """Lit la configuration créée par Rust, met la scène à jour et l'enregistre sur place."""
    total_started_at = perf_counter()
    arguments = sys.argv[sys.argv.index("--") + 1 :]
    settings = json.loads(Path(arguments[0]).read_text(encoding="utf-8"))
    log_event("Démarrage de la finition de scène")
    log_scene_stats("avant finition")
    phase_started_at = perf_counter()
    optimize_orthophoto(settings)
    log_event("Optimisation de l'orthophoto terminée", phase_started_at)
    phase_started_at = perf_counter()
    drape_roads(settings)
    log_event("Plaquage des routes sur le terrain terminé", phase_started_at)
    phase_started_at = perf_counter()
    apply_building_colors(settings)
    log_event("Matériaux des bâtiments terminés", phase_started_at)
    phase_started_at = perf_counter()
    apply_tree_shape(settings)
    log_event("Végétation terminée", phase_started_at)
    phase_started_at = perf_counter()
    bpy.ops.wm.save_as_mainfile(filepath=settings["output"], compress=True)
    log_event("Fichier Blender enregistré", phase_started_at)
    log_scene_stats("après finition")
    log_event("Finition complète", total_started_at)
    print("GEO_PLATFORM_POSTPROCESS_OK", settings["output"])


if __name__ == "__main__":
    main()
