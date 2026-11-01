//! Contexte : journal durable des actions utiles au diagnostic du serveur.
//! Usage : les écritures restent indépendantes des jobs et sont affichées du plus récent au plus ancien.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use tokio::{io::AsyncWriteExt, sync::Mutex};

use crate::error::{AppError, AppResult};

/// Niveau court destiné à l'interface et aux consommateurs de l'API.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerLogLevel {
    Info,
    Warning,
    Error,
}

/// Événement autonome écrit dans le journal JSONL.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ServerLogEntry {
    pub id: String,
    pub at: DateTime<Utc>,
    pub level: ServerLogLevel,
    pub action: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
}

/// Journal append-only protégé contre deux écritures simultanées.
#[derive(Debug)]
pub struct EventLog {
    path: PathBuf,
    write_lock: Mutex<()>,
}

impl EventLog {
    /// Prépare le fichier sous la racine d'état du serveur.
    pub async fn open(storage_root: &Path) -> AppResult<Self> {
        let directory = storage_root.join("state");
        tokio::fs::create_dir_all(&directory).await?;
        Ok(Self {
            path: directory.join("server.log.jsonl"),
            write_lock: Mutex::new(()),
        })
    }

    /// Ajoute une ligne JSON complète sans réécrire l'historique existant.
    pub async fn append(
        &self,
        level: ServerLogLevel,
        action: impl Into<String>,
        message: impl Into<String>,
        job_id: Option<&str>,
    ) -> AppResult<()> {
        let entry = ServerLogEntry {
            id: uuid::Uuid::new_v4().to_string(),
            at: Utc::now(),
            level,
            action: action.into(),
            message: message.into(),
            job_id: job_id.map(str::to_owned),
        };
        let level_label = match entry.level {
            ServerLogLevel::Info => "INFO",
            ServerLogLevel::Warning => "AVERTISSEMENT",
            ServerLogLevel::Error => "ERREUR",
        };
        let job_label = entry
            .job_id
            .as_deref()
            .map(|id| format!(" job={id}"))
            .unwrap_or_default();
        println!(
            "[{}] [SERVEUR] [{level_label}] {}{job_label} · {}",
            entry
                .at
                .with_timezone(&Local)
                .to_rfc3339_opts(SecondsFormat::Secs, false),
            entry.action,
            entry.message
        );
        let mut line = serde_json::to_vec(&entry)
            .map_err(|error| AppError::storage(format!("journal serveur : {error}")))?;
        line.push(b'\n');
        let _guard = self.write_lock.lock().await;
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;
        file.write_all(&line).await?;
        file.flush().await?;
        Ok(())
    }

    /// Retourne les dernières entrées dans l'ordre d'affichage du dashboard.
    pub async fn list(&self, limit: usize) -> AppResult<Vec<ServerLogEntry>> {
        let content = match tokio::fs::read_to_string(&self.path).await {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        content
            .lines()
            .rev()
            .take(limit.min(1_000))
            .map(|line| {
                serde_json::from_str(line).map_err(|error| {
                    AppError::storage(format!("entrée de journal invalide : {error}"))
                })
            })
            .collect()
    }
}
