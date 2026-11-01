"""Contexte : la finalisation graphique doit rester bornée et isolée de Blender personnel.
Usage : python -m unittest discover -s tests -p 'test_*.py'.
Un faux processus contrôle les fichiers temporaires, les erreurs et le mode désactivé.
"""

import importlib.util
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location(
    "scene_outliner", Path(__file__).resolve().parents[1] / "blender/scene_outliner.py")
outliner = importlib.util.module_from_spec(spec)
with patch.dict(sys.modules, {"bpy": SimpleNamespace()}):
    spec.loader.exec_module(outliner)


class OutlinerFinalization(unittest.TestCase):
    def test_disabled_collapse_does_not_start_a_graphical_process(self):
        with patch.object(outliner.subprocess, "run") as run:
            outliner.finalize_outliner(Path("unused.blend"), {"view": {"collapse_outliner": False}})
        run.assert_not_called()

    def launch(self, folder, response):
        fake = SimpleNamespace(app=SimpleNamespace(binary_path="blender"),
                               ops=SimpleNamespace(wm=SimpleNamespace(read_factory_settings=Mock())))
        config = {"view": {"collapse_outliner": True, "outliner_timeout_seconds": 47}}

        def respond(command, **kwargs):
            if response is not None:
                Path(command[-1]).write_text(json.dumps(response), encoding="utf-8")
        with patch.object(outliner, "__file__", str(folder / "blender/scene_outliner.py")), \
                patch.object(outliner, "bpy", fake), \
                patch.object(outliner.subprocess, "run", side_effect=respond) as run:
            outliner.finalize_outliner(folder / "scene.part.blend", config)
        fake.ops.wm.read_factory_settings.assert_called_once_with(use_empty=True)
        return run.call_args

    def test_graphical_process_uses_its_own_recovery_and_preferences(self):
        with tempfile.TemporaryDirectory() as temporary:
            folder = Path(temporary)
            call = self.launch(folder, {"success": True})
            self.assertEqual(call.kwargs["timeout"], 47)
            environment = call.kwargs["env"]
            self.assertEqual(Path(environment["TEMP"]), folder / "interface-temp")
            self.assertEqual(Path(environment["BLENDER_USER_CONFIG"]), folder / "interface-temp/config")
            self.assertIn("--disable-autoexec", call.args[0])
            self.assertNotIn("--background", call.args[0])

    def test_missing_receipt_or_failed_finalization_is_an_error(self):
        for result in [None, {"success": False, "error": "dessin impossible"}]:
            with tempfile.TemporaryDirectory() as temporary:
                with self.assertRaises(RuntimeError):
                    self.launch(Path(temporary), result)


if __name__ == "__main__":
    unittest.main()
