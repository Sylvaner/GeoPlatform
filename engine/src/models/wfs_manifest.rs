//! Contexte : une recherche WFS peut nécessiter plusieurs fichiers de réponse.
//! Usage : complete.json est écrit seulement après la dernière page ; il permet
//! de réutiliser une recherche terminée et de refuser un cache incomplet.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct WfsManifest {
    pub source: String,
    pub layer: String,
    pub bbox: [f64; 4],
    pub sort_by: String,
    /// Décalage de chaque page, dans l'ordre de lecture.
    pub pages: Vec<usize>,
    pub features: usize,
}
