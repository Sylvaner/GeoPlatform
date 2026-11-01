//! Contexte : avant le téléchargement, chaque tuile est associée à un fichier local.
//! Usage : `cache.rs` construit la liste des tuiles manquantes ; `download.rs`
//! utilise leurs indices pour les requêtes et leur chemin pour l'enregistrement.

use std::path::PathBuf;

/// Adresse d'une tuile dans la grille et destination de son fichier sur disque.
pub struct CachedTile {
    /// Indice de ligne transmis au serveur.
    pub row: u64,
    /// Indice de colonne transmis au serveur.
    pub col: u64,
    /// Chemin possédé par cette structure, par exemple « .../12/34.bil ».
    /// PathBuf est l'équivalent d'un String pour un chemin de fichier.
    pub path: PathBuf,
}
