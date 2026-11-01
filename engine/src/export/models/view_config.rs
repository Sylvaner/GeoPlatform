//! Contexte : la vue de travail Blender possède ses propres réglages, distincts de la caméra.
//! Usage : ces valeurs sont enregistrées dans le .blend et appliquées à son ouverture.

use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ViewConfig {
    pub clip_end_m: f64,
    pub collapse_outliner: bool,
    pub hide_terrain_with_orthophoto: bool,
    pub outliner_timeout_seconds: u64,
}
