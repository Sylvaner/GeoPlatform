//! Contexte : chaque couche devient une collection Blender que l'on peut masquer.
//! Usage : enabled=false exclut ses données ; visible=false conserve la collection masquée.
//! Les dimensions de lignes et points sont des choix d'affichage, pas des mesures nouvelles.

use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LayerConfig {
    pub enabled: bool,
    pub visible: bool,
    pub color: [f64; 4],
    pub line_width_m: f64,
    pub z_offset_m: f64,
    pub fallback_height_m: f64,
}
