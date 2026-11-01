//! Contexte : description persistante d'un fichier produit par un worker.
//! Usage : l'API expose la description et sert le contenu sans révéler de chemin interne.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Nature d'un fichier géré par la plateforme.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    DownloadManifest,
    Stl,
    Blender,
    Preview,
    WorkerLog,
    Configuration,
}

/// Métadonnées nécessaires au téléchargement et au dashboard.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Artifact {
    pub id: String,
    pub job_id: String,
    pub name: String,
    pub kind: ArtifactKind,
    pub media_type: String,
    pub size_bytes: u64,
    pub created_at: DateTime<Utc>,
    /// Chemin absolu conservé pour la reprise et affiché comme information de gestion.
    pub path: String,
}
