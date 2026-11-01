//! Contexte : synthèse prête à afficher par le futur dashboard.
//! Usage : agréger l'état des jobs, l'espace disque et les zones sans lancer de calcul.

use std::{collections::BTreeMap, path::Path};

use serde::Serialize;

use crate::{
    domain::{JobKind, JobStatus},
    error::{AppError, AppResult},
    service::AppService,
};

/// Vue compacte de l'activité et du stockage.
#[derive(Debug, Serialize)]
pub struct Dashboard {
    pub jobs: JobCounters,
    pub storage: StorageSummary,
    pub locations: Vec<LocationSummary>,
}

/// Nombre de jobs dans chaque étape du cycle de vie.
#[derive(Debug, Default, Serialize)]
pub struct JobCounters {
    pub queued: usize,
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
    pub cancelled: usize,
}

/// Occupation totale et détail par couche source.
#[derive(Debug, Serialize)]
pub struct StorageSummary {
    pub root: String,
    pub total_bytes: u64,
    pub data_bytes: u64,
    pub exports_bytes: u64,
    pub by_layer_bytes: BTreeMap<String, u64>,
}

/// Zone demandée, utilisable pour centrer une carte de navigation.
#[derive(Debug, Serialize)]
pub struct LocationSummary {
    pub job_id: String,
    pub center: [f64; 2],
    pub size_m: f64,
    pub crs: String,
    pub status: JobStatus,
}

/// Produit toutes les informations du dashboard en une seule lecture cohérente.
pub async fn build(service: &AppService) -> AppResult<Dashboard> {
    let jobs = service.list_jobs().await?;
    let mut counters = JobCounters::default();
    let mut locations = Vec::new();
    for (index, job) in jobs.iter().enumerate() {
        match job.status {
            JobStatus::Queued => counters.queued += 1,
            JobStatus::Running => counters.running += 1,
            JobStatus::Completed => counters.completed += 1,
            JobStatus::Failed => counters.failed += 1,
            JobStatus::Cancelled => counters.cancelled += 1,
        }
        // La carte possède sa route paginée ; le dashboard garde seulement un
        // échantillon récent afin que sa réponse reste petite avec le temps.
        if index < 200 {
            let area = match &job.kind {
                JobKind::Download(request) => &request.area,
                JobKind::Export(request) => &request.area,
            };
            locations.push(LocationSummary {
                job_id: job.id.clone(),
                center: area.center,
                size_m: area.size_m,
                crs: area.crs().to_owned(),
                status: job.status,
            });
        }
    }
    let config = service.configuration().await;
    let root = config.storage.root.clone();
    let data_root = config.data_root();
    let exports_root = config.exports_root();
    let sizes =
        tokio::task::spawn_blocking(move || storage_sizes(&root, &data_root, &exports_root))
            .await
            .map_err(|error| AppError::storage(format!("calcul de l'espace disque : {error}")))??;
    Ok(Dashboard {
        jobs: counters,
        storage: sizes,
        locations,
    })
}

/// Parcourt le disque dans une tâche bloquante isolée du serveur HTTP.
fn storage_sizes(root: &Path, data_root: &Path, exports_root: &Path) -> AppResult<StorageSummary> {
    let mut by_layer_bytes = BTreeMap::new();
    for (folder, label) in [
        ("RGE_ALTI", "elevation"),
        ("BD_ORTHO", "orthophoto"),
        ("LIDAR_HD", "lidar"),
        ("BD_TOPO", "bd_topo"),
        ("CADASTRE", "cadastre"),
        ("BD_TOPAGE", "hydrography"),
    ] {
        by_layer_bytes.insert(label.to_owned(), directory_size(&data_root.join(folder))?);
    }
    Ok(StorageSummary {
        root: root.to_string_lossy().into_owned(),
        total_bytes: directory_size(root)?,
        data_bytes: directory_size(data_root)?,
        exports_bytes: directory_size(exports_root)?,
        by_layer_bytes,
    })
}

/// Additionne récursivement les fichiers réguliers et ignore les dossiers absents.
fn directory_size(root: &Path) -> AppResult<u64> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.into()),
    };
    let mut size = 0_u64;
    for entry in entries {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            size = size.saturating_add(directory_size(&entry.path())?);
        } else if metadata.is_file() {
            size = size.saturating_add(metadata.len());
        }
    }
    Ok(size)
}
