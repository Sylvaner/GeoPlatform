"""Contexte : les réglages d'ouverture doivent garder le terrain réactivable.
Usage : python -m unittest discover -s tests -p test_*.py
Un faux contexte suffit pour ces règles ; blender_verify.py contrôle ensuite le vrai fichier.
"""

import contextlib
import importlib.util
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location(
    "scene_view", Path(__file__).resolve().parents[1] / "blender/scene_view.py")
view = importlib.util.module_from_spec(spec)
with patch.dict(sys.modules, {"bpy": SimpleNamespace()}):
    spec.loader.exec_module(view)


def config():
    return {"layers": {"terrain": {"visible": True},
                       "orthophoto": {"enabled": True}},
            "view": {"hide_terrain_with_orthophoto": True, "clip_end_m": 6000,
                     "collapse_outliner": True}}


class SceneOpening(unittest.TestCase):
    def test_terrain_is_hidden_when_orthophoto_is_present(self):
        self.assertFalse(view.terrain_visible(config(), True))

    def test_terrain_stays_visible_when_orthophoto_is_absent_or_disabled(self):
        c = config()
        self.assertTrue(view.terrain_visible(c, False))
        c["layers"]["orthophoto"]["enabled"] = False
        self.assertTrue(view.terrain_visible(c, True))

    def test_user_can_keep_terrain_visible_or_hide_it_explicitly(self):
        c = config()
        c["view"]["hide_terrain_with_orthophoto"] = False
        self.assertTrue(view.terrain_visible(c, True))
        c["layers"]["terrain"]["visible"] = False
        self.assertFalse(view.terrain_visible(c, False))

    def test_clip_distance_applies_to_all_saved_views_and_tree_is_compact(self):
        first = SimpleNamespace(type="VIEW_3D", clip_end=1000)
        second = SimpleNamespace(type="VIEW_3D", clip_end=1000)
        outliner = SimpleNamespace(type="OUTLINER", display_mode="VIEW_LAYER", use_sync_select=True)
        areas = [SimpleNamespace(type="VIEW_3D", spaces=[first, second]),
                 SimpleNamespace(type="OUTLINER", spaces=[outliner], regions=[SimpleNamespace(type="WINDOW")])]
        class Collapse:
            def poll(self):
                return True

            def __call__(self, **kwargs):
                return {"FINISHED"}
        screen = SimpleNamespace(areas=areas)
        fake = SimpleNamespace(data=SimpleNamespace(screens=[screen]),
                               context=SimpleNamespace(temp_override=lambda **kwargs: contextlib.nullcontext(),
                                                       window_manager=SimpleNamespace(windows=[SimpleNamespace(screen=screen)])),
                               ops=SimpleNamespace(outliner=SimpleNamespace(show_one_level=Collapse())))
        with patch.object(view, "bpy", fake):
            view.setup_view({"config": config()})
        self.assertEqual(first.clip_end, 6000)
        self.assertEqual(second.clip_end, 6000)
        self.assertEqual(outliner.display_mode, "VIEW_LAYER")
        self.assertFalse(outliner.use_sync_select)


if __name__ == "__main__":
    unittest.main()
