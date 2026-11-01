//! Contexte : les LAZ sont volumineux et un fichier partiel ne doit pas être réutilisé.
//! Usage : find_missing vérifie les reçus ; le téléchargeur écrit un reçu après réussite.
//! Les fichiers .part restent ignorés. Une URL d'édition différente invalide le reçu.

use anyhow::Result;
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::models::{DownloadReceipt, LidarTile};

/// Renvoie les dalles à récupérer et le nombre de fichiers déjà vérifiés dans le cache.
pub async fn find_missing(
    root: &Path,
    tiles: Vec<LidarTile>,
    force: bool,
) -> (Vec<LidarTile>, usize) {
    let mut missing = Vec::new();
    let mut present = 0;
    for tile in tiles {
        if !force && is_complete(&root.join(&tile.relative_path), &tile.url).await {
            present += 1;
        } else {
            missing.push(tile);
        }
    }
    (missing, present)
}

/// Le reçu accompagne le fichier original : nom.copc.laz.download.json.
pub fn receipt_path(path: &Path) -> PathBuf {
    path.with_extension("laz.download.json")
}

/// Vérifie que le transfert précédent s'est terminé, pour cette URL et cette taille.
pub async fn is_complete(path: &Path, url: &str) -> bool {
    let Ok(metadata) = fs::metadata(path).await else {
        return false;
    };
    if !metadata.is_file() || metadata.len() < 227 {
        return false;
    }
    let Ok(bytes) = fs::read(receipt_path(path)).await else {
        return false;
    };
    let Ok(receipt) = serde_json::from_slice::<DownloadReceipt>(&bytes) else {
        return false;
    };
    receipt.url == url && receipt.size == metadata.len()
}

/// Le reçu est lui aussi écrit via un temporaire : pas de reçu partiellement valide.
pub async fn save_receipt(path: &Path, url: &str, size: u64) -> Result<()> {
    let receipt = DownloadReceipt {
        url: url.to_owned(),
        size,
    };
    crate::cache::write_tile(&receipt_path(path), &serde_json::to_vec_pretty(&receipt)?).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support as support;

    #[tokio::test]
    async fn large_file_without_receipt_is_not_complete() {
        let root = support::directory();
        let path = root.join("tile.laz");
        fs::write(&path, vec![0; 300]).await.unwrap();
        assert!(!is_complete(&path, "source").await);
        support::cleanup(&root);
    }
    #[tokio::test]
    async fn malformed_receipt_is_not_complete() {
        let root = support::directory();
        let path = root.join("tile.laz");
        fs::write(&path, vec![0; 300]).await.unwrap();
        fs::write(receipt_path(&path), b"{").await.unwrap();
        assert!(!is_complete(&path, "source").await);
        support::cleanup(&root);
    }
    #[tokio::test]
    async fn changed_file_size_invalidates_its_receipt() {
        let root = support::directory();
        let path = root.join("tile.laz");
        fs::write(&path, vec![0; 300]).await.unwrap();
        save_receipt(&path, "source", 300).await.unwrap();
        fs::write(&path, vec![0; 299]).await.unwrap();
        assert!(!is_complete(&path, "source").await);
        support::cleanup(&root);
    }
    #[tokio::test]
    async fn undersized_laz_is_rejected_even_with_matching_receipt() {
        let root = support::directory();
        let path = root.join("tile.laz");
        fs::write(&path, vec![0; 100]).await.unwrap();
        save_receipt(&path, "source", 100).await.unwrap();
        assert!(!is_complete(&path, "source").await);
        support::cleanup(&root);
    }
}
