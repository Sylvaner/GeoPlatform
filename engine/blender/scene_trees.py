"""Contexte : végétation légère dérivée de la canopée LiDAR, et aperçu des points.
Usage : un tronc et un houppier partagés suffisent pour chaque arbre stylisé.
Les volumes représentent des cellules de végétation, pas des essences identifiées.
"""

import math
import random
import bpy
from scene_geometry import collection, markers
from scene_materials import material


def move_to(obj, target):
    for previous in list(obj.users_collection):
        previous.objects.unlink(obj)
    target.objects.link(obj)


def build_trees(plan):
    config = plan["config"]
    style = config["layers"]["vegetation"]
    if not style["enabled"] or not plan["trees"]:
        return
    target = collection("vegetation", style["visible"])
    settings = config["vegetation"]
    target["interpretation"] = "Volumes de canopée par cellule LiDAR ; pas un inventaire d'arbres."
    target["nombre_arbres"] = len(plan["trees"])
    target["fichiers_lidar"] = len(plan["lidar_files"])
    leaves = material("Feuillage naturel",settings["foliage_color"],True,settings["procedural_variation"])
    bark = material("Écorce naturelle",settings["bark_color"],True,settings["procedural_variation"])
    # Les prototypes fournissent un seul maillage partagé par tous les objets.
    bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=settings["canopy_subdivisions"],radius=1)
    crown = bpy.context.object
    crown_mesh = crown.data
    crown_mesh.materials.append(leaves)
    for polygon in crown_mesh.polygons:
        polygon.use_smooth = True
    bpy.data.objects.remove(crown,do_unlink=True)
    bpy.ops.mesh.primitive_cylinder_add(vertices=6,radius=1,depth=1)
    trunk = bpy.context.object
    trunk_mesh = trunk.data
    trunk_mesh.materials.append(bark)
    bpy.data.objects.remove(trunk,do_unlink=True)
    randomizer = random.Random(settings["seed"])
    for index,tree in enumerate(plan["trees"]):
        x,y,z = tree["position"]
        height,radius = tree["height"],tree["radius"]
        trunk = bpy.data.objects.new(f"Tronc {index:05}",trunk_mesh)
        target.objects.link(trunk)
        trunk.location = (x,y,z+height*.25)
        trunk.scale = (height*settings["trunk_radius_ratio"],)*2+(height*.5,)
        crown = bpy.data.objects.new(f"Houppier {index:05}",crown_mesh)
        target.objects.link(crown)
        crown.location = (x,y,z+height*.65)
        crown.scale = (radius,radius*.9,height*.35)
        crown.rotation_euler.z = randomizer.random()*math.tau


def build_points(plan):
    config = plan["config"]
    style = config["layers"]["lidar_points"]
    if not style["enabled"] or not plan["points"]:
        return
    target = collection("lidar_points",style["visible"])
    # Un seul maillage par classe évite des milliers d'objets et de matériaux.
    colors = {2:(.45,.32,.18,1),3:(.40,.52,.16,1),4:(.24,.42,.10,1),
              5:(.12,.30,.06,1),6:(.58,.58,.56,1),9:(.12,.34,.52,1)}
    classes = {}
    for point,classification in plan["points"]:
        classes.setdefault(classification,[]).append(point)
    for classification,points in classes.items():
        mat = material(f"Classe LiDAR {classification}",colors.get(classification,style["color"]))
        markers(f"Points classe {classification}",points,target,mat,
                config["lidar"]["display_point_size_m"],config["lidar"]["display_point_size_m"])
