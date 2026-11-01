//! Contexte : les objets vectoriels sont découpés à la scène puis posés sur le terrain.
//! Usage : les hauteurs de bâtiments viennent d'abord des champs indiqués, sinon du style.

use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VectorsConfig {
    pub allow_partial_cache: bool,
    pub missing_layers: String,
    pub max_objects_per_layer: usize,
    pub line_sample_step_m: f64,
    pub max_road_vertices: usize,
    pub building_height_fields: Vec<String>,
    pub topage_edition: String,
}
