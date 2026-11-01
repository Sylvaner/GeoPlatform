"""Contexte : une couche peut contenir un objet sans aucune géométrie exploitable.
Usage : python -m unittest discover -s tests -p 'test_*.py'.
Les contrôles portent sur les données utiles, indépendamment du nom de l'objet.
"""

import importlib.util
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location(
    "scene_collections", Path(__file__).resolve().parents[1] / "blender/scene_collections.py")
collections = importlib.util.module_from_spec(spec)
with patch.dict(sys.modules, {"bpy": SimpleNamespace()}):
    spec.loader.exec_module(collections)


class UsefulGeometry(unittest.TestCase):
    def test_empty_mesh_or_curve_is_not_data(self):
        self.assertFalse(collections.has_geometry(SimpleNamespace(type="MESH", data=SimpleNamespace(polygons=[]))))
        self.assertFalse(collections.has_geometry(SimpleNamespace(type="CURVE", data=SimpleNamespace(splines=[]))))
        point = SimpleNamespace(points=[1], bezier_points=[])
        self.assertFalse(collections.has_geometry(SimpleNamespace(type="CURVE", data=SimpleNamespace(splines=[point]))))

    def test_real_faces_lines_and_scene_equipment_are_kept(self):
        self.assertTrue(collections.has_geometry(SimpleNamespace(type="MESH", data=SimpleNamespace(polygons=[1]))))
        line = SimpleNamespace(points=[1, 2], bezier_points=[])
        self.assertTrue(collections.has_geometry(SimpleNamespace(type="CURVE", data=SimpleNamespace(splines=[line]))))
        self.assertTrue(collections.has_geometry(SimpleNamespace(type="CAMERA")))
        self.assertTrue(collections.has_geometry(SimpleNamespace(type="LIGHT")))


if __name__ == "__main__":
    unittest.main()
