//! Contexte : le fichier est préparé pour un rendu léger ; le calcul d'image reste optionnel.
//! Usage : preview=true crée une image de contrôle en plus du .blend.

use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RenderConfig {
    pub engine: String,
    pub resolution_x: u32,
    pub resolution_y: u32,
    pub samples: u32,
    pub preview: bool,
    pub preview_file: String,
    pub transparent_background: bool,
}
