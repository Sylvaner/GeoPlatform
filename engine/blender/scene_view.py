"""Contexte : la vue 3D et l'arborescence sont enregistrées dans les espaces de travail.
Usage : régler toutes les vues, conserver le terrain masqué et préparer une liste compacte.
Le mode VIEW_LAYER reste celui demandé. Le repli est finalisé séparément après
un premier dessin de l'arborescence, qui n'existe pas encore en arrière-plan.
"""

import bpy


def terrain_visible(config, orthophoto_present):
    """Masquer le terrain de secours tout en gardant sa collection activable."""
    return (config["layers"]["terrain"]["visible"] and not
            (config["view"]["hide_terrain_with_orthophoto"] and
             config["layers"]["orthophoto"]["enabled"] and orthophoto_present))


def setup_view(plan):
    settings = plan["config"]["view"]
    for screen in bpy.data.screens:
        for area in screen.areas:
            # Un espace 3D peut aussi être conservé derrière un autre éditeur.
            for space in area.spaces:
                if space.type == "VIEW_3D":
                    space.clip_end = settings["clip_end_m"]
                elif space.type == "OUTLINER":
                    space.display_mode = "VIEW_LAYER"
                    space.use_sync_select = False
