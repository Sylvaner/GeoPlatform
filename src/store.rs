//! Contexte : persistance simple des jobs sous forme de JSON lisible.
//! Usage : chaque mutation remplace atomiquement un seul fichier, sans serveur de base de données.

use std::path::{Path, PathBuf};

use tokio::sync::Mutex;

use crate::{
    domain::{Artifact, Job},
    error::{AppError, AppResult},
};

/// Dépôt durable et sérialisé pour éviter deux écritures simultanées du même job.
#[derive(Debug)]
pub struct JobStore {
    root: PathBuf,
    write_lock: Mutex<()>,
}

impl JobStore {
    /// Crée le dossier d'état s'il n'existe pas encore.
    pub async fn open(storage_root: &Path) -> AppResult<Self> {
        let root = storage_root.join("state").join("jobs");
        tokio::fs::create_dir_all(&root).await?;
        Ok(Self {
            root,
            write_lock: Mutex::new(()),
        })
    }

    /// Sauvegarde un job par remplacement atomique.
    pub async fn save(&self, job: &Job) -> AppResult<()> {
        let _guard = self.write_lock.lock().await;
        let path = self.path(&job.id);
        let temporary = path.with_extension("json.part");
        let bytes =
            serde_json::to_vec_pretty(job).map_err(|error| AppError::storage(error.to_string()))?;
        tokio::fs::write(&temporary, bytes).await?;
        tokio::fs::rename(&temporary, &path).await?;
        Ok(())
    }

    /// Lit un job par son identifiant opaque.
    pub async fn get(&self, id: &str) -> AppResult<Job> {
        validate_id(id)?;
        let path = self.path(id);
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => AppError::not_found(format!("job {id}")),
                _ => AppError::from(error),
            })?;
        serde_json::from_slice(&bytes).map_err(|error| {
            AppError::storage(format!("job {} invalide : {error}", path.display()))
        })
    }

    /// Supprime un enregistrement de job sans toucher aux fichiers qu'il référence.
    pub async fn delete(&self, id: &str) -> AppResult<()> {
        validate_id(id)?;
        let _guard = self.write_lock.lock().await;
        match tokio::fs::remove_file(self.path(id)).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(AppError::not_found(format!("job {id}")))
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Retourne les jobs du plus récent au plus ancien.
    pub async fn list(&self) -> AppResult<Vec<Job>> {
        let mut directory = tokio::fs::read_dir(&self.root).await?;
        let mut jobs = Vec::new();
        while let Some(entry) = directory.next_entry().await? {
            if entry.path().extension().and_then(|part| part.to_str()) != Some("json") {
                continue;
            }
            let bytes = tokio::fs::read(entry.path()).await?;
            let job = serde_json::from_slice(&bytes).map_err(|error| {
                AppError::storage(format!("job {} invalide : {error}", entry.path().display()))
            })?;
            jobs.push(job);
        }
        jobs.sort_by_key(|job: &Job| std::cmp::Reverse(job.created_at));
        Ok(jobs)
    }

    /// Cherche un fichier géré parmi les jobs terminés.
    pub async fn find_artifact(&self, id: &str) -> AppResult<Artifact> {
        self.list()
            .await?
            .into_iter()
            .flat_map(|job| job.artifacts)
            .find(|artifact| artifact.id == id)
            .ok_or_else(|| AppError::not_found(format!("fichier {id}")))
    }

    /// Agrège les artefacts connus sans créer un second catalogue à synchroniser.
    pub async fn list_artifacts(&self) -> AppResult<Vec<Artifact>> {
        let mut artifacts: Vec<_> = self
            .list()
            .await?
            .into_iter()
            .flat_map(|job| job.artifacts)
            .collect();
        artifacts.sort_by_key(|artifact| std::cmp::Reverse(artifact.created_at));
        artifacts.dedup_by(|left, right| left.id == right.id);
        Ok(artifacts)
    }

    /// Produit le chemin privé correspondant à un identifiant déjà validé.
    fn path(&self, id: &str) -> PathBuf {
        self.root.join(format!("{id}.json"))
    }
}

/// Refuse les séparateurs de chemin avant tout accès au disque.
fn validate_id(id: &str) -> AppResult<()> {
    if uuid::Uuid::parse_str(id).is_err() {
        return Err(AppError::validation("identifiant de job invalide"));
    }
    Ok(())
}
