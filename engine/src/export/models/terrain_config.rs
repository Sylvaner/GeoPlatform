//! Contexte : le terrain est un maillage régulier, issu des BIL ou des points sol LiDAR.
//! Usage : auto essaie d'abord ALTI ; lidar utilise la classe sol des fichiers LAZ disponibles.

use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainConfig {
    pub source: String,
    pub grid_step_m: f64,
    pub max_vertices: usize,
    pub vertical_exaggeration: f64,
    pub altitude_origin: String,
    pub valid_altitude_min_m: f64,
    pub valid_altitude_max_m: f64,
    pub lidar_ground_class: u8,
    pub lidar_fill_radius_cells: usize,
}
