"""Contexte : savoir ce qu'un fichier Blender a réellement conservé du LiDAR.
Usage : blender --background scene.blend --python tests/blender_lidar_report.py
Lecture seule : affiche le rapport et les volumes présents, sans réenregistrer la scène.
"""

import json
import bpy

config_text = bpy.data.texts.get("Configuration interne.json")
report_text = bpy.data.texts.get("Compte rendu export.json")
config = json.loads(config_text.as_string()) if config_text else {}
report = json.loads(report_text.as_string()) if report_text else {}
vegetation = bpy.data.collections.get("vegetation")
points = bpy.data.collections.get("lidar_points")
print("LIDAR_REPORT", json.dumps({
    "file": bpy.data.filepath,
    "area": config.get("area"),
    "vegetation_config": config.get("vegetation"),
    "layers": {key: config.get("layers", {}).get(key) for key in ["vegetation", "lidar_points"]},
    "lidar_files": report.get("fichiers_lidar"),
    "lidar_summary": report.get("lidar"),
    "warnings": report.get("avertissements"),
    "vegetation_objects": len(vegetation.objects) if vegetation else 0,
    "vegetation_hidden": vegetation.hide_viewport if vegetation else None,
    "point_objects": len(points.objects) if points else 0,
}, ensure_ascii=False))
