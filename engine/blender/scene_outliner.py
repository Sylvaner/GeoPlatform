"""Contexte : Blender crée l'arborescence View Layer seulement lors de son premier dessin.
Usage : après l'export, une instance dédiée construit puis replie les collections et sauvegarde.
Sous Windows sa fenêtre est masquée ; aucun script automatique n'est ajouté au .blend.
"""

import json
import os
from pathlib import Path
import subprocess
import sys
import bpy


def finalize_outliner(output, config):
    if not config["view"]["collapse_outliner"]:
        return
    script = Path(__file__).with_name("finalize_outliner.py")
    receipt = script.parent.parent / "outliner-result.json"
    temporary = script.parent.parent / "interface-temp"
    preferences = temporary / "config"
    preferences.mkdir(parents=True, exist_ok=True)
    # Blender écrit un quit.blend à sa fermeture graphique. Isoler cette session
    # préserve la récupération et les préférences d'une instance ouverte par l'utilisateur.
    environment = dict(os.environ)
    environment.update(TEMP=str(temporary), TMP=str(temporary), TMPDIR=str(temporary),
                       BLENDER_USER_CONFIG=str(preferences))
    command = [bpy.app.binary_path, "--factory-startup", "--disable-autoexec",
               "--no-window-focus", "--python-exit-code", "1", str(output),
               "--python", str(script), "--", str(receipt)]
    startup = None
    if sys.platform == "win32":
        startup = subprocess.STARTUPINFO()
        startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
        startup.wShowWindow = subprocess.SW_HIDE
    # Libérer la scène de calcul avant de la rouvrir : les milliers de textures
    # intégrées ne doivent pas rester deux fois en mémoire pendant la finalisation.
    bpy.ops.wm.read_factory_settings(use_empty=True)
    print("[EXPORT] Préparation des collections repliées en mode View Layer", flush=True)
    subprocess.run(command, check=True, startupinfo=startup, env=environment,
                   timeout=config["view"]["outliner_timeout_seconds"])
    if not receipt.is_file():
        raise RuntimeError("Blender n'a pas confirmé le repli de l'arborescence")
    result = json.loads(receipt.read_text(encoding="utf-8"))
    if not result.get("success"):
        raise RuntimeError("Repli de l'arborescence : " + result.get("error", "échec inconnu"))
