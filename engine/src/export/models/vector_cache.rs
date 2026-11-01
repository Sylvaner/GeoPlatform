//! Contexte : un cache vectoriel peut couvrir toute la zone ou seulement une partie.
//! Usage : garder son identité, les pages utilisables et son statut sans prétendre le compléter.

use std::path::PathBuf;
#[derive(Debug)]
pub struct VectorCache {
    pub directory: PathBuf,
    pub source_path: PathBuf,
    pub bbox: [f64; 4],
    pub complete: bool,
    pub pages: Vec<(usize, PathBuf)>,
    pub expected_features: Option<usize>,
}
