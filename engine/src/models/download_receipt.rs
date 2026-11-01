//! Contexte : un gros fichier LAZ non vide peut malgré tout être incomplet.
//! Usage : lidar/cache.rs enregistre ce reçu après la fin du téléchargement.
//! Un fichier n'est réutilisé que si son URL et sa taille correspondent au reçu.

use serde::{Deserialize, Serialize};

/// Preuve locale de fin du transfert, sans décompression ni export du nuage de points.
#[derive(Debug, Serialize, Deserialize)]
pub struct DownloadReceipt {
    /// Source exacte : une nouvelle URL d'édition doit provoquer un nouveau transfert.
    pub url: String,
    /// Taille totale effectivement écrite, comparée aussi au Content-Length si présent.
    pub size: u64,
}
