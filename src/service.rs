//! Contexte : façade métier commune aux routes HTTP et aux commandes locales.
//! Usage : normaliser, mettre en cache et envoyer les jobs sans exposer les détails des workers.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use chrono::Utc;
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

use crate::{
    backend::{EngineBackend, ProcessingBackend},
    cache::ResultCache,
    config::AppConfig,
    domain::{
        Area, Artifact, ArtifactKind, DataLayer, DownloadRequest, ExportPreset, ExportPresetDraft,
        ExportRequest, Job, JobKind, JobStatus,
    },
    error::{AppError, AppResult},
    logs::{EventLog, ServerLogEntry, ServerLogLevel},
    presets::ExportPresetStore,
    store::JobStore,
    workers,
};

/// Orchestrateur léger, partageable sans verrou global autour des traitements.
pub struct AppService {
    pub(crate) config: Arc<Mutex<AppConfig>>,
    pub(crate) store: Arc<JobStore>,
    pub(crate) cache: Arc<ResultCache>,
    presets: Arc<ExportPresetStore>,
    logs: Arc<EventLog>,
    backend: Arc<dyn ProcessingBackend>,
    download_sender: mpsc::Sender<String>,
    export_sender: mpsc::Sender<String>,
    cache_warmup_sender: mpsc::Sender<String>,
    active_keys: Mutex<HashMap<String, String>>,
    cancellations: Mutex<HashMap<String, CancellationToken>>,
    cache_warmup_pending: Mutex<HashSet<String>>,
}

impl AppService {
    /// Construit le service de production avec l'adaptateur vers les moteurs existants.
    pub async fn start(config: AppConfig) -> AppResult<Arc<Self>> {
        let shared_config = Arc::new(Mutex::new(config));
        let backend = Arc::new(EngineBackend::new(Arc::clone(&shared_config)));
        Self::start_with_backend(shared_config, backend).await
    }

    /// Point d'injection utilisé par les tests et les futurs moteurs natifs.
    pub async fn start_with_backend(
        config: Arc<Mutex<AppConfig>>,
        backend: Arc<dyn ProcessingBackend>,
    ) -> AppResult<Arc<Self>> {
        let snapshot = config.lock().await.clone();
        tokio::fs::create_dir_all(&snapshot.storage.root).await?;
        let partial_files_removed =
            cleanup_abandoned_partial_files(snapshot.storage.root.clone()).await?;
        let store = Arc::new(JobStore::open(&snapshot.storage.root).await?);
        let work_directories_removed =
            cleanup_abandoned_work_directories(&snapshot, store.as_ref()).await?;
        let cache = Arc::new(ResultCache::open(&snapshot.storage.root).await?);
        let presets = Arc::new(ExportPresetStore::open(&snapshot.storage.root).await?);
        let logs = Arc::new(EventLog::open(&snapshot.storage.root).await?);
        let (download_sender, download_receiver) = mpsc::channel(256);
        let (export_sender, export_receiver) = mpsc::channel(256);
        let (cache_warmup_sender, cache_warmup_receiver) = mpsc::channel(256);
        let service = Arc::new(Self {
            config,
            store,
            cache,
            presets,
            logs,
            backend,
            download_sender,
            export_sender,
            cache_warmup_sender,
            active_keys: Mutex::new(HashMap::new()),
            cancellations: Mutex::new(HashMap::new()),
            cache_warmup_pending: Mutex::new(HashSet::new()),
        });
        workers::spawn_dispatcher(
            "download",
            download_receiver,
            snapshot.workers.downloads,
            Arc::clone(&service),
        );
        workers::spawn_dispatcher(
            "export",
            export_receiver,
            snapshot.workers.exports,
            Arc::clone(&service),
        );
        workers::spawn_cache_dispatcher(cache_warmup_receiver, Arc::clone(&service));
        if partial_files_removed > 0 || work_directories_removed > 0 {
            service
                .log(
                    ServerLogLevel::Info,
                    "startup_cleanup",
                    format!(
                        "Nettoyage de démarrage : {partial_files_removed} téléchargement(s) partiel(s) et {work_directories_removed} dossier(s) de travail abandonné(s) supprimés",
                    ),
                    None,
                )
                .await;
        }
        service.resume_interrupted_jobs().await?;
        service.enqueue_existing_cache_warmups().await?;
        service
            .log(
                ServerLogLevel::Info,
                "server_started",
                "Serveur prêt et files de workers démarrées",
                None,
            )
            .await;
        Ok(service)
    }

    /// Normalise puis soumet une demande de téléchargement.
    pub async fn submit_download(&self, request: DownloadRequest) -> AppResult<Job> {
        self.submit(JobKind::Download(request.normalized()?)).await
    }

    /// Normalise puis soumet une demande d'export.
    pub async fn submit_export(&self, mut request: ExportRequest) -> AppResult<Job> {
        let default_colors = self.config.lock().await.render.colors.clone();
        match request.advanced.colors.as_mut() {
            Some(colors) => colors.fill_missing_from(&default_colors),
            None => request.advanced.colors = Some(default_colors),
        }
        self.submit(JobKind::Export(Box::new(request.normalized()?)))
            .await
    }

    /// Retourne un job depuis le stockage durable.
    pub async fn get_job(&self, id: &str) -> AppResult<Job> {
        self.store.get(id).await
    }

    /// Retourne l'historique complet du plus récent au plus ancien.
    pub async fn list_jobs(&self) -> AppResult<Vec<Job>> {
        self.remove_missing_exports().await?;
        self.store.list().await
    }

    /// Vérifie au démarrage les zones terminées et programme les surfaces STL manquantes.
    async fn enqueue_existing_cache_warmups(&self) -> AppResult<()> {
        let warmup = self.config.lock().await.cache_warmup.clone();
        if warmup.stl_grid_steps_m.is_empty() && warmup.blender_atlas_resolutions_px.is_empty() {
            return Ok(());
        }
        for job in self.store.list().await? {
            self.enqueue_cache_warmup(&job).await?;
        }
        Ok(())
    }

    /// Ajoute une seule fois à la file les zones terminées contenant du LiDAR.
    async fn enqueue_cache_warmup(&self, job: &Job) -> AppResult<()> {
        let warmup = self.config.lock().await.cache_warmup.clone();
        let JobKind::Download(request) = &job.kind else {
            return Ok(());
        };
        let needs_stl = !warmup.stl_grid_steps_m.is_empty()
            && (request.layers.is_empty() || request.layers.contains(&DataLayer::Lidar));
        let needs_blender = !warmup.blender_atlas_resolutions_px.is_empty()
            && (request.layers.is_empty() || request.layers.contains(&DataLayer::Orthophoto))
            && (request.layers.is_empty()
                || request.layers.contains(&DataLayer::Elevation)
                || request.layers.contains(&DataLayer::Lidar));
        if job.status != JobStatus::Completed || (!needs_stl && !needs_blender) {
            return Ok(());
        }
        let mut pending = self.cache_warmup_pending.lock().await;
        if !pending.insert(job.id.clone()) {
            return Ok(());
        }
        drop(pending);
        if self.cache_warmup_sender.send(job.id.clone()).await.is_err() {
            self.cache_warmup_pending.lock().await.remove(&job.id);
            return Err(AppError::storage("la file de préchauffage STL est arrêtée"));
        }
        Ok(())
    }

    /// Exécute les surfaces configurées sans changer l'état du job de téléchargement.
    pub(crate) async fn process_cache_warmup(&self, id: &str) -> AppResult<()> {
        let result = async {
            let job = self.store.get(id).await?;
            let JobKind::Download(request) = &job.kind else {
                return Ok(());
            };
            if job.status != JobStatus::Completed {
                return Ok(());
            }
            let warmup = self.config.lock().await.cache_warmup.clone();
            let needs_stl = !warmup.stl_grid_steps_m.is_empty()
                && (request.layers.is_empty() || request.layers.contains(&DataLayer::Lidar));
            let needs_blender = !warmup.blender_atlas_resolutions_px.is_empty()
                && (request.layers.is_empty() || request.layers.contains(&DataLayer::Orthophoto))
                && (request.layers.is_empty()
                    || request.layers.contains(&DataLayer::Elevation)
                    || request.layers.contains(&DataLayer::Lidar));
            if !needs_stl && !needs_blender {
                return Ok(());
            }
            self.log(
                ServerLogLevel::Info,
                "cache_warmup_started",
                format!(
                    "Pas STL : {:?} m ; atlas Blender : {:?} px",
                    warmup.stl_grid_steps_m, warmup.blender_atlas_resolutions_px
                ),
                Some(id),
            )
            .await;
            if needs_stl {
                self.backend
                    .prewarm_stl_surfaces(
                        request,
                        &warmup.stl_grid_steps_m,
                        CancellationToken::new(),
                    )
                    .await?;
            }
            if needs_blender {
                self.backend
                    .prewarm_blender_atlases(
                        request,
                        &warmup.blender_atlas_resolutions_px,
                        CancellationToken::new(),
                    )
                    .await?;
            }
            self.log(
                ServerLogLevel::Info,
                "cache_warmup_completed",
                "Préchauffages terminés ou caches déjà présents",
                Some(id),
            )
            .await;
            Ok(())
        }
        .await;
        self.cache_warmup_pending.lock().await.remove(id);
        result
    }

    /// Retourne les dernières actions persistées par le serveur.
    pub async fn list_server_logs(&self, limit: usize) -> AppResult<Vec<ServerLogEntry>> {
        self.logs.list(limit).await
    }

    /// Retire une zone de la carte sans effacer les données sources partagées.
    pub async fn delete_zone(&self, id: &str) -> AppResult<()> {
        let job = self.store.get(id).await?;
        if !matches!(&job.kind, JobKind::Download(_)) {
            return Err(AppError::validation(
                "seul un téléchargement peut être supprimé comme zone",
            ));
        }
        if !job.status.is_terminal() {
            return Err(AppError::Conflict(
                "annulez le téléchargement avant de supprimer cette zone".to_owned(),
            ));
        }
        self.remove_zone_job(&job).await?;
        self.log(
            ServerLogLevel::Info,
            "zone_deleted",
            "Zone et données propres supprimées ; les tuiles partagées restent dédupliquées",
            Some(id),
        )
        .await;
        Ok(())
    }

    /// Renomme une zone existante sans toucher à ses fichiers ni à sa clé de cache.
    pub async fn rename_zone(&self, id: &str, name: String) -> AppResult<Job> {
        let mut job = self.store.get(id).await?;
        let JobKind::Download(request) = &mut job.kind else {
            return Err(AppError::validation(
                "seul un téléchargement peut être renommé comme zone",
            ));
        };
        request.rename(name)?;
        self.store.save(&job).await?;
        self.log(
            ServerLogLevel::Info,
            "zone_renamed",
            "Zone renommée",
            Some(id),
        )
        .await;
        Ok(job)
    }

    /// Demande l'arrêt d'un processus et marque immédiatement un job encore en file.
    pub async fn cancel_job(&self, id: &str) -> AppResult<Job> {
        let mut job = self.store.get(id).await?;
        if job.status.is_terminal() {
            return Err(AppError::Conflict("ce job est déjà terminé".to_owned()));
        }
        if let Some(token) = self.cancellations.lock().await.get(id) {
            token.cancel();
        }
        job.status = JobStatus::Cancelled;
        job.message = "Annulation demandée".to_owned();
        job.finished_at = Some(Utc::now());
        self.store.save(&job).await?;
        self.log(
            ServerLogLevel::Info,
            "job_cancel_requested",
            "Annulation demandée par l’utilisateur",
            Some(id),
        )
        .await;
        self.release_job(id).await;
        Ok(job)
    }

    /// Supprime un export terminé avec ses artefacts et son entrée d'historique.
    pub async fn delete_export(&self, id: &str) -> AppResult<()> {
        let job = self.store.get(id).await?;
        if !matches!(&job.kind, JobKind::Export(_)) {
            return Err(AppError::validation(
                "seul un export peut être supprimé depuis cet historique",
            ));
        }
        if !job.status.is_terminal() {
            return Err(AppError::Conflict(
                "annulez l’export avant de le supprimer".to_owned(),
            ));
        }
        for artifact in job.artifacts.clone() {
            self.delete_artifact(&artifact.id).await?;
        }
        self.store.delete(id).await?;
        self.release_job(id).await;
        self.log(
            ServerLogLevel::Info,
            "export_deleted",
            "Export et fichiers associés supprimés",
            Some(id),
        )
        .await;
        Ok(())
    }

    /// Liste les fichiers sans dupliquer leur catalogue dans une base séparée.
    pub async fn list_artifacts(&self) -> AppResult<Vec<Artifact>> {
        self.remove_missing_exports().await?;
        self.store.list_artifacts().await
    }

    /// Retire l'historique d'un export terminé dont le fichier final a été supprimé.
    async fn remove_missing_exports(&self) -> AppResult<()> {
        for job in self.store.list().await? {
            if job.status != JobStatus::Completed || !matches!(&job.kind, JobKind::Export(_)) {
                continue;
            }
            let Some(output) = job.artifacts.iter().find(|artifact| {
                matches!(artifact.kind, ArtifactKind::Stl | ArtifactKind::Blender)
            }) else {
                continue;
            };
            match tokio::fs::metadata(&output.path).await {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    self.delete_export(&job.id).await?;
                }
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    /// Liste les profils d'export nommés disponibles pour le wizard et l'API.
    pub async fn list_export_presets(&self) -> AppResult<Vec<ExportPreset>> {
        self.presets.list().await
    }

    /// Sauvegarde un profil d'export après validation métier.
    pub async fn create_export_preset(&self, draft: ExportPresetDraft) -> AppResult<ExportPreset> {
        self.presets.create(draft).await
    }

    /// Supprime un profil sans toucher aux exports déjà générés.
    pub async fn delete_export_preset(&self, id: &str) -> AppResult<()> {
        self.presets.delete(id).await
    }

    /// Retrouve les métadonnées et le chemin privé d'un fichier.
    pub async fn get_artifact(&self, id: &str) -> AppResult<Artifact> {
        self.store.find_artifact(id).await
    }

    /// Supprime un fichier et invalide chaque résultat final qui le référençait.
    pub async fn delete_artifact(&self, id: &str) -> AppResult<()> {
        let artifact = self.store.find_artifact(id).await?;
        match tokio::fs::remove_file(&artifact.path).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        for mut job in self.store.list().await? {
            let previous = job.artifacts.len();
            job.artifacts
                .retain(|candidate| candidate.path != artifact.path);
            if job.artifacts.len() != previous {
                self.cache.invalidate(&job.cache_key).await?;
                self.store.save(&job).await?;
            }
        }
        Ok(())
    }

    /// Attend la fin d'un job local sans bloquer le moteur asynchrone.
    pub async fn wait_for_job(&self, id: &str) -> AppResult<Job> {
        loop {
            let job = self.get_job(id).await?;
            if job.status.is_terminal() {
                return Ok(job);
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    /// Renvoie une copie cohérente de la configuration active.
    pub async fn configuration(&self) -> AppConfig {
        self.config.lock().await.clone()
    }

    /// Sauvegarde les valeurs modifiables ; le nombre de workers prendra effet au redémarrage.
    pub async fn update_configuration(&self, mut next: AppConfig) -> AppResult<AppConfig> {
        next.validate()?;
        let mut current = self.config.lock().await;
        next.source_path = current.source_path.clone();
        let warmup_changed = current.cache_warmup != next.cache_warmup;
        next.save()?;
        *current = next.clone();
        drop(current);
        if warmup_changed {
            self.enqueue_existing_cache_warmups().await?;
        }
        Ok(next)
    }

    /// Soumet un contrat déjà normalisé, avec cache et dédoublonnage des calculs en cours.
    async fn submit(&self, kind: JobKind) -> AppResult<Job> {
        let force = match &kind {
            JobKind::Download(request) => request.force,
            JobKind::Export(request) => request.force,
        };
        let cache_key = ResultCache::key(&kind)?;
        if !force {
            if let Some(job_id) = self.active_keys.lock().await.get(&cache_key).cloned() {
                return self.store.get(&job_id).await;
            }
            if let Some(entry) = self.cache.get(&cache_key).await? {
                let mut job = Job::queued(kind, cache_key);
                job.status = JobStatus::Completed;
                job.progress_percent = 100;
                job.message = "Résultat réutilisé depuis le cache".to_owned();
                job.cache_hit = true;
                job.finished_at = Some(Utc::now());
                job.artifacts = entry.artifacts;
                self.store.save(&job).await?;
                self.log(
                    ServerLogLevel::Info,
                    "cache_hit",
                    "Résultat final réutilisé sans relancer de calcul",
                    Some(&job.id),
                )
                .await;
                if matches!(&job.kind, JobKind::Download(_)) {
                    self.prune_covered_downloads(&job).await;
                    self.enqueue_cache_warmup(&job).await?;
                }
                return Ok(job);
            }
        }
        let job = Job::queued(kind, cache_key.clone());
        // Le second contrôle est effectué sous le verrou afin que deux requêtes
        // simultanées ne puissent jamais démarrer le même calcul coûteux.
        let mut active = self.active_keys.lock().await;
        if !force && let Some(job_id) = active.get(&cache_key).cloned() {
            drop(active);
            return self.store.get(&job_id).await;
        }
        self.store.save(&job).await?;
        active.insert(cache_key, job.id.clone());
        drop(active);
        self.cancellations
            .lock()
            .await
            .insert(job.id.clone(), CancellationToken::new());
        self.enqueue(&job).await?;
        self.log(
            ServerLogLevel::Info,
            "job_queued",
            "Travail ajouté à sa file de workers",
            Some(&job.id),
        )
        .await;
        Ok(job)
    }

    /// Envoie un job vers la file dédiée à son type.
    async fn enqueue(&self, job: &Job) -> AppResult<()> {
        let sender = match job.kind {
            JobKind::Download(_) => &self.download_sender,
            JobKind::Export(_) => &self.export_sender,
        };
        sender
            .send(job.id.clone())
            .await
            .map_err(|_| AppError::storage("la file de workers est arrêtée"))
    }

    /// Reprend les jobs qui étaient en attente ou en cours lors du dernier arrêt.
    async fn resume_interrupted_jobs(&self) -> AppResult<()> {
        for mut job in self.store.list().await? {
            if matches!(job.status, JobStatus::Queued | JobStatus::Running) {
                job.status = JobStatus::Queued;
                job.progress_percent = 0;
                job.message = "Repris après le redémarrage".to_owned();
                job.started_at = None;
                self.store.save(&job).await?;
                self.active_keys
                    .lock()
                    .await
                    .insert(job.cache_key.clone(), job.id.clone());
                self.cancellations
                    .lock()
                    .await
                    .insert(job.id.clone(), CancellationToken::new());
                self.enqueue(&job).await?;
            }
        }
        Ok(())
    }

    /// Exécute un job reçu par un dispatcher et persiste toutes ses transitions.
    pub(crate) async fn process_job(&self, id: &str) -> AppResult<()> {
        let mut job = self.store.get(id).await?;
        if job.status == JobStatus::Cancelled {
            self.release_job(id).await;
            return Ok(());
        }
        let cancellation = self
            .cancellations
            .lock()
            .await
            .get(id)
            .cloned()
            .unwrap_or_default();
        job.status = JobStatus::Running;
        job.progress_percent = 5;
        job.message = "Traitement en cours".to_owned();
        job.started_at = Some(Utc::now());
        self.store.save(&job).await?;
        self.log(
            ServerLogLevel::Info,
            "job_started",
            "Un worker a commencé le traitement",
            Some(&job.id),
        )
        .await;
        let (progress_sender, mut progress_receiver) = mpsc::unbounded_channel();
        let execution_job = job.clone();
        let execution = self
            .backend
            .execute(&execution_job, cancellation.clone(), progress_sender);
        tokio::pin!(execution);
        let result = loop {
            tokio::select! {
                result = &mut execution => break result,
                Some(progress) = progress_receiver.recv() => {
                    job.progress_percent = progress.percent.clamp(5, 95);
                    job.message = progress.message;
                    self.store.save(&job).await?;
                    self.log(
                        ServerLogLevel::Info,
                        "job_progress",
                        format!("{} · {} %", job.message, job.progress_percent),
                        Some(&job.id),
                    )
                    .await;
                }
            }
        };
        if cancellation.is_cancelled() {
            job.status = JobStatus::Cancelled;
            job.message = "Travail annulé".to_owned();
            self.log(
                ServerLogLevel::Info,
                "job_cancelled",
                "Le processus du worker a été arrêté",
                Some(&job.id),
            )
            .await;
        } else {
            match result {
                Ok(artifacts) => {
                    let warning = worker_warning(&artifacts).await;
                    let completed_with_warning = warning.is_some();
                    job.status = JobStatus::Completed;
                    job.progress_percent = 100;
                    job.message = warning.unwrap_or_else(|| "Traitement terminé".to_owned());
                    job.artifacts = artifacts;
                    self.cache.put(&job.cache_key, &job.artifacts).await?;
                    self.log(
                        if completed_with_warning {
                            ServerLogLevel::Warning
                        } else {
                            ServerLogLevel::Info
                        },
                        if completed_with_warning {
                            "job_completed_with_warning"
                        } else {
                            "job_completed"
                        },
                        job.message.clone(),
                        Some(&job.id),
                    )
                    .await;
                }
                Err(error) => {
                    job.status = JobStatus::Failed;
                    job.message = error.to_string();
                    self.log(
                        ServerLogLevel::Error,
                        "job_failed",
                        job.message.clone(),
                        Some(&job.id),
                    )
                    .await;
                }
            }
        }
        job.finished_at = Some(Utc::now());
        self.store.save(&job).await?;
        self.release_job(id).await;
        if job.status == JobStatus::Completed && matches!(&job.kind, JobKind::Download(_)) {
            self.invalidate_export_result_caches().await?;
            self.prune_covered_downloads(&job).await;
            self.enqueue_cache_warmup(&job).await?;
        }
        Ok(())
    }

    /// Les données téléchargées sont une entrée des exports, même si leur requête n'a pas changé.
    async fn invalidate_export_result_caches(&self) -> AppResult<()> {
        for job in self.store.list().await? {
            if job.status == JobStatus::Completed && matches!(&job.kind, JobKind::Export(_)) {
                self.cache.invalidate(&job.cache_key).await?;
            }
        }
        Ok(())
    }

    /// Supprime les anciennes zones entièrement couvertes avec au moins les mêmes couches.
    async fn prune_covered_downloads(&self, covering_job: &Job) {
        let JobKind::Download(covering) = &covering_job.kind else {
            return;
        };
        let jobs = match self.store.list().await {
            Ok(jobs) => jobs,
            Err(error) => {
                self.log(
                    ServerLogLevel::Error,
                    "zone_prune_failed",
                    error.to_string(),
                    Some(&covering_job.id),
                )
                .await;
                return;
            }
        };
        let candidates: Vec<_> = jobs
            .into_iter()
            .filter(|job| job.id != covering_job.id && job.status == JobStatus::Completed)
            .filter(|job| {
                let JobKind::Download(candidate) = &job.kind else {
                    return false;
                };
                area_contains(&covering.area, &candidate.area)
                    && layers_include(&covering.layers, &candidate.layers)
            })
            .collect();
        for candidate in candidates {
            match self.remove_zone_job(&candidate).await {
                Ok(()) => {
                    self.log(
                        ServerLogLevel::Info,
                        "zone_pruned",
                        "Ancienne zone redondante retirée après extension",
                        Some(&candidate.id),
                    )
                    .await;
                }
                Err(error) => {
                    self.log(
                        ServerLogLevel::Error,
                        "zone_prune_failed",
                        error.to_string(),
                        Some(&candidate.id),
                    )
                    .await;
                }
            }
        }
    }

    /// Supprime les métadonnées d'une zone et uniquement ses fichiers devenus sans référence.
    async fn remove_zone_job(&self, job: &Job) -> AppResult<()> {
        let other_jobs: Vec<_> = self
            .store
            .list()
            .await?
            .into_iter()
            .filter(|candidate| candidate.id != job.id)
            .collect();
        let referenced_paths: HashSet<_> = other_jobs
            .iter()
            .flat_map(|candidate| candidate.artifacts.iter())
            .map(|artifact| artifact.path.as_str())
            .collect();
        self.delete_download_snapshots(job, &other_jobs).await?;
        for artifact in &job.artifacts {
            if referenced_paths.contains(artifact.path.as_str()) {
                continue;
            }
            match tokio::fs::remove_file(&artifact.path).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        let cache_is_still_used = other_jobs.iter().any(|candidate| {
            candidate.status == JobStatus::Completed && candidate.cache_key == job.cache_key
        });
        if !cache_is_still_used {
            self.cache.invalidate(&job.cache_key).await?;
        }
        self.store.delete(&job.id).await?;
        self.release_job(&job.id).await;
        Ok(())
    }

    /// Efface les extractions WFS propres à la zone tout en conservant les caches de tuiles partagés.
    async fn delete_download_snapshots(&self, job: &Job, other_jobs: &[Job]) -> AppResult<()> {
        let targets = download_snapshot_directories(job).await?;
        if targets.is_empty() {
            return Ok(());
        }
        let mut referenced = HashSet::new();
        for other in other_jobs {
            referenced.extend(download_snapshot_directories(other).await?);
        }
        let data_root = self.config.lock().await.data_root();
        for target in targets {
            if referenced.contains(&target) || !is_safe_snapshot_directory(&target, &data_root) {
                continue;
            }
            match tokio::fs::remove_dir_all(&target).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    /// Écrit un événement sans rendre le traitement dépendant du journal de diagnostic.
    async fn log(
        &self,
        level: ServerLogLevel,
        action: &str,
        message: impl Into<String>,
        job_id: Option<&str>,
    ) {
        let _ = self.logs.append(level, action, message, job_id).await;
    }

    /// Retire les verrous logiques d'un job terminal.
    pub(crate) async fn release_job(&self, id: &str) {
        self.cancellations.lock().await.remove(id);
        self.active_keys
            .lock()
            .await
            .retain(|_, job_id| job_id != id);
    }

    /// Vérifie qu'un chemin servi reste un fichier régulier existant.
    pub async fn artifact_path(&self, id: &str) -> AppResult<(Artifact, std::fs::File)> {
        let artifact = self.get_artifact(id).await?;
        let file = std::fs::File::open(Path::new(&artifact.path)).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                AppError::not_found(format!("contenu du fichier {id}"))
            } else {
                AppError::from(error)
            }
        })?;
        Ok((artifact, file))
    }
}

/// Vérifie qu'une emprise carrée contient entièrement une autre emprise du même repère.
fn area_contains(outer: &Area, inner: &Area) -> bool {
    if !outer.crs().eq_ignore_ascii_case(inner.crs()) {
        return false;
    }
    let (delta_y, delta_x) = if outer.is_wgs84() {
        let average_latitude = ((outer.center[0] + inner.center[0]) / 2.0).to_radians();
        (
            (outer.center[0] - inner.center[0]).abs() * 111_320.0,
            (outer.center[1] - inner.center[1]).abs()
                * 111_320.0
                * average_latitude.cos().abs().max(0.01),
        )
    } else {
        (
            (outer.center[1] - inner.center[1]).abs(),
            (outer.center[0] - inner.center[0]).abs(),
        )
    };
    let outer_half = outer.size_m / 2.0;
    let inner_half = inner.size_m / 2.0;
    delta_x + inner_half <= outer_half + 0.01 && delta_y + inner_half <= outer_half + 0.01
}

/// Vérifie que chaque couche de l'ancienne zone existe dans la nouvelle.
fn layers_include(outer: &[DataLayer], inner: &[DataLayer]) -> bool {
    inner.iter().all(|layer| outer.contains(layer))
}

/// Retrouve les dossiers d'extraction propres à une zone dans son journal de worker.
async fn download_snapshot_directories(job: &Job) -> AppResult<HashSet<PathBuf>> {
    if !matches!(&job.kind, JobKind::Download(_)) {
        return Ok(HashSet::new());
    }
    let Some(log) = job
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == ArtifactKind::WorkerLog)
    else {
        return Ok(HashSet::new());
    };
    let content = match tokio::fs::read_to_string(&log.path).await {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(HashSet::new());
        }
        Err(error) => return Err(error.into()),
    };
    Ok(content
        .lines()
        .filter_map(|line| line.trim().strip_prefix("Données"))
        .filter_map(|line| line.split_once(':').map(|(_, path)| path.trim()))
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .collect())
}

/// Limite la suppression aux dossiers de bbox recréables, jamais à une racine de cache partagée.
fn is_safe_snapshot_directory(path: &Path, data_root: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(data_root) else {
        return false;
    };
    let parts: Vec<_> = relative.components().collect();
    let Some(dataset) = parts.first().map(|part| part.as_os_str().to_string_lossy()) else {
        return false;
    };
    match dataset.as_ref() {
        "BD_TOPO" | "CADASTRE" => parts.len() == 2,
        "BD_TOPAGE" => parts.len() == 3,
        _ => false,
    }
}

/// Supprime au redémarrage les fichiers `.part` qu'aucun processus ne peut encore utiliser.
async fn cleanup_abandoned_partial_files(storage_root: PathBuf) -> AppResult<usize> {
    tokio::task::spawn_blocking(move || remove_partial_files(&storage_root))
        .await
        .map_err(|error| AppError::storage(format!("nettoyage de démarrage interrompu : {error}")))?
        .map_err(AppError::from)
}

/// Parcourt uniquement la racine de stockage du nouveau projet sans suivre les liens symboliques.
fn remove_partial_files(directory: &Path) -> std::io::Result<usize> {
    if !directory.is_dir() {
        return Ok(0);
    }
    let mut removed = 0;
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            removed += remove_partial_files(&path)?;
        } else if file_type.is_file()
            && path.extension().and_then(|extension| extension.to_str()) == Some("part")
        {
            std::fs::remove_file(path)?;
            removed += 1;
        }
    }
    Ok(removed)
}

/// Efface les calculs intermédiaires de jobs interrompus et les dossiers vides terminés.
async fn cleanup_abandoned_work_directories(
    config: &AppConfig,
    store: &JobStore,
) -> AppResult<usize> {
    let mut removed = 0;
    for job in store.list().await? {
        if !matches!(job.kind, JobKind::Export(_)) {
            continue;
        }
        let work = config.exports_root().join(&job.id).join("work");
        if job.status == JobStatus::Completed {
            match tokio::fs::remove_dir(&work).await {
                Ok(()) => removed += 1,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                    ) => {}
                Err(error) => return Err(error.into()),
            }
        } else {
            match tokio::fs::remove_dir_all(&work).await {
                Ok(()) => removed += 1,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(removed)
}

/// Traduit les avertissements structurés du worker en message visible sur le dashboard.
async fn worker_warning(artifacts: &[Artifact]) -> Option<String> {
    let log = artifacts
        .iter()
        .find(|artifact| artifact.kind == ArtifactKind::WorkerLog)?;
    let content = tokio::fs::read_to_string(&log.path).await.ok()?;
    if content.contains("[ROUTES RÉPARÉES]") {
        return Some(
            "Export terminé avec avertissement : l'union des routes était invalide, leurs rubans ont été reconstruits et conservés dans le STL.".to_owned(),
        );
    }
    if content.contains("[TUILES MANQUANTES]") {
        return Some(
            "Téléchargement terminé avec avertissement : quelques tuiles orthophoto indisponibles sont signalées dans le journal ; les autres données ont été conservées.".to_owned(),
        );
    }
    content.contains("[AVERTISSEMENT]").then(|| {
        "Traitement terminé avec avertissement ; consultez le journal du worker.".to_owned()
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use async_trait::async_trait;
    use tempfile::TempDir;

    use super::*;
    use crate::{
        config::{CacheWarmupConfig, ServerConfig, StorageConfig, WorkerConfig},
        domain::{
            AdvancedExport, Area, ArtifactKind, DataLayer, ExportFormat, ExportQuality,
            ExportRequest,
        },
    };

    /// Backend de test qui matérialise un petit fichier sans processus externe.
    struct TestBackend {
        root: PathBuf,
    }

    #[async_trait]
    impl ProcessingBackend for TestBackend {
        /// Produit un artefact déterministe pour tester l'orchestration et son cache.
        async fn execute(
            &self,
            job: &Job,
            _cancellation: CancellationToken,
            progress: crate::backend::ProgressSender,
        ) -> AppResult<Vec<Artifact>> {
            let _ = progress.send(crate::backend::ProcessingProgress {
                percent: 50,
                message: "Traitement de test".to_owned(),
            });
            let path = self.root.join(format!("{}.txt", job.id));
            tokio::fs::write(&path, b"ok").await?;
            Ok(vec![Artifact {
                id: uuid::Uuid::new_v4().to_string(),
                job_id: job.id.clone(),
                name: "result.txt".to_owned(),
                kind: ArtifactKind::DownloadManifest,
                media_type: "text/plain".to_owned(),
                size_bytes: 2,
                created_at: Utc::now(),
                path: path.to_string_lossy().into_owned(),
            }])
        }
    }

    /// Construit un service isolé pour chaque test.
    async fn test_service(directory: &TempDir) -> Arc<AppService> {
        let config = AppConfig {
            server: ServerConfig {
                bind: "127.0.0.1:0".parse().expect("adresse valide"),
            },
            storage: StorageConfig {
                root: directory.path().to_path_buf(),
            },
            workers: WorkerConfig {
                downloads: 1,
                exports: 1,
            },
            cache_warmup: CacheWarmupConfig::default(),
            render: crate::config::RenderConfig::default(),
            source_path: directory.path().join("geo.toml"),
        };
        let backend = Arc::new(TestBackend {
            root: directory.path().to_path_buf(),
        });
        AppService::start_with_backend(Arc::new(Mutex::new(config)), backend)
            .await
            .expect("service")
    }

    /// Vérifie qu'une seconde demande identique évite réellement le traitement.
    #[tokio::test]
    async fn completed_result_is_reused() {
        // Le dossier reste sous le projet afin que le test respecte aussi les
        // environnements dont les répertoires temporaires système sont protégés.
        let directory =
            tempfile::tempdir_in(std::env::current_dir().expect("répertoire courant disponible"))
                .expect("dossier temporaire");
        let service = test_service(&directory).await;
        let request = DownloadRequest {
            name: None,
            area: Area {
                center: [48.0, 4.0],
                size_m: 1000.0,
                advanced: None,
            },
            layers: vec![DataLayer::Lidar],
            force: false,
        };
        let first = service
            .submit_download(request.clone())
            .await
            .expect("soumission");
        let first = service.wait_for_job(&first.id).await.expect("fin");
        assert_eq!(first.status, JobStatus::Completed);
        let mut renamed_request = request;
        renamed_request.name = Some("Même emprise, autre nom".to_owned());
        let second = service
            .submit_download(renamed_request)
            .await
            .expect("cache");
        assert!(second.cache_hit);
        assert_eq!(second.status, JobStatus::Completed);
        let JobKind::Download(second_request) = &second.kind else {
            panic!("téléchargement attendu");
        };
        assert_eq!(
            second_request.name.as_deref(),
            Some("Même emprise, autre nom")
        );
        assert_eq!(service.list_jobs().await.expect("jobs").len(), 1);
    }

    /// La consultation de l'historique purge un export dont le STL a été supprimé hors API.
    #[tokio::test]
    async fn missing_export_output_removes_its_job_and_artifacts() {
        let directory =
            tempfile::tempdir_in(std::env::current_dir().expect("répertoire courant disponible"))
                .expect("dossier temporaire");
        let service = test_service(&directory).await;
        let job_id = uuid::Uuid::new_v4().to_string();
        let missing_output = directory.path().join("efface.stl");
        let job = Job {
            id: job_id.clone(),
            kind: JobKind::Export(Box::new(ExportRequest {
                area: Area {
                    center: [48.0, 4.0],
                    size_m: 1_000.0,
                    advanced: None,
                },
                format: ExportFormat::Stl,
                layers: vec![DataLayer::Lidar],
                quality: ExportQuality::Standard,
                output_name: Some("efface.stl".to_owned()),
                advanced: AdvancedExport::default(),
                force: false,
            })),
            status: JobStatus::Completed,
            progress_percent: 100,
            message: "Export terminé".to_owned(),
            cache_key: "missing-output-test".to_owned(),
            cache_hit: false,
            created_at: Utc::now(),
            started_at: Some(Utc::now()),
            finished_at: Some(Utc::now()),
            artifacts: vec![Artifact {
                id: uuid::Uuid::new_v4().to_string(),
                job_id: job_id.clone(),
                name: "efface.stl".to_owned(),
                kind: ArtifactKind::Stl,
                media_type: "model/stl".to_owned(),
                size_bytes: 0,
                created_at: Utc::now(),
                path: missing_output.to_string_lossy().into_owned(),
            }],
        };
        service.store.save(&job).await.expect("job de test");

        assert!(service.list_jobs().await.expect("historique").is_empty());
        assert!(service.store.get(&job_id).await.is_err());
    }

    /// Un nouveau téléchargement invalide les résultats d’export calculés sur d’anciennes sources.
    #[tokio::test]
    async fn completed_download_invalidates_export_result_index() {
        let directory =
            tempfile::tempdir_in(std::env::current_dir().expect("répertoire courant disponible"))
                .expect("dossier temporaire");
        let service = test_service(&directory).await;
        let kind = JobKind::Export(Box::new(ExportRequest {
            area: Area {
                center: [48.0, 4.0],
                size_m: 1_000.0,
                advanced: None,
            },
            format: ExportFormat::Stl,
            layers: vec![DataLayer::Lidar],
            quality: ExportQuality::Standard,
            output_name: Some("ancien.stl".to_owned()),
            advanced: AdvancedExport::default(),
            force: false,
        }));
        let cache_key = ResultCache::key(&kind).expect("clé d’export");
        let mut job = Job::queued(kind, cache_key.clone());
        job.status = JobStatus::Completed;
        service.store.save(&job).await.expect("job export");
        service
            .cache
            .put(&cache_key, &[])
            .await
            .expect("index d’export");
        assert!(
            service
                .cache
                .get(&cache_key)
                .await
                .expect("cache")
                .is_some()
        );

        service
            .invalidate_export_result_caches()
            .await
            .expect("invalidation");

        assert!(
            service
                .cache
                .get(&cache_key)
                .await
                .expect("cache")
                .is_none()
        );
    }

    /// Vérifie qu'une zone plus vaste ne masque que les emprises réellement contenues.
    #[test]
    fn containment_accounts_for_centres_and_layers() {
        let outer = Area {
            center: [48.0, 4.0],
            size_m: 2_000.0,
            advanced: None,
        };
        let inner = Area {
            center: [48.002, 4.002],
            size_m: 1_000.0,
            advanced: None,
        };
        assert!(area_contains(&outer, &inner));
        assert!(layers_include(
            &[DataLayer::Lidar, DataLayer::Buildings],
            &[DataLayer::Lidar]
        ));
        assert!(!layers_include(
            &[DataLayer::Lidar],
            &[DataLayer::Lidar, DataLayer::Buildings]
        ));
    }

    /// Refuse toute racine large et accepte uniquement les dossiers de bbox connus.
    #[test]
    fn snapshot_cleanup_is_restricted_to_regenerable_directories() {
        let root = Path::new("C:/cache/data");
        assert!(is_safe_snapshot_directory(
            Path::new("C:/cache/data/BD_TOPO/1_2_3_4"),
            root
        ));
        assert!(is_safe_snapshot_directory(
            Path::new("C:/cache/data/BD_TOPAGE/2026/1_2_3_4"),
            root
        ));
        assert!(!is_safe_snapshot_directory(
            Path::new("C:/cache/data/LIDAR_HD/tiles"),
            root
        ));
        assert!(!is_safe_snapshot_directory(root, root));
    }
}
