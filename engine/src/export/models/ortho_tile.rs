//! Contexte : une tuile désigne le JPEG original du cache, sans nouvelle compression.
//! Usage : Blender partage ce même fichier intégré entre le terrain et les toits.

use serde::Serialize;
#[derive(Debug, Serialize)]
pub struct OrthoTile {
    pub col: u64,
    pub row: u64,
    pub path: String,
    pub width: u64,
    pub height: u64,
}
