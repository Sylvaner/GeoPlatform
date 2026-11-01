"""Contexte : cette instance Blender ne calcule aucune donnée ; elle prépare l'interface sauvegardée.
Usage : lancé automatiquement par scene_outliner.py, puis fermé dès la sauvegarde terminée.
Le dessin explicite de chaque Outliner est nécessaire avant show_one_level(open=False).
"""

import json
from pathlib import Path
import sys
import traceback
import bpy


def collapse_workspace(window, workspace):
    window.workspace = workspace
    screen = window.screen
    count = 0
    for area in screen.areas:
        if area.type != "OUTLINER":
            continue
        area.spaces.active.display_mode = "VIEW_LAYER"
        area.spaces.active.use_sync_select = False
        region = next(r for r in area.regions if r.type == "WINDOW")
        with bpy.context.temp_override(window=window, screen=screen, area=area, region=region):
            # DRAW vise la région : DRAW_WIN ne suffit pas dans une fenêtre masquée.
            area.tag_redraw()
            bpy.ops.wm.redraw_timer(type="DRAW", iterations=1)
            # Notre hiérarchie collection / objet / données / matériaux reste peu profonde.
            for _ in range(8):
                bpy.ops.outliner.show_one_level(open=False)
            count += 1
    return count


def finish():
    receipt = Path(sys.argv[sys.argv.index("--")+1])
    try:
        config = json.loads(bpy.data.texts["Configuration interne.json"].as_string())
        bpy.context.preferences.filepaths.save_version = 0
        window = bpy.context.window_manager.windows[0]
        initial = window.workspace
        count = sum(collapse_workspace(window, workspace) for workspace in bpy.data.workspaces)
        window.workspace = initial
        if not count:
            raise RuntimeError("aucun Outliner disponible pour préparer l'arborescence")
        bpy.ops.wm.save_as_mainfile(filepath=bpy.data.filepath,
                                   compress=config["blender"]["compress_file"])
        result = {"success": True, "outliners": count, "mode": "VIEW_LAYER"}
        print("EXPORT_OUTLINER_OK", count, flush=True)
    except Exception:
        result = {"success": False, "error": traceback.format_exc()}
    receipt.write_text(json.dumps(result, ensure_ascii=False), encoding="utf-8")
    bpy.ops.wm.quit_blender()


if __name__ == "__main__":
    # Le script de démarrage précède la création des fenêtres. Le timer intervient
    # une fois la boucle d'interface démarrée, sans demander de clic à l'utilisateur.
    bpy.app.timers.register(finish, first_interval=0.5)
