//! Contexte : les JPEG restent dans leur définition et leur projection d'origine.
//! Usage : seuls les UV des maillages sont adaptés ; aucune image n'est redimensionnée.

use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OrthoConfig {
    pub max_tiles: usize,
    pub max_loaded_tiles: usize,
    pub interpolation: String,
}
