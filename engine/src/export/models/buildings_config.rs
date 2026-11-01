//! Contexte : un contour BD TOPO peut recevoir un toit mesuré dans les points LiDAR.
//! Usage : auto exige une couverture et une densité suffisantes, sinon garde BD TOPO.
//! Ces seuils sont une règle de sélection explicite, pas une garantie de vérité terrain.

use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BuildingsConfig {
    pub source: String,
    pub texture_roofs: bool,
    pub use_bd_topo_altitudes: bool,
    pub cell_size_m: f64,
    pub min_points: usize,
    pub min_coverage: f64,
    pub min_density_per_m2: f64,
    pub min_height_m: f64,
    pub max_height_m: f64,
    pub max_cell_spread_m: f64,
    pub fill_radius_cells: usize,
    pub max_cells: usize,
    pub max_cells_per_building: usize,
}
