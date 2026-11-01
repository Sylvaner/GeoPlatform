//! Contexte : l'exporteur lit le schéma 2 du cache WMTS, sans relire le serveur.
//! Usage : les champs nécessaires à la géoréférence sont possédés, pas empruntés.

use serde::Deserialize;
#[derive(Debug, Deserialize)]
pub struct RasterMetadata {
    pub schema_version: u32,
    pub crs: String,
    pub format: String,
    pub coordinate_order: String,
    pub matrix: crate::models::TileMatrix,
}
