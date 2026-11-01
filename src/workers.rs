//! Contexte : deux dispatchers indépendants empêchent un export lourd de bloquer les téléchargements.
//! Usage : chaque dispatcher limite sa concurrence avec un sémaphore puis lance un job isolé.

use std::sync::Arc;

use tokio::sync::{Semaphore, mpsc};

use crate::{domain::JobStatus, service::AppService};

/// Démarre un dispatcher nommé avec une limite fixe de travaux simultanés.
pub fn spawn_dispatcher(
    name: &'static str,
    mut receiver: mpsc::Receiver<String>,
    concurrency: usize,
    service: Arc<AppService>,
) {
    tokio::spawn(async move {
        let permits = Arc::new(Semaphore::new(concurrency));
        while let Some(job_id) = receiver.recv().await {
            let permit = match Arc::clone(&permits).acquire_owned().await {
                Ok(permit) => permit,
                Err(_) => break,
            };
            let service = Arc::clone(&service);
            tokio::spawn(async move {
                let _permit = permit;
                if let Err(error) = service.process_job(&job_id).await {
                    tracing::error!(worker = name, job_id, %error, "échec du worker");
                    if let Ok(mut job) = service.store.get(&job_id).await
                        && !job.status.is_terminal()
                    {
                        job.status = JobStatus::Failed;
                        job.message = error.to_string();
                        job.finished_at = Some(chrono::Utc::now());
                        let _ = service.store.save(&job).await;
                    }
                    service.release_job(&job_id).await;
                }
            });
        }
    });
}

/// Démarre le worker de préchauffage séparément des téléchargements et exports utilisateur.
pub fn spawn_cache_dispatcher(mut receiver: mpsc::Receiver<String>, service: Arc<AppService>) {
    tokio::spawn(async move {
        let permits = Arc::new(Semaphore::new(1));
        while let Some(job_id) = receiver.recv().await {
            let permit = match Arc::clone(&permits).acquire_owned().await {
                Ok(permit) => permit,
                Err(_) => break,
            };
            let service = Arc::clone(&service);
            tokio::spawn(async move {
                let _permit = permit;
                if let Err(error) = service.process_cache_warmup(&job_id).await {
                    tracing::warn!(job_id, %error, "échec du préchauffage STL");
                }
            });
        }
    });
}
