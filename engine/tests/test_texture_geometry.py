"""Contexte : les découpes de maillage ne doivent créer ni trou ni UV mélangés.
Usage : python -m unittest discover -s tests -p test_texture_geometry.py
Ces tests utilisent des coordonnées synthétiques et fonctionnent sans Blender ni cache.
"""

import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "scene_texture_geometry", Path(__file__).resolve().parents[1] / "blender/scene_texture_geometry.py")
geometry = importlib.util.module_from_spec(spec)
spec.loader.exec_module(geometry)


def area(polygon):
    origin = polygon[0][1]
    return abs(sum((a[1][0]-origin[0])*(b[1][1]-origin[1]) -
                   (b[1][0]-origin[0])*(a[1][1]-origin[1])
                   for a, b in zip(polygon, polygon[1:]+polygon[:1]))) / 2


class NativeTileClipping(unittest.TestCase):
    def test_four_native_tiles_preserve_complete_surface(self):
        polygon = [((u, v, u+v), (u, v)) for u, v in [(0.2, 0.2), (1.8, 0.2), (1.8, 1.8), (0.2, 1.8)]]
        pieces = list(geometry.tile_parts(polygon))
        self.assertEqual({key for key, _ in pieces}, {(0, 0), (1, 0), (0, 1), (1, 1)})
        self.assertAlmostEqual(sum(area(p) for _, p in pieces), area(polygon))
        for (col, row), piece in pieces:
            for point, uv in piece:
                self.assertTrue(col <= uv[0] <= col+1 and row <= uv[1] <= row+1)
                self.assertAlmostEqual(point[2], uv[0]+uv[1])

    def test_large_wmts_indices_do_not_create_sliver_faces(self):
        polygon = [((u, v, 0), (u, v)) for u, v in
                   [(269045, 181678), (269046, 181678), (269046, 181679), (269045, 181679)]]
        pieces = list(geometry.tile_parts(polygon))
        self.assertEqual(len(pieces), 1)
        self.assertEqual(pieces[0][0], (269045, 181678))
        self.assertAlmostEqual(area(pieces[0][1]), 1)

    def test_triangle_crossing_a_tile_border_conserves_area(self):
        polygon = [((u, v, 0), (u, v)) for u, v in [(0.2, 0.2), (1.7, 0.4), (0.7, 1.8)]]
        pieces = list(geometry.tile_parts(polygon))
        self.assertAlmostEqual(sum(area(p) for _, p in pieces), area(polygon))


if __name__ == "__main__":
    unittest.main()
