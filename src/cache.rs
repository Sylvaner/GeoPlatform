//! Contexte : index de cache final, distinct des tuiles et surfaces conservées par les moteurs.
//! Usage : une clé identique réutilise immédiatement les artefacts encore présents sur disque.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    domain::{Artifact, JobKind},
    error::{AppError, AppResult},
};

/// Version à changer lorsqu'une évolution rend les anciens résultats incompatibles.
const CACHE_SCHEMA: u32 = 1;

/// Résultat terminal réutilisable sans relancer un worker.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CacheEntry {
    pub schema: u32,
    pub artifacts: Vec<Artifact>,
}

/// Petit index de fichiers JSON, un fichier indépendant par clé.
#[derive(Debug)]
pub struct ResultCache {
    root: PathBuf,
}

impl ResultCache {
    /// Prépare le dossier d'index sous la racine du stockage.
    pub async fn open(storage_root: &Path) -> AppResult<Self> {
        let root = storage_root.join("cache").join("results");
        tokio::fs::create_dir_all(&root).await?;
        Ok(Self { root })
    }

    /// Calcule une clé déterministe à partir du contrat métier complet.
    pub fn key(kind: &JobKind) -> AppResult<String> {
        // `force` commande l'orchestration, pas le contenu. L'exclure permet à un
        // recalcul forcé de remplacer la même entrée au lieu de créer un cache parallèle.
        let mut content = kind.clone();
        match &mut content {
            JobKind::Download(request) => {
                request.force = false;
                request.name = None;
            }
            JobKind::Export(request) => request.force = false,
        }
        let bytes = serde_json::to_vec(&(CACHE_SCHEMA, content))
            .map_err(|error| AppError::storage(error.to_string()))?;
        Ok(hex::encode(Sha256::digest(bytes)))
    }

    /// Charge une entrée seulement si tous ses fichiers existent encore.
    pub async fn get(&self, key: &str) -> AppResult<Option<CacheEntry>> {
        let path = self.path(key);
        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let entry: CacheEntry = serde_json::from_slice(&bytes).map_err(|error| {
            AppError::storage(format!(
                "index de cache {} invalide : {error}",
                path.display()
            ))
        })?;
        if entry.schema != CACHE_SCHEMA {
            return Ok(None);
        }
        for artifact in &entry.artifacts {
            if tokio::fs::metadata(&artifact.path).await.is_err() {
                return Ok(None);
            }
        }
        Ok(Some(entry))
    }

    /// Écrit atomiquement la liste des artefacts d'un calcul terminé.
    pub async fn put(&self, key: &str, artifacts: &[Artifact]) -> AppResult<()> {
        let entry = CacheEntry {
            schema: CACHE_SCHEMA,
            artifacts: artifacts.to_vec(),
        };
        let path = self.path(key);
        let temporary = path.with_extension("json.part");
        let bytes = serde_json::to_vec_pretty(&entry)
            .map_err(|error| AppError::storage(error.to_string()))?;
        tokio::fs::write(&temporary, bytes).await?;
        tokio::fs::rename(&temporary, &path).await?;
        Ok(())
    }

    /// Supprime seulement l'index final ; les tuiles sources restent intactes.
    pub async fn invalidate(&self, key: &str) -> AppResult<()> {
        match tokio::fs::remove_file(self.path(key)).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    /// Transforme une clé contrôlée en chemin local.
    fn path(&self, key: &str) -> PathBuf {
        self.root.join(format!("{key}.json"))
    }
}
