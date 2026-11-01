//! Contexte : la caméra est placée au-dessus de la zone et regarde son centre.
//! Usage : azimuth et elevation définissent une vue oblique ; orthographic évite la perspective.

use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CameraConfig {
    pub projection: String,
    pub azimuth_deg: f64,
    pub elevation_deg: f64,
    pub framing_margin: f64,
    pub focal_length_mm: f64,
    pub clip_end_m: f64,
}
