//! Contexte : les surfaces d'eau doivent rester lisibles au-dessus de l'orthophoto.
//! Usage : clearance sépare les surfaces ; thickness donne un petit volume vers le bas.
//! Ces valeurs servent à la représentation, pas à déduire la profondeur réelle d'un lac.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WaterConfig {
    pub surface_clearance_m: f64,
    pub thickness_m: f64,
    pub max_vertices: usize,
}
