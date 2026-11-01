"""Contexte : cadrage au-dessus du terrain, soleil et ciel sans ressources externes.
Usage : les angles et réglages de rendu viennent tous du TOML d'export.
"""

import math
import bpy
from mathutils import Vector
from scene_geometry import collection


def setup(plan):
    config = plan["config"]
    scene = bpy.context.scene
    scene.unit_settings.system = "METRIC"
    scene.unit_settings.scale_length = 1
    scene.render.engine = config["render"]["engine"]
    scene.render.resolution_x = config["render"]["resolution_x"]
    scene.render.resolution_y = config["render"]["resolution_y"]
    scene.render.resolution_percentage = 100
    scene.render.film_transparent = config["render"]["transparent_background"]
    if scene.render.engine == "CYCLES":
        scene.cycles.samples = config["render"]["samples"]
        scene.cycles.device = "CPU"
    elif hasattr(scene,"eevee"):
        if hasattr(scene.eevee,"taa_render_samples"):
            scene.eevee.taa_render_samples = config["render"]["samples"]
    target = collection("Caméra et lumière")
    settings = config["camera"]
    side = plan["terrain"]["side"]
    heights = plan["terrain"]["heights"]
    origin_z = plan["terrain"]["origin"][2]
    exaggeration = plan["terrain"]["exaggeration"]
    center = Vector((0,0,((min(heights)+max(heights))/2-origin_z)*exaggeration))
    azimuth,elevation = math.radians(settings["azimuth_deg"]),math.radians(settings["elevation_deg"])
    direction = Vector((math.sin(azimuth)*math.cos(elevation),math.cos(azimuth)*math.cos(elevation),math.sin(elevation)))
    data = bpy.data.cameras.new("Vue du terrain")
    camera = bpy.data.objects.new("Caméra du terrain",data)
    target.objects.link(camera)
    scene.camera = camera
    data.lens = settings["focal_length_mm"]
    data.clip_end = settings["clip_end_m"]
    aspect = scene.render.resolution_x/scene.render.resolution_y
    # Une boîte englobante évite qu'un coin sorte du cadre en vue oblique.
    camera.location = center+direction*side*2
    camera.rotation_euler = (center-camera.location).to_track_quat("-Z","Y").to_euler()
    rotation = camera.rotation_euler.to_matrix().transposed()
    corners = [rotation @ (Vector((x,y,z))-center)
               for x in [-side/2,side/2] for y in [-side/2,side/2]
               for z in [(min(heights)-origin_z)*exaggeration,(max(heights)-origin_z)*exaggeration]]
    width = max(p.x for p in corners)-min(p.x for p in corners)
    height = max(p.y for p in corners)-min(p.y for p in corners)
    if settings["projection"] == "orthographic":
        data.type = "ORTHO"
        data.ortho_scale = max(width,height*aspect)*settings["framing_margin"]
    else:
        distance = max(width/(2*math.tan(data.angle_x/2)),height/(2*math.tan(data.angle_y/2)))
        depth = max(abs(p.z) for p in corners)
        camera.location = center+direction*(distance*settings["framing_margin"]+depth)
    light = config["lighting"]
    sun_data = bpy.data.lights.new("Soleil naturel","SUN")
    sun_data.energy = light["sun_energy"]
    sun_data.angle = math.radians(light["sun_angle_deg"])
    sun = bpy.data.objects.new("Soleil naturel",sun_data)
    target.objects.link(sun)
    sun.rotation_euler = (math.radians(90-light["sun_elevation_deg"]),0,math.radians(light["sun_azimuth_deg"]))
    world = bpy.data.worlds.new("Ciel naturel")
    scene.world = world
    world.use_nodes = True
    background = world.node_tree.nodes.get("Background")
    background.inputs["Color"].default_value = light["sky_color"]
    background.inputs["Strength"].default_value = light["sky_strength"]
    # La scène s'ouvre directement sur la caméra, également en aperçu matière.
    for screen in bpy.data.screens:
        for area in screen.areas:
            if area.type == "VIEW_3D":
                area.spaces.active.region_3d.view_perspective = "CAMERA"
                area.spaces.active.shading.type = "MATERIAL"
    scene["crs"] = "EPSG:2154"
    scene["origine_lambert93"] = plan["terrain"]["origin"]
    scene["exageration_verticale"] = exaggeration
