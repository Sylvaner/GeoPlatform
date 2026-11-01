//! Contexte : le LiDAR peut servir au terrain, à la végétation et à un aperçu des points.
//! Usage : seuls les fichiers complets du cache sont lus ; les .part sont ignorés.

use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LidarConfig {
    pub max_display_points: usize,
    pub display_point_size_m: f64,
    pub display_stride: usize,
}
