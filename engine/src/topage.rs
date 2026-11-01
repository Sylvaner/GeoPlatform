//! Contexte : BD TOPAGE est le référentiel hydrographique diffusé par le Sandre.
//! Usage : `cargo run -- BD_TOPAGE --center latitude longitude --side mètres`.
//! Cette commande utilise explicitement le millésime 2026, vérifié dans le catalogue
//! officiel, et ses huit couches de métropole (FXX). Une nouvelle année doit être
//! vérifiée avant de changer les noms, l'URL et le dossier d'édition ci-dessous.
//! On conserve les objets entiers et leurs codes métier : aucun calcul de crue,
//! de débit, de qualité de l'eau ou de réseau d'assainissement n'est réalisé ici.

use crate::{cli::DownloadOptions, models::WfsLayer, vector};
use anyhow::Result;

pub const URL: &str = "https://services.sandre.eaufrance.fr/geo/topage2026";
// Le catalogue WFS du Sandre annonce ce libellé exact, différent de celui de l'IGN.
const GEOJSON: &str = "application/json; subtype=geojson";

pub const LAYERS: &[WfsLayer] = &[
    WfsLayer {
        identifier: "sa:CoursEau_FXX_Topage2026",
        label: "Cours d'eau",
        sort_by: "gid A",
        output_format: GEOJSON,
    },
    WfsLayer {
        identifier: "sa:TronconHydrographique_FXX_Topage2026",
        label: "Tronçons hydrographiques",
        sort_by: "gid A",
        output_format: GEOJSON,
    },
    WfsLayer {
        identifier: "sa:PlanEau_FXX_Topage2026",
        label: "Plans d'eau",
        sort_by: "gid A",
        output_format: GEOJSON,
    },
    WfsLayer {
        identifier: "sa:SurfaceElementaire_FXX_Topage2026",
        label: "Surfaces élémentaires",
        sort_by: "gid A",
        output_format: GEOJSON,
    },
    WfsLayer {
        identifier: "sa:NoeudHydrographique_FXX_Topage2026",
        label: "Nœuds hydrographiques",
        sort_by: "gid A",
        output_format: GEOJSON,
    },
    WfsLayer {
        identifier: "sa:BassinVersantTopographique_FXX_Topage2026",
        label: "Bassins versants topographiques",
        sort_by: "gid A",
        output_format: GEOJSON,
    },
    WfsLayer {
        identifier: "sa:BassinHydrographique_FXX_Topage2026",
        label: "Bassins hydrographiques",
        sort_by: "gid A",
        output_format: GEOJSON,
    },
    WfsLayer {
        identifier: "sa:LimiteTerreEau_FXX_Topage2026",
        label: "Limites terre-eau",
        sort_by: "gid A",
        output_format: GEOJSON,
    },
];

pub async fn run(options: DownloadOptions) -> Result<()> {
    // Une édition différente doit cohabiter avec l'ancienne, pas l'écraser.
    vector::run(options, "BD_TOPAGE/2026", URL, LAYERS).await
}
