//! Contexte : l'export lit un cache et crée ses propres fichiers de travail.
//! Usage : les chemins relatifs partent du dossier qui contient le TOML de configuration.
//! Les chemins absolus permettent un cache externe ou réseau et une sortie sur un autre disque.

use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PathsConfig {
    pub cache_root: String,
    pub output_file: String,
    pub work_directory: String,
    pub keep_work_files: bool,
    pub overwrite: bool,
    pub prepare_only: bool,
}
