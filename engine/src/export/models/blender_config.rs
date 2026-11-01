//! Contexte : seul Blender peut enregistrer sa scène native au format .blend.
//! Usage : executable=auto cherche Blender dans PATH puis les dossiers usuels Windows.

use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlenderConfig {
    pub executable: String,
    pub compress_file: bool,
    pub threads: usize,
}
