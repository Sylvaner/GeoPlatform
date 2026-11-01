//! Contexte : le PCI Vecteur décrit les parcelles cadastrales françaises.
//! Usage : `cargo run -- CADASTRE --center latitude longitude --side mètres`.
//! La Géoplateforme diffuse la couche sous le nom historique PARCELLAIRE_EXPRESS.
//! Le nom complet inclut CADASTRALPARCELS. : omettre ce préfixe provoque une erreur WFS.
//! Les pages GeoJSON conservent les géométries et tous les attributs d'origine :
//! idu, commune, section, numéro, contenance… Ce flux ne contient pas les propriétaires.

use crate::{cli::DownloadOptions, models::WfsLayer, vector, wfs};
use anyhow::Result;

pub const LAYERS: &[WfsLayer] = &[WfsLayer {
    identifier: "CADASTRALPARCELS.PARCELLAIRE_EXPRESS:parcelle",
    label: "Parcelles cadastrales (PCI Vecteur)",
    // idu est la référence cadastrale complète : commune, préfixe, section, numéro.
    // Le tri garde les zéros initiaux ; ce n'est pas une conversion en entier.
    sort_by: "idu A",
    output_format: "application/json",
}];

pub async fn run(options: DownloadOptions) -> Result<()> {
    vector::run(options, "CADASTRE", wfs::URL, LAYERS).await
}
