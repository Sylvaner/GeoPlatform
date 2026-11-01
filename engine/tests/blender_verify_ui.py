"""Contexte : vérifier l'interface réellement restaurée, sans replier l'arbre pendant le contrôle.
Usage : blender scene.blend --python tests/blender_verify_ui.py -- --screenshot image.png
Cette instance lit la scène, prend une capture après dessin, puis quitte sans l'enregistrer.
"""

import argparse
from pathlib import Path
import sys
import traceback
import bpy


def verify():
    parser = argparse.ArgumentParser()
    parser.add_argument("--screenshot", required=True)
    args = parser.parse_args(sys.argv[sys.argv.index("--")+1:])
    try:
        window = bpy.context.window_manager.windows[0]
        for area in window.screen.areas:
            if area.type == "OUTLINER":
                assert area.spaces.active.display_mode == "VIEW_LAYER"
                region = next(r for r in area.regions if r.type == "WINDOW")
                with bpy.context.temp_override(window=window, area=area, region=region):
                    area.tag_redraw()
                    bpy.ops.wm.redraw_timer(type="DRAW", iterations=1)
        bpy.ops.wm.redraw_timer(type="DRAW_WIN", iterations=1)
        path = Path(args.screenshot).resolve()
        path.parent.mkdir(parents=True, exist_ok=True)
        bpy.ops.screen.screenshot(filepath=str(path))
        print("VERIFICATION_INTERFACE_OK", path, flush=True)
    except Exception:
        print(traceback.format_exc(), flush=True)
    bpy.ops.wm.quit_blender()


if __name__ == "__main__":
    bpy.app.timers.register(verify, first_interval=0.5)
