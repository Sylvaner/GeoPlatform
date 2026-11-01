//! Contexte : les arbres stylisés sont dérivés des points de végétation du LiDAR local.
//! Usage : une cellule fournit une représentation de canopée, pas un arbre identifié.
//! Les troncs et feuillages partagent des maillages simples et des matériaux procéduraux.

use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VegetationConfig {
    pub classes: Vec<u8>,
    pub cell_size_m: f64,
    pub min_height_m: f64,
    pub max_height_m: f64,
    pub max_trees: usize,
    pub crown_radius_ratio: f64,
    pub trunk_radius_ratio: f64,
    pub canopy_subdivisions: u32,
    pub seed: u64,
    pub foliage_color: [f64; 4],
    pub bark_color: [f64; 4],
    pub procedural_variation: f64,
}
