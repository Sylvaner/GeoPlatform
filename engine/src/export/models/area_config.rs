//! Contexte : la scène correspond à un carré exprimé en mètres Lambert-93.
//! Usage : center respecte l'ordre indiqué par crs, latitude/longitude par défaut.

use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AreaConfig {
    pub center: [f64; 2],
    pub crs: String,
    pub side_m: f64,
}
