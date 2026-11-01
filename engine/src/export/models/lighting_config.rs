//! Contexte : un soleil et un ciel procédural donnent un éclairage naturel sans fichier HDRI.
//! Usage : les angles sont des choix de présentation, sans simulation astronomique.

use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LightingConfig {
    pub sun_azimuth_deg: f64,
    pub sun_elevation_deg: f64,
    pub sun_energy: f64,
    pub sun_angle_deg: f64,
    pub sky_strength: f64,
    pub sky_color: [f64; 4],
}
