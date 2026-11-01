//! Contexte : suivi durable d'un travail asynchrone.
//! Usage : un job contient sa requête complète, son état et tous ses fichiers.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{Artifact, DownloadRequest, ExportRequest};

/// Travail confié à l'une des deux files de workers.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", content = "request", rename_all = "snake_case")]
pub enum JobKind {
    Download(DownloadRequest),
    Export(Box<ExportRequest>),
}

/// Cycle de vie observable d'un travail.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl JobStatus {
    /// Indique qu'aucun worker ne modifiera plus spontanément ce job.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

/// Enregistrement complet sauvegardé après chaque transition importante.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Job {
    pub id: String,
    pub kind: JobKind,
    pub status: JobStatus,
    pub progress_percent: u8,
    pub message: String,
    pub cache_key: String,
    pub cache_hit: bool,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub artifacts: Vec<Artifact>,
}

impl Job {
    /// Construit un travail en attente avec des timestamps cohérents.
    pub fn queued(kind: JobKind, cache_key: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            kind,
            status: JobStatus::Queued,
            progress_percent: 0,
            message: "En attente d'un worker".to_owned(),
            cache_key,
            cache_hit: false,
            created_at: Utc::now(),
            started_at: None,
            finished_at: None,
            artifacts: Vec::new(),
        }
    }
}
