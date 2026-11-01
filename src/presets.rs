//! Contexte : persistance légère des profils d'export nommés.
//! Usage : un seul fichier JSON lisible suffit car la collection reste volontairement petite.

use std::path::{Path, PathBuf};

use tokio::sync::Mutex;

use crate::{
    domain::{ExportPreset, ExportPresetDraft},
    error::{AppError, AppResult},
};

/// Stockage sérialisé des profils avec un verrou limité aux écritures.
#[derive(Debug)]
pub struct ExportPresetStore {
    path: PathBuf,
    write_lock: Mutex<()>,
}

impl ExportPresetStore {
    /// Prépare le fichier sous le même répertoire d'état que les jobs.
    pub async fn open(storage_root: &Path) -> AppResult<Self> {
        let state_root = storage_root.join("state");
        tokio::fs::create_dir_all(&state_root).await?;
        Ok(Self {
            path: state_root.join("export-presets.json"),
            write_lock: Mutex::new(()),
        })
    }

    /// Retourne les profils du plus récent au plus ancien.
    pub async fn list(&self) -> AppResult<Vec<ExportPreset>> {
        let _guard = self.write_lock.lock().await;
        let mut presets = self.read_all().await?;
        presets.sort_by_key(|preset| std::cmp::Reverse(preset.created_at));
        Ok(presets)
    }

    /// Ajoute un profil validé et remplace un homonyme pour garder la liste claire.
    pub async fn create(&self, draft: ExportPresetDraft) -> AppResult<ExportPreset> {
        let _guard = self.write_lock.lock().await;
        let draft = draft.normalized()?;
        let mut presets = self.read_all().await?;
        presets.retain(|preset| !preset.name.eq_ignore_ascii_case(&draft.name));
        let preset = ExportPreset::from_draft(draft);
        presets.push(preset.clone());
        self.write_all(&presets).await?;
        Ok(preset)
    }

    /// Supprime un profil par identifiant opaque.
    pub async fn delete(&self, id: &str) -> AppResult<()> {
        uuid::Uuid::parse_str(id)
            .map_err(|_| AppError::validation("identifiant de profil invalide"))?;
        let _guard = self.write_lock.lock().await;
        let mut presets = self.read_all().await?;
        let previous = presets.len();
        presets.retain(|preset| preset.id != id);
        if presets.len() == previous {
            return Err(AppError::not_found(format!("profil {id}")));
        }
        self.write_all(&presets).await
    }

    /// Lit une collection absente comme une liste vide.
    async fn read_all(&self) -> AppResult<Vec<ExportPreset>> {
        let bytes = match tokio::fs::read(&self.path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        serde_json::from_slice(&bytes).map_err(|error| {
            AppError::storage(format!(
                "profils d'export {} invalides : {error}",
                self.path.display()
            ))
        })
    }

    /// Remplace le petit catalogue complet sous le verrou du stockage.
    async fn write_all(&self, presets: &[ExportPreset]) -> AppResult<()> {
        let bytes = serde_json::to_vec_pretty(presets)
            .map_err(|error| AppError::storage(error.to_string()))?;
        tokio::fs::write(&self.path, bytes).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AdvancedExport, DataLayer, ExportFormat, ExportQuality};

    /// Vérifie qu'un nom réutilisé met à jour le profil sans créer de doublon.
    #[tokio::test]
    async fn same_name_replaces_previous_preset() {
        let directory =
            tempfile::tempdir_in(std::env::current_dir().expect("répertoire courant disponible"))
                .expect("dossier temporaire");
        let store = ExportPresetStore::open(directory.path())
            .await
            .expect("stockage");
        let draft = |format| ExportPresetDraft {
            name: "Maquette urbaine".to_owned(),
            format,
            layers: vec![DataLayer::Lidar],
            quality: ExportQuality::Standard,
            advanced: AdvancedExport::default(),
        };

        store
            .create(draft(ExportFormat::Stl))
            .await
            .expect("premier profil");
        store
            .create(draft(ExportFormat::Blender))
            .await
            .expect("profil remplacé");

        let presets = store.list().await.expect("liste");
        assert_eq!(presets.len(), 1);
        assert_eq!(presets[0].format, ExportFormat::Blender);
    }
}
