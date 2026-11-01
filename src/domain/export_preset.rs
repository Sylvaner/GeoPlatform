//! Contexte : profils d'export nommés et réutilisables par le front comme par l'API.
//! Usage : un profil conserve uniquement les choix de rendu, jamais une zone géographique.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

use super::{AdvancedExport, DataLayer, ExportFormat, ExportQuality};

/// Configuration nommée reçue lors de la dernière étape du wizard.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExportPresetDraft {
    pub name: String,
    pub format: ExportFormat,
    #[serde(default)]
    pub layers: Vec<DataLayer>,
    #[serde(default)]
    pub quality: ExportQuality,
    #[serde(default)]
    pub advanced: AdvancedExport,
}

impl ExportPresetDraft {
    /// Nettoie le nom et stabilise l'ordre des couches avant la sauvegarde.
    pub fn normalized(mut self) -> AppResult<Self> {
        self.name = self.name.trim().to_owned();
        if self.name.is_empty() || self.name.chars().count() > 60 {
            return Err(AppError::validation(
                "le nom du profil doit contenir entre 1 et 60 caractères",
            ));
        }
        self.advanced.validate()?;
        self.layers.sort_unstable();
        self.layers.dedup();
        Ok(self)
    }
}

/// Profil durable avec son identifiant et sa date de création.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ExportPreset {
    pub id: String,
    pub name: String,
    pub format: ExportFormat,
    pub layers: Vec<DataLayer>,
    pub quality: ExportQuality,
    pub advanced: AdvancedExport,
    pub created_at: DateTime<Utc>,
}

impl ExportPreset {
    /// Transforme un brouillon validé en ressource persistante.
    pub fn from_draft(draft: ExportPresetDraft) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: draft.name,
            format: draft.format,
            layers: draft.layers,
            quality: draft.quality,
            advanced: draft.advanced,
            created_at: Utc::now(),
        }
    }
}
