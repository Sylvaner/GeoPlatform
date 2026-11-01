//! Contexte : les fichiers BIL bruts ne décrivent pas leur emplacement géographique.
//! Usage : `cache.rs` construit cette structure et la sérialise dans `matrix.json`.
//! Les données sont seulement empruntées le temps de produire le JSON.

use serde::Serialize;

use super::TileMatrix;

/// Informations nécessaires pour interpréter les tuiles d'un niveau du cache.
// `'a` est une durée de vie, pas une donnée : elle indique que les références
// ci-dessous ne peuvent pas être utilisées plus longtemps que les valeurs lues.
// `&'a str` emprunte du texte existant ; il n'en crée pas une copie.
#[derive(Serialize)]
pub struct CacheMetadata<'a> {
    /// Version du format : 2 distingue ces métadonnées de l'ancien cache Lambert seul.
    pub schema_version: u32,
    /// Identifiant de la couche IGN.
    pub layer: &'a str,
    /// Identifiant du jeu de grilles WMTS.
    pub tile_matrix_set: &'a str,
    /// Système des tuiles enregistrées, qui peut différer du Lambert-93 de la demande.
    pub crs: &'a str,
    /// Ordre des coordonnées d'origine : longitude_latitude ou x_y.
    pub coordinate_order: &'a str,
    /// Format des fichiers téléchargés.
    pub format: &'a str,
    /// Style demandé au serveur.
    pub style: &'a str,
    /// Grille permettant de convertir les indices ligne/colonne en coordonnées.
    pub matrix: &'a TileMatrix,
}
