//! Contexte : une TileMatrix décrit la grille des tuiles à un niveau de résolution.
//! Usage : remplie par `wmts/capabilities.rs`, lue par `geometry.rs`, puis enregistrée
//! dans `matrix.json` par `cache.rs`. Elle permet de géoréférencer les tuiles.
//! Pour une tuile (row, col), son bord gauche vaut top_left_x + col × tile_width
//! × resolution_units_per_px ; son bord haut vaut top_left_y - row × tile_height
//! × resolution_units_per_px. La ligne augmente vers le sud, la colonne vers l'est.
//! Ces calculs restent dans le CRS indiqué ici, pas forcément en Lambert-93.

use serde::{Deserialize, Serialize};

/// Description d'un niveau de la grille WMTS.
// `Serialize` permet à serde_json de convertir les champs de la structure en JSON.
#[derive(Debug, Serialize, Deserialize)]
pub struct TileMatrix {
    /// Jeu de grilles annoncé pour cette couche, par exemple WGS84G_6_14.
    pub matrix_set: String,
    /// CRS utilisé par les calculs : EPSG:4326, EPSG:2154 ou EPSG:3857.
    /// En interne, X/Y signifie longitude/latitude pour EPSG:4326.
    pub crs: String,
    /// Nom du niveau annoncé par le serveur, à réutiliser dans les requêtes GetTile.
    pub identifier: String,
    /// Dénominateur d'échelle WMTS, utilisé pour calculer la résolution.
    pub scale_denominator: f64,
    /// Pas d'un pixel dans l'unité du CRS : degrés en WGS84, mètres en Lambert-93.
    pub resolution_units_per_px: f64,
    /// Résolution nominale issue de l'échelle WMTS, en mètres par pixel.
    /// Pour une grille en degrés, ce chiffre est conventionnel : la largeur réelle
    /// en mètres d'un pixel varie avec la latitude. Ce n'est pas une garantie de 1 m.
    pub nominal_resolution_m_per_px: f64,
    /// Coordonnée X du coin supérieur gauche, dans l'unité du CRS.
    pub top_left_x: f64,
    /// Coordonnée Y du coin supérieur gauche, dans l'unité du CRS.
    pub top_left_y: f64,
    /// Largeur d'une tuile, en pixels.
    pub tile_width: u64,
    /// Hauteur d'une tuile, en pixels.
    pub tile_height: u64,
    /// Nombre total de colonnes de tuiles dans cette grille.
    pub matrix_width: u64,
    /// Nombre total de lignes de tuiles dans cette grille.
    pub matrix_height: u64,
    /// Indices autorisés pour cette couche et ce niveau, bornes incluses.
    /// Une couche peut n'occuper qu'une partie de la grille mondiale.
    pub limits: super::TileRange,
}
