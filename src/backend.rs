//! Contexte : frontière unique entre l'orchestration et les calculs géographiques.
//! Usage : le backend actuel pilote les exécutables Rust existants pendant leur migration progressive.

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Instant,
};

use async_trait::async_trait;
use chrono::{Local, SecondsFormat, Utc};
use clap::Parser;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::Command,
    sync::{Mutex, mpsc},
    time::{Duration, sleep},
};
use tokio_util::sync::CancellationToken;

use crate::{
    config::AppConfig,
    domain::{
        AdvancedExport, Artifact, ArtifactKind, DataLayer, DownloadRequest, ExportFormat,
        ExportQuality, ExportRequest, ForestMaterialMode, Job, JobKind, OrthophotoStorage,
        TextureMode, TreeShape,
    },
    error::{AppError, AppResult},
    road_repair,
};

/// Limite historique conservée comme plancher pour les scènes courantes.
const TERRAIN_DEFAULT_MAX_VERTICES: usize = 250_000;
/// Plafond de sécurité qui évite qu'une très grande zone épuise la mémoire de Blender.
const TERRAIN_HARD_MAX_VERTICES: usize = 750_000;
/// Nombre de scènes géométriques intermédiaires conservées pour les exports suivants.
const BLENDER_BASE_CACHE_LIMIT: usize = 12;
/// Les scènes finales sont plus lourdes ; quelques variantes visuelles suffisent.
const BLENDER_FINAL_CACHE_LIMIT: usize = 6;
/// Une requête cadastrale plus large surcharge régulièrement le WFS IGN et finit en 504.
const CADASTRE_WFS_CHUNK_SIZE_M: f64 = 5_000.0;

/// Contrat minimal que les workers connaissent du moteur de calcul.
#[async_trait]
pub trait ProcessingBackend: Send + Sync {
    /// Exécute le travail et renvoie uniquement des fichiers complètement écrits.
    async fn execute(
        &self,
        job: &Job,
        cancellation: CancellationToken,
        progress: ProgressSender,
    ) -> AppResult<Vec<Artifact>>;

    /// Préchauffe les grilles STL partagées après un téléchargement LiDAR.
    async fn prewarm_stl_surfaces(
        &self,
        _request: &DownloadRequest,
        _steps_m: &[f64],
        _cancellation: CancellationToken,
    ) -> AppResult<()> {
        Ok(())
    }

    /// Préchauffe des variantes Blender à partir d'une scène de base réutilisable.
    async fn prewarm_blender_atlases(
        &self,
        _request: &DownloadRequest,
        _resolutions_px: &[u32],
        _cancellation: CancellationToken,
    ) -> AppResult<()> {
        Ok(())
    }
}

/// Étape courte envoyée par un moteur au service pendant un traitement long.
#[derive(Clone, Debug)]
pub struct ProcessingProgress {
    pub percent: u8,
    pub message: String,
}

/// Canal non bloquant utilisé par les moteurs pour publier leur étape courante.
pub type ProgressSender = mpsc::UnboundedSender<ProcessingProgress>;

/// Envoie une étape au dashboard sans rendre le calcul dépendant de son affichage.
fn report_progress(progress: &ProgressSender, percent: u8, message: impl Into<String>) {
    let _ = progress.send(ProcessingProgress {
        percent,
        message: message.into(),
    });
}

/// Appelle directement le moteur de téléchargement avec les options du job courant.
async fn run_engine_download(
    dataset: &str,
    request: &DownloadRequest,
    data_root: &Path,
    cancellation: CancellationToken,
    progress: &ProgressSender,
    start_percent: u8,
    end_percent: u8,
) -> AppResult<String> {
    let program = std::env::current_exe().map_err(AppError::from)?;
    let mut arguments = vec![
        "--engine-worker".to_owned(),
        "download".to_owned(),
        dataset.to_owned(),
        "--center".to_owned(),
        request.area.center[0].to_string(),
        request.area.center[1].to_string(),
        "--crs".to_owned(),
        legacy_crs(&request.area).to_owned(),
        "--side".to_owned(),
        request.area.size_m.to_string(),
        "--root".to_owned(),
        data_root.to_string_lossy().into_owned(),
        "--concurrency".to_owned(),
        "6".to_owned(),
    ];
    if request.force {
        arguments.push("--force".to_owned());
    }
    let mut line_progress = DownloadLineProgress::new(dataset, start_percent, end_percent);
    run_command_streaming(&program, &arguments, cancellation, |line| {
        line_progress.observe(line, progress);
    })
    .await
}

/// Suit les tuiles réussies afin d’alimenter la progression des téléchargements.
struct DownloadLineProgress {
    dataset: String,
    total: Option<usize>,
    completed: usize,
    last_reported: usize,
    start_percent: u8,
    end_percent: u8,
}

impl DownloadLineProgress {
    fn new(dataset: &str, start_percent: u8, end_percent: u8) -> Self {
        Self {
            dataset: dataset.to_owned(),
            total: None,
            completed: 0,
            last_reported: 0,
            start_percent,
            end_percent,
        }
    }

    fn observe(&mut self, line: &str, progress: &ProgressSender) {
        if line.contains("À télécharger") {
            self.total = line
                .split(|character: char| !character.is_ascii_digit())
                .find_map(|part| part.parse::<usize>().ok());
            return;
        }
        if !line.trim_start().starts_with("[OK]") {
            return;
        }
        self.completed += 1;
        let total = self.total.unwrap_or(self.completed).max(1);
        let interval = total.div_ceil(50).max(1);
        if self.completed == 1
            || self.completed.saturating_sub(self.last_reported) >= interval
            || self.total == Some(self.completed)
        {
            self.last_reported = self.completed;
            let range = usize::from(self.end_percent.saturating_sub(self.start_percent));
            let percent = self
                .start_percent
                .saturating_add(((range * self.completed.min(total)) / total) as u8);
            report_progress(
                progress,
                percent,
                format!(
                    "{} · tuile {}/{}",
                    self.dataset,
                    self.completed.min(total),
                    total
                ),
            );
        }
    }
}

/// Exécute l'un des moteurs d'export intégrés avec les options déjà normalisées.
async fn run_engine_export(
    format: ExportFormat,
    arguments: &[String],
    cancellation: CancellationToken,
) -> AppResult<String> {
    let mode = match format {
        ExportFormat::Blender => "blender",
        ExportFormat::Stl => "stl",
        ExportFormat::StlLayers => "stl-layers",
    };
    let program = std::env::current_exe().map_err(AppError::from)?;
    let mut worker_arguments = vec!["--engine-worker".to_owned(), mode.to_owned()];
    worker_arguments.extend(arguments.iter().cloned());
    run_command(&program, &worker_arguments, cancellation).await
}

/// Prépare le cache LiDAR continu utilisé par la canopée Blender.
async fn run_engine_stl_cache(
    arguments: &[String],
    cancellation: CancellationToken,
) -> AppResult<PathBuf> {
    let program = std::env::current_exe().map_err(AppError::from)?;
    let mut worker_arguments = vec!["--engine-worker".to_owned(), "stl-cache".to_owned()];
    worker_arguments.extend(arguments.iter().cloned());
    let log = run_command(&program, &worker_arguments, cancellation).await?;
    log.lines()
        .find_map(|line| line.strip_prefix("[GEO_ENGINE_CACHE_PATH] "))
        .map(PathBuf::from)
        .ok_or_else(|| AppError::engine("le moteur n'a pas indiqué le cache LiDAR de la forêt"))
}

/// Calcule seulement une surface STL réutilisable, sans écrire de maquette finale.
async fn run_engine_stl_surface_warm(
    config_path: &Path,
    cancellation: CancellationToken,
) -> AppResult<String> {
    let program = std::env::current_exe().map_err(AppError::from)?;
    let arguments = vec![
        "--engine-worker".to_owned(),
        "stl-warm".to_owned(),
        "--config".to_owned(),
        config_path.to_string_lossy().into_owned(),
        "--quality".to_owned(),
        "balanced".to_owned(),
    ];
    run_command(&program, &arguments, cancellation).await
}

/// Donne aux exécutables CLI le mode worker privé utilisé par le backend.
pub fn engine_worker_arguments() -> Option<Vec<OsString>> {
    let mut arguments = std::env::args_os().skip(1);
    if arguments.next()?.as_os_str() == "--engine-worker" {
        Some(arguments.collect())
    } else {
        None
    }
}

#[derive(Debug, Parser)]
#[command(name = "geo-stl-cache-warm", disable_help_flag = true)]
struct StlSurfaceWarmArguments {
    #[arg(long)]
    config: PathBuf,
    #[arg(long, value_enum, default_value = "balanced")]
    quality: geo_engine::stl::StlQuality,
}

/// Exécute une opération moteur dans un enfant annulable du binaire appelant.
pub async fn run_engine_worker(arguments: Vec<OsString>) -> anyhow::Result<()> {
    let mut arguments = arguments.into_iter();
    let mode = arguments
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| anyhow::anyhow!("mode moteur absent"))?;
    match mode.as_str() {
        "download" => {
            geo_engine::engine_api::download_cli(arguments.collect()).await?;
            Ok(())
        }
        "blender" => {
            let values = std::iter::once(OsString::from("geo-export")).chain(arguments);
            let parsed = geo_engine::export::models::ExportArguments::try_parse_from(values)?;
            geo_engine::export::run(parsed)
        }
        "stl" => {
            let values = std::iter::once(OsString::from("geo-stl")).chain(arguments);
            let parsed = geo_engine::stl::StlArguments::try_parse_from(values)?;
            geo_engine::stl::run(parsed)
        }
        "stl-layers" => {
            let values = std::iter::once(OsString::from("geo-stl-layers")).chain(arguments);
            let parsed = geo_engine::stl::LayeredStlArguments::try_parse_from(values)?;
            geo_engine::stl::run_layered(parsed)
        }
        "stl-cache" => {
            let values = std::iter::once(OsString::from("geo-stl")).chain(arguments);
            let parsed = geo_engine::stl::StlArguments::try_parse_from(values)?;
            let path = geo_engine::stl::run_with_cache(parsed)?;
            println!("[GEO_ENGINE_CACHE_PATH] {}", path.display());
            Ok(())
        }
        "stl-warm" => {
            let values = std::iter::once(OsString::from("geo-stl-cache-warm")).chain(arguments);
            let parsed = StlSurfaceWarmArguments::try_parse_from(values)?;
            let path = geo_engine::stl::warm_surface_cache(&parsed.config, parsed.quality)?;
            println!("[GEO_ENGINE_WARM_CACHE] {}", path.display());
            Ok(())
        }
        _ => anyhow::bail!("mode moteur inconnu : {mode}"),
    }
}

/// Backend relié aux moteurs Rust intégrés au workspace.
#[derive(Debug)]
pub struct EngineBackend {
    config: Arc<Mutex<AppConfig>>,
}

impl EngineBackend {
    /// Conserve la même configuration partagée que l'API.
    pub fn new(config: Arc<Mutex<AppConfig>>) -> Self {
        Self { config }
    }

    /// Lance les produits nécessaires, une seule fois quand deux couches partagent BD TOPO.
    async fn download(
        &self,
        job: &Job,
        request: &DownloadRequest,
        cancellation: CancellationToken,
        progress: ProgressSender,
    ) -> AppResult<Vec<Artifact>> {
        let config = self.config.lock().await.clone();
        let job_root = config.storage.root.join("jobs").join(&job.id);
        tokio::fs::create_dir_all(&job_root).await?;
        tokio::fs::create_dir_all(config.data_root()).await?;
        let datasets: BTreeSet<_> = request
            .layers
            .iter()
            .map(|layer| layer.legacy_dataset())
            .collect();
        let log_path = job_root.join("worker.log");
        let dataset_count = datasets.len().max(1);
        let mut combined_log = String::new();
        let mut warnings = Vec::new();
        for (index, dataset) in datasets.into_iter().enumerate() {
            report_progress(
                &progress,
                10 + ((index * 70) / dataset_count) as u8,
                format!(
                    "Téléchargement de {dataset} ({}/{dataset_count})",
                    index + 1
                ),
            );
            let start_percent = 10 + ((index * 70) / dataset_count) as u8;
            let end_percent = 10 + (((index + 1) * 70) / dataset_count) as u8;
            if dataset == "CADASTRE" && request.area.size_m > CADASTRE_WFS_CHUNK_SIZE_M {
                match download_cadastre_in_chunks(
                    config.data_root(),
                    request,
                    cancellation.clone(),
                    &progress,
                    start_percent,
                    end_percent,
                )
                .await
                {
                    Ok(output) => {
                        combined_log.push_str(&output);
                        continue;
                    }
                    Err(error) => {
                        combined_log.push_str(&format!(
                            "## {dataset} — téléchargement découpé en échec\n{error}\n"
                        ));
                        tokio::fs::write(&log_path, &combined_log).await?;
                        return Err(error);
                    }
                }
            }
            let mut attempt = 1_u8;
            loop {
                let mut attempt_request = request.clone();
                attempt_request.force = request.force && attempt == 1;
                match run_engine_download(
                    dataset,
                    &attempt_request,
                    &config.data_root(),
                    cancellation.clone(),
                    &progress,
                    start_percent,
                    end_percent,
                )
                .await
                {
                    Ok(output) => {
                        combined_log
                            .push_str(&format!("## {dataset} — tentative {attempt}\n{output}\n"));
                        break;
                    }
                    Err(error) if tolerable_orthophoto_gaps(dataset, &error).is_some() => {
                        let (missing, total) = tolerable_orthophoto_gaps(dataset, &error)
                            .expect("la condition vient de vérifier ces valeurs");
                        let warning = format!(
                            "[AVERTISSEMENT] [TUILES MANQUANTES] {missing} tuile(s) orthophoto indisponible(s) sur {total}. Les petites zones sans image, fréquentes en mer, sont conservées et utilisent la couleur de l'eau dans l'atlas Blender."
                        );
                        combined_log.push_str(&format!(
                            "## {dataset} — couverture partielle acceptée\n{error}\n{warning}\n"
                        ));
                        warnings.push(warning);
                        break;
                    }
                    Err(error) if is_retryable_download_error(&error) && attempt < 4 => {
                        combined_log.push_str(&format!(
                            "## {dataset} — tentative {attempt} incomplète\n{error}\n"
                        ));
                        attempt += 1;
                        report_progress(
                            &progress,
                            10 + ((index * 70) / dataset_count) as u8,
                            format!(
                                "{dataset} : reprise des éléments manquants (tentative {attempt}/4)"
                            ),
                        );
                        // Les fichiers réussis sont déjà en cache. Les tentatives suivantes ne
                        // doivent donc jamais forcer leur téléchargement une seconde fois.
                        sleep(Duration::from_secs(u64::from(attempt))).await;
                    }
                    Err(error) => {
                        combined_log.push_str(&format!(
                            "## {dataset} — tentative {attempt} en échec\n{error}\n"
                        ));
                        tokio::fs::write(&log_path, &combined_log).await?;
                        return Err(error);
                    }
                }
            }
        }
        report_progress(&progress, 90, "Écriture du manifeste de la zone");
        let manifest_path = job_root.join("download.json");
        let manifest = DownloadManifest {
            schema_version: 1,
            job_id: &job.id,
            request,
            data_root: config.data_root(),
            warnings: &warnings,
            completed_at: Utc::now().to_rfc3339(),
        };
        write_json(&manifest_path, &manifest).await?;
        tokio::fs::write(&log_path, combined_log).await?;
        Ok(vec![
            artifact(job, &manifest_path, ArtifactKind::DownloadManifest).await?,
            artifact(job, &log_path, ArtifactKind::WorkerLog).await?,
        ])
    }

    /// Crée la configuration calculée puis appelle le moteur correspondant au format.
    async fn export(
        &self,
        job: &Job,
        request: &ExportRequest,
        cancellation: CancellationToken,
        progress: ProgressSender,
    ) -> AppResult<Vec<Artifact>> {
        let export_started = Instant::now();
        report_progress(&progress, 10, "Préparation de la configuration d'export");
        let config = self.config.lock().await.clone();
        let job_root = config.exports_root().join(&job.id);
        tokio::fs::create_dir_all(&job_root).await?;
        let output_path = job_root.join(request.resolved_output_name());
        let engine_config_path = job_root.join("engine.toml");
        let request_path = job_root.join("request.json");
        write_json(&request_path, request).await?;
        let legacy_config = legacy_export_config(request, &config, &job_root, &output_path);
        let config_text = toml::to_string_pretty(&legacy_config)
            .map_err(|error| AppError::engine(format!("configuration d'export : {error}")))?;
        tokio::fs::write(&engine_config_path, config_text).await?;

        let arguments = export_command(request, &engine_config_path, &output_path);
        if request.format == ExportFormat::Blender {
            report_progress(&progress, 20, "Recherche d'une scène Blender réutilisable");
        } else {
            report_progress(
                &progress,
                25,
                format!(
                    "Génération de l'export {}",
                    export_format_label(request.format)
                ),
            );
        }
        let log_path = job_root.join("worker.log");
        let source_fingerprint = if request.format == ExportFormat::Blender {
            let data_root = config.data_root();
            Some(
                tokio::task::spawn_blocking(move || blender_source_fingerprint(&data_root))
                    .await
                    .map_err(|error| AppError::engine(error.to_string()))??,
            )
        } else {
            None
        };
        let base_cache_path = source_fingerprint
            .as_deref()
            .map(|fingerprint| blender_base_cache_path(&config, request, fingerprint))
            .transpose()?;
        let final_cache_path = source_fingerprint
            .as_deref()
            .map(|fingerprint| blender_final_cache_path(&config, request, fingerprint))
            .transpose()?;
        let mut cache_notice = String::new();
        let mut final_cache_hit = false;
        if !request.force
            && let Some(cache_path) = final_cache_path.as_deref()
            && restore_blender_base(cache_path, &output_path)
                .await
                .unwrap_or(false)
        {
            final_cache_hit = true;
            report_progress(&progress, 82, "Export Blender final retrouvé dans le cache");
            cache_notice = format!(
                "[{}] [CACHE BLENDER] Scène finale réutilisée sans génération ni finition\n",
                local_timestamp()
            );
        }
        let mut base_cache_hit = final_cache_hit;
        if may_restore_blender_base(final_cache_hit, request.force)
            && let Some(cache_path) = base_cache_path.as_deref()
        {
            let restore_started = Instant::now();
            match restore_blender_base(cache_path, &output_path).await {
                Ok(true) => {
                    base_cache_hit = true;
                    report_progress(&progress, 55, "Cache Blender trouvé · géométrie réutilisée");
                    cache_notice = format!(
                        "[{}] [CACHE BLENDER] Scène géométrique réutilisée · copie {}\n",
                        local_timestamp(),
                        readable_duration(restore_started.elapsed())
                    );
                }
                Ok(false) => {}
                Err(error) => {
                    cache_notice = format!(
                        "[{}] [CACHE BLENDER] Lecture impossible, reconstruction complète : {error}\n",
                        local_timestamp()
                    );
                }
            }
        }
        let mut log = if base_cache_hit {
            cache_notice
        } else {
            if request.format == ExportFormat::Blender {
                report_progress(
                    &progress,
                    25,
                    "Cache absent · construction de la géométrie Blender",
                );
            }
            let generation =
                run_engine_export(request.format, &arguments, cancellation.clone()).await;
            let generated = match generation {
                Ok(log) => log,
                Err(error) if is_road_topology_error(request, &arguments, &error) => {
                    report_progress(
                        &progress,
                        55,
                        "Routes invalides détectées, reconstruction segmentée en cours",
                    );
                    let first_error = error.to_string();
                    let Some(surface_cache) = road_repair::surface_cache_path(&first_error) else {
                        tokio::fs::write(&log_path, &first_error).await?;
                        return Err(AppError::engine(
                            "le moteur n'a pas indiqué le cache de surface nécessaire à la réparation des routes",
                        ));
                    };
                    let retry_arguments = without_road_arguments(&arguments);
                    match run_engine_export(request.format, &retry_arguments, cancellation.clone())
                        .await
                    {
                        Ok(retry_log) => {
                            report_progress(
                                &progress,
                                78,
                                "Ajout des rubans routiers réparés au STL",
                            );
                            match road_repair::append_segmented_roads(
                                output_path.clone(),
                                config.data_root(),
                                surface_cache,
                                request.clone(),
                            )
                            .await
                            {
                                Ok(report) => format!(
                                    "[AVERTISSEMENT] [ROUTES RÉPARÉES] L'union des chaussées n'était pas manifold. Les routes ont été reconstruites en {} rubans fermés ({} triangles) depuis {} page(s) BD TOPO.\n\n## Première tentative\n{first_error}\n\n## Reconstruction de la base sans routes\n{retry_log}",
                                    report.segments, report.triangles, report.source_files
                                ),
                                Err(repair_error) => {
                                    let combined = format!(
                                        "La première tentative a échoué sur l'union des routes. La base sans routes a été créée, mais la reconstruction segmentée a échoué.\n\n## Première tentative\n{first_error}\n\n## Reconstruction de la base\n{retry_log}\n\n## Réparation des routes\n{repair_error}"
                                    );
                                    tokio::fs::write(&log_path, &combined).await?;
                                    return Err(AppError::engine(combined));
                                }
                            }
                        }
                        Err(retry_error) => {
                            let combined = format!(
                                "La première tentative a échoué sur le maillage des routes. La reconstruction de la base a également échoué.\n\n## Première tentative\n{first_error}\n\n## Nouvelle tentative\n{retry_error}"
                            );
                            tokio::fs::write(&log_path, &combined).await?;
                            return Err(AppError::engine(combined));
                        }
                    }
                }
                Err(error) => {
                    tokio::fs::write(&log_path, error.to_string()).await?;
                    return Err(error);
                }
            };
            format!("{cache_notice}{generated}")
        };
        if tokio::fs::metadata(&output_path).await.is_err() {
            return Err(AppError::engine(format!(
                "le moteur n'a pas créé {}",
                output_path.display()
            )));
        }
        if !base_cache_hit && let Some(cache_path) = base_cache_path.as_deref() {
            report_progress(&progress, 72, "Mise en cache de la scène Blender de base");
            let cache_started = Instant::now();
            match store_blender_base(&output_path, cache_path).await {
                Ok(()) => log.push_str(&format!(
                    "[{}] [CACHE BLENDER] Scène géométrique enregistrée · {}\n",
                    local_timestamp(),
                    readable_duration(cache_started.elapsed())
                )),
                Err(error) => log.push_str(&format!(
                    "[{}] [CACHE BLENDER] Enregistrement ignoré : {error}\n",
                    local_timestamp()
                )),
            }
        }
        if uses_stl_canopy(request) {
            report_progress(&progress, 84, "Construction de la canopée LiDAR continue");
            let canopy_started = Instant::now();
            let Some(surface_cache) = road_repair::surface_cache_path(&log) else {
                let error = AppError::engine(
                    "le moteur n'a pas indiqué le cache LiDAR nécessaire à la canopée",
                );
                log.push_str("\n## Canopée LiDAR\n");
                log.push_str(&error.to_string());
                tokio::fs::write(&log_path, &log).await?;
                return Err(error);
            };
            match road_repair::append_forest_canopy(
                output_path.clone(),
                config.data_root(),
                surface_cache,
                request.clone(),
            )
            .await
            {
                Ok(report) => log.push_str(&format!(
                    "\n## Canopée LiDAR\n[{}] {} massif(s) continu(s), {} triangles ajoutés au STL, {} cellule(s) retirée(s) au droit des routes · durée {}\n",
                    local_timestamp(),
                    report.patches,
                    report.triangles,
                    report.road_cells_removed,
                    readable_duration(canopy_started.elapsed())
                )),
                Err(error) => {
                    log.push_str("\n## Échec de la canopée LiDAR\n");
                    log.push_str(&error.to_string());
                    tokio::fs::write(&log_path, &log).await?;
                    return Err(error);
                }
            }
        }
        let mut postprocess_config_path = None;
        if request.format == ExportFormat::Blender && !final_cache_hit {
            let canopy_cache = if uses_blender_canopy(request) {
                report_progress(&progress, 80, "Lecture du masque forestier LiDAR complet");
                match ensure_blender_canopy_cache(
                    request,
                    &engine_config_path,
                    &job_root,
                    cancellation.clone(),
                    &progress,
                )
                .await
                {
                    Ok((path, cache_log)) => {
                        log.push_str("\n## Masque forestier LiDAR\n");
                        log.push_str(&cache_log);
                        Some(path)
                    }
                    Err(error) => {
                        log.push_str("\n## Échec du masque forestier LiDAR\n");
                        log.push_str(&error.to_string());
                        tokio::fs::write(&log_path, &log).await?;
                        return Err(error);
                    }
                }
            } else {
                None
            };
            report_progress(
                &progress,
                86,
                "Optimisation de l’orthophoto et des matériaux Blender",
            );
            let postprocess_result = postprocess_blender(
                request,
                &job_root,
                &output_path,
                cancellation.clone(),
                progress.clone(),
                canopy_cache.as_deref(),
            )
            .await;
            match postprocess_result {
                Ok((postprocess_log, settings_path)) => {
                    log.push_str("\n## Finition Blender\n");
                    log.push_str(&postprocess_log);
                    postprocess_config_path = Some(settings_path);
                }
                Err(error) => {
                    log.push_str("\n## Échec de la finition Blender\n");
                    log.push_str(&error.to_string());
                    tokio::fs::write(&log_path, &log).await?;
                    return Err(error);
                }
            }
        }
        if request.format == ExportFormat::Blender
            && !final_cache_hit
            && let Some(cache_path) = final_cache_path.as_deref()
        {
            report_progress(&progress, 94, "Mise en cache de l'export Blender terminé");
            match store_blender_final(&output_path, cache_path).await {
                Ok(()) => log.push_str(&format!(
                    "[{}] [CACHE BLENDER] Scène finale enregistrée pour les prochains exports identiques\n",
                    local_timestamp()
                )),
                Err(error) => log.push_str(&format!(
                    "[{}] [CACHE BLENDER] Cache final ignoré : {error}\n",
                    local_timestamp()
                )),
            }
        }
        let kind = match request.format {
            ExportFormat::Stl | ExportFormat::StlLayers => ArtifactKind::Stl,
            ExportFormat::Blender => ArtifactKind::Blender,
        };
        let mut artifacts = vec![
            artifact(job, &output_path, kind).await?,
            artifact(job, &request_path, ArtifactKind::Configuration).await?,
            artifact(job, &engine_config_path, ArtifactKind::Configuration).await?,
        ];
        if let Some(path) = postprocess_config_path {
            artifacts.push(artifact(job, &path, ArtifactKind::Configuration).await?);
        }
        log.push_str(&format!(
            "[{}] [EXPORT] Traitement complet · durée {}\n",
            local_timestamp(),
            readable_duration(export_started.elapsed())
        ));
        tokio::fs::write(&log_path, &log).await?;
        artifacts.push(artifact(job, &log_path, ArtifactKind::WorkerLog).await?);
        Ok(artifacts)
    }
}

#[async_trait]
impl ProcessingBackend for EngineBackend {
    /// Oriente le job vers le traitement correspondant sans logique de file d'attente.
    async fn execute(
        &self,
        job: &Job,
        cancellation: CancellationToken,
        progress: ProgressSender,
    ) -> AppResult<Vec<Artifact>> {
        match &job.kind {
            JobKind::Download(request) => self.download(job, request, cancellation, progress).await,
            JobKind::Export(request) => self.export(job, request, cancellation, progress).await,
        }
    }

    /// Construit les surfaces LiDAR réutilisables sans produire de STL temporaire.
    async fn prewarm_stl_surfaces(
        &self,
        download: &DownloadRequest,
        steps_m: &[f64],
        cancellation: CancellationToken,
    ) -> AppResult<()> {
        if steps_m.is_empty()
            || (!download.layers.is_empty() && !download.layers.contains(&DataLayer::Lidar))
        {
            return Ok(());
        }
        let config = self.config.lock().await.clone();
        let warm_root = config
            .storage
            .root
            .join("cache")
            .join("warmup")
            .join(uuid::Uuid::new_v4().to_string());
        tokio::fs::create_dir_all(&warm_root).await?;
        let mut steps = steps_m.to_vec();
        steps.sort_by(f64::total_cmp);
        steps.dedup_by(|left, right| *left == *right);
        let result = async {
            for (index, step_m) in steps.into_iter().enumerate() {
                if cancellation.is_cancelled() {
                    return Err(AppError::Conflict("préchauffage annulé".to_owned()));
                }
                let output_root = warm_root.join(index.to_string());
                tokio::fs::create_dir_all(&output_root).await?;
                let output_path = output_root.join("surface-cache-warmup.stl");
                let request = ExportRequest {
                    area: download.area.clone(),
                    format: ExportFormat::Stl,
                    layers: vec![DataLayer::Lidar],
                    quality: ExportQuality::Standard,
                    output_name: Some("surface-cache-warmup.stl".to_owned()),
                    advanced: AdvancedExport {
                        terrain_grid_step_m: Some(step_m),
                        include_buildings: Some(false),
                        include_roads: Some(false),
                        include_trees: Some(false),
                        ..AdvancedExport::default()
                    },
                    force: false,
                }
                .normalized()?;
                let legacy_config =
                    legacy_export_config(&request, &config, &output_root, &output_path);
                let config_path = output_root.join("engine.toml");
                let config_text = toml::to_string_pretty(&legacy_config).map_err(|error| {
                    AppError::engine(format!("configuration cache STL : {error}"))
                })?;
                tokio::fs::write(&config_path, config_text).await?;
                let log = run_engine_stl_surface_warm(&config_path, cancellation.clone()).await?;
                tracing::info!(
                    grid_step_m = step_m,
                    "surface de cache STL prête : {}",
                    log.lines()
                        .find_map(|line| line.strip_prefix("[GEO_ENGINE_WARM_CACHE] "))
                        .unwrap_or("cache STL")
                );
            }
            AppResult::Ok(())
        }
        .await;
        if let Err(error) = tokio::fs::remove_dir_all(&warm_root).await
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(%error, "impossible de nettoyer les fichiers temporaires du cache STL");
        }
        result
    }

    /// Construit les caches Blender demandés en réutilisant la scène de base commune.
    async fn prewarm_blender_atlases(
        &self,
        download: &DownloadRequest,
        resolutions_px: &[u32],
        cancellation: CancellationToken,
    ) -> AppResult<()> {
        if resolutions_px.is_empty()
            || (!download.layers.is_empty() && !download.layers.contains(&DataLayer::Orthophoto))
            || (!download.layers.is_empty()
                && !download.layers.contains(&DataLayer::Elevation)
                && !download.layers.contains(&DataLayer::Lidar))
        {
            return Ok(());
        }
        if find_blender_executable().is_none() {
            tracing::info!("préchauffage Blender ignoré : Blender n'est pas installé");
            return Ok(());
        }
        let config = self.config.lock().await.clone();
        let mut resolutions = resolutions_px.to_vec();
        resolutions.sort_unstable();
        resolutions.dedup();
        for resolution in resolutions {
            if cancellation.is_cancelled() {
                return Err(AppError::Conflict("préchauffage annulé".to_owned()));
            }
            let request = ExportRequest {
                area: download.area.clone(),
                format: ExportFormat::Blender,
                layers: download.layers.clone(),
                quality: ExportQuality::Standard,
                output_name: Some(format!("precalcule-{resolution}.blend")),
                advanced: AdvancedExport {
                    colors: Some(config.render.colors.clone()),
                    orthophoto_storage: Some(OrthophotoStorage::Atlas),
                    orthophoto_resolution_px: Some(resolution),
                    ..AdvancedExport::default()
                },
                force: false,
            }
            .normalized()?;
            let job = Job::queued(
                JobKind::Export(Box::new(request.clone())),
                format!("blender-cache-warmup-{resolution}"),
            );
            let (progress, _progress_receiver) = mpsc::unbounded_channel();
            let result = self
                .export(&job, &request, cancellation.clone(), progress)
                .await;
            let output_root = config.exports_root().join(&job.id);
            if let Err(cleanup_error) = tokio::fs::remove_dir_all(&output_root).await
                && cleanup_error.kind() != std::io::ErrorKind::NotFound
            {
                tracing::warn!(%cleanup_error, "impossible de nettoyer l'export Blender temporaire");
            }
            result?;
            tracing::info!(resolution_px = resolution, "cache atlas Blender prêt");
        }
        Ok(())
    }
}

/// Fournit un nom court et stable pour les messages de progression.
fn export_format_label(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Stl => "STL",
        ExportFormat::StlLayers => "STL par couches",
        ExportFormat::Blender => "Blender",
    }
}

/// Manifeste lisible qui relie les données brutes à leur configuration d'origine.
#[derive(Serialize)]
struct DownloadManifest<'a> {
    schema_version: u32,
    job_id: &'a str,
    request: &'a DownloadRequest,
    data_root: PathBuf,
    warnings: &'a [String],
    completed_at: String,
}

/// Sous-ensemble du TOML compris par les moteurs existants.
#[derive(Serialize)]
struct LegacyExportConfig {
    schema_version: u32,
    area: LegacyArea,
    paths: LegacyPaths,
    terrain: LegacyTerrain,
    buildings: LegacyBuildings,
    vegetation: LegacyVegetation,
    view: LegacyView,
    layers: BTreeMap<String, LegacyLayer>,
}

/// Désactive la seconde ouverture de Blender utilisée seulement pour replier l'Outliner.
#[derive(Serialize)]
struct LegacyView {
    collapse_outliner: bool,
}

/// Zone traduite dans le vocabulaire du moteur existant.
#[derive(Serialize)]
struct LegacyArea {
    center: [f64; 2],
    crs: String,
    side_m: f64,
}

/// Chemins entièrement résolus pour rendre chaque export reproductible.
#[derive(Serialize)]
struct LegacyPaths {
    cache_root: PathBuf,
    output_file: PathBuf,
    work_directory: PathBuf,
    keep_work_files: bool,
    overwrite: bool,
    prepare_only: bool,
}

/// Réglages de relief déduits du seul preset de qualité.
#[derive(Serialize)]
struct LegacyTerrain {
    grid_step_m: f64,
    max_vertices: usize,
    vertical_exaggeration: f64,
}

/// Réglage existant qui contrôle le plaquage des orthophotos sur les toits.
#[derive(Serialize)]
struct LegacyBuildings {
    texture_roofs: bool,
}

/// Couleur de feuillage comprise par le générateur procédural Blender.
#[derive(Serialize)]
struct LegacyVegetation {
    classes: Vec<u8>,
    foliage_color: [f64; 4],
    procedural_variation: f64,
}

/// Réglages transmis à la finition Blender conservée entièrement dans le nouveau projet.
#[derive(Serialize)]
struct BlenderPostprocessConfig {
    output: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    canopy_cache: Option<PathBuf>,
    vertical_exaggeration: f64,
    road_elevation_m: f64,
    wall_color: [f64; 4],
    roof_color: [f64; 4],
    texture_roofs: bool,
    orthophoto_storage: String,
    orthophoto_resolution_px: u32,
    water_color: [f64; 4],
    tree_shape: String,
    procedural_materials: bool,
    forest_quality: String,
    canopy_smoothing: f64,
    forest_material_mode: String,
    forest_color_low: [f64; 4],
    forest_color_high: [f64; 4],
    carve_forest_roads: bool,
    keep_isolated_trees: bool,
}

/// Valeur minimale nécessaire pour activer ou masquer une couche existante.
#[derive(Serialize)]
struct LegacyLayer {
    enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    color: Option<[f64; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    z_offset_m: Option<f64>,
}

/// Construit le TOML détaillé à partir du contrat court conservé par l'API.
fn legacy_export_config(
    request: &ExportRequest,
    config: &AppConfig,
    job_root: &Path,
    output_path: &Path,
) -> LegacyExportConfig {
    let selected: BTreeSet<_> = request.layers.iter().copied().collect();
    let all = selected.is_empty();
    let contains = |layer| all || selected.contains(&layer);
    let texture_mode = request.advanced.texture_mode.unwrap_or_default();
    let include_buildings = request.advanced.include_buildings.unwrap_or(true);
    let include_roads = request.advanced.include_roads.unwrap_or(true);
    let include_trees = request.advanced.include_trees.unwrap_or(true);
    let colors = request.advanced.colors.as_ref();
    let preferred_grid_step = request
        .advanced
        .terrain_grid_step_m
        .unwrap_or_else(|| request.quality.grid_step_m());
    let (grid_step_m, max_vertices) =
        automatic_terrain_plan(request.area.size_m, preferred_grid_step);
    let mut layers = BTreeMap::new();
    let mut add = |name: &str, enabled, color: Option<[f64; 4]>| {
        layers.insert(
            name.to_owned(),
            LegacyLayer {
                enabled,
                color,
                z_offset_m: None,
            },
        );
    };
    add(
        "terrain",
        contains(DataLayer::Elevation) || contains(DataLayer::Lidar),
        colors.and_then(|colors| colors.ground),
    );
    add(
        "orthophoto",
        contains(DataLayer::Orthophoto) && texture_mode != TextureMode::None,
        None,
    );
    add(
        "vegetation",
        contains(DataLayer::Lidar) && include_trees,
        None,
    );
    add("lidar_points", contains(DataLayer::Lidar), None);
    add(
        "bd_topo.batiment",
        contains(DataLayer::Buildings) && include_buildings,
        colors.and_then(|colors| colors.walls),
    );
    for name in [
        "bd_topo.troncon_de_route",
        "bd_topo.troncon_de_voie_ferree",
        "bd_topo.construction_lineaire",
        "bd_topo.construction_ponctuelle",
        "bd_topo.construction_surfacique",
        "bd_topo.equipement_de_transport",
        "bd_topo.ligne_electrique",
        "bd_topo.pylone",
        "bd_topo.poste_de_transformation",
        "bd_topo.canalisation",
        "bd_topo.reservoir",
    ] {
        let color = if name == "bd_topo.troncon_de_route" {
            Some(
                colors
                    .and_then(|colors| colors.roads)
                    .unwrap_or([0.105_882, 0.121_569, 0.113_725, 1.0]),
            )
        } else {
            None
        };
        add(name, contains(DataLayer::Transport) && include_roads, color);
    }
    add("cadastre.parcelle", contains(DataLayer::Cadastre), None);
    for name in [
        "bd_topage.CoursEau",
        "bd_topage.TronconHydrographique",
        "bd_topage.PlanEau",
        "bd_topage.SurfaceElementaire",
        "bd_topage.NoeudHydrographique",
        "bd_topage.BassinVersantTopographique",
        "bd_topage.BassinHydrographique",
        "bd_topage.LimiteTerreEau",
    ] {
        add(
            name,
            contains(DataLayer::Hydrography),
            Some(
                colors
                    .and_then(|colors| colors.water)
                    .unwrap_or([0.05, 0.42, 0.68, 1.0]),
            ),
        );
    }
    // L'ancien moteur ajoutait toujours 0,20 m aux chaussées. Le curseur Blender
    // devenait donc un second décalage et ne pouvait jamais remettre la route au sol.
    // La finition projette désormais la route sur le vrai maillage, puis applique
    // road_elevation_m comme hauteur absolue au-dessus de ce maillage.
    if let Some(roads) = layers.get_mut("bd_topo.troncon_de_route") {
        roads.z_offset_m = Some(0.0);
    }
    LegacyExportConfig {
        schema_version: 1,
        area: LegacyArea {
            center: request.area.center,
            crs: legacy_crs(&request.area).to_owned(),
            side_m: request.area.size_m,
        },
        paths: LegacyPaths {
            cache_root: config.data_root(),
            // Le lecteur TOML historique valide encore une extension Blender,
            // même lorsque geo-stl reçoit sa vraie destination via `--output`.
            output_file: if matches!(request.format, ExportFormat::Stl | ExportFormat::StlLayers) {
                job_root.join("engine-validation.blend")
            } else {
                output_path.to_path_buf()
            },
            work_directory: job_root.join("work"),
            keep_work_files: false,
            overwrite: true,
            prepare_only: false,
        },
        terrain: LegacyTerrain {
            grid_step_m,
            max_vertices,
            vertical_exaggeration: request.advanced.vertical_exaggeration.unwrap_or(1.0),
        },
        buildings: LegacyBuildings {
            texture_roofs: contains(DataLayer::Orthophoto)
                && texture_mode == TextureMode::GroundAndRoofs,
        },
        vegetation: LegacyVegetation {
            // Une forêt continue représente les houppiers hauts (classe ASPRS 5).
            // Les classes 3 et 4 couvrent aussi cultures, haies et végétation basse :
            // les inclure créait à tort une nappe forestière jusque dans les villages.
            classes: if request.advanced.tree_shape.unwrap_or_default() == TreeShape::Canopy {
                vec![5]
            } else {
                vec![3, 4, 5]
            },
            foliage_color: colors
                .and_then(|colors| colors.trees)
                .unwrap_or([0.12, 0.28, 0.065, 1.0]),
            procedural_variation: if request.advanced.procedural_materials.unwrap_or(true) {
                0.12
            } else {
                0.0
            },
        },
        // Le repli automatique lançait une seconde instance graphique de Blender et pouvait
        // planter dans le pilote Intel. L'organisation des collections reste intacte sans lui.
        view: LegacyView {
            collapse_outliner: false,
        },
        layers,
    }
}

/// Calcule ensemble la finesse et la limite mémoire à partir de la zone et de la qualité.
fn automatic_terrain_plan(side_m: f64, preferred_step_m: f64) -> (f64, usize) {
    let preferred_axis_points = (side_m / preferred_step_m).ceil() as usize + 1;
    let preferred_vertices = preferred_axis_points.saturating_mul(preferred_axis_points);
    if preferred_vertices <= TERRAIN_HARD_MAX_VERTICES {
        let limit_with_margin = ((preferred_vertices as f64) * 1.05).ceil() as usize;
        return (
            preferred_step_m,
            limit_with_margin.clamp(TERRAIN_DEFAULT_MAX_VERTICES, TERRAIN_HARD_MAX_VERTICES),
        );
    }
    let max_axis_points = (TERRAIN_HARD_MAX_VERTICES as f64).sqrt().floor().max(2.0);
    let minimum_step = side_m / (max_axis_points - 1.0);
    let safe_step = (minimum_step * 10.0).ceil() / 10.0;
    (preferred_step_m.max(safe_step), TERRAIN_HARD_MAX_VERTICES)
}

/// Autorise une régénération forcée à repartir de la géométrie compatible déjà calculée.
fn may_restore_blender_base(final_cache_hit: bool, _forced_regeneration: bool) -> bool {
    // « Régénérer » doit recalculer la finition demandée, pas les données et la géométrie.
    // La clé de base décide déjà si un changement structurel interdit la réutilisation.
    !final_cache_hit
}

/// Calcule une clé qui ignore uniquement les finitions appliquées après la génération géométrique.
fn blender_source_fingerprint(data_root: &Path) -> AppResult<String> {
    let mut directories = vec![data_root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = directories.pop() {
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(AppError::from(error)),
        };
        for entry in entries {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                if entry.file_name() == "STL_CACHE" {
                    continue;
                }
                directories.push(entry.path());
            } else if file_type.is_file() {
                let path = entry.path();
                if path
                    .extension()
                    .is_some_and(|extension| extension == "part")
                {
                    continue;
                }
                let metadata = entry.metadata()?;
                let modified_ns = metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |duration| duration.as_nanos());
                files.push((
                    path.strip_prefix(data_root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .into_owned(),
                    metadata.len(),
                    modified_ns,
                ));
            }
        }
    }
    files.sort_unstable();
    let bytes = serde_json::to_vec(&files)
        .map_err(|error| AppError::storage(format!("empreinte des données Blender : {error}")))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn blender_base_cache_key(request: &ExportRequest, source_fingerprint: &str) -> AppResult<String> {
    let mut reusable = request.clone();
    reusable.output_name = None;
    reusable.force = false;
    // La base d'une canopée est calculée uniquement avec la classe LiDAR 5,
    // contrairement aux arbres individuels qui acceptent les classes 3, 4 et 5.
    // Ces deux géométries ne doivent donc pas partager la même scène de base.
    if reusable.advanced.tree_shape != Some(TreeShape::Canopy) {
        reusable.advanced.tree_shape = None;
    }
    reusable.advanced.forest_quality = None;
    reusable.advanced.canopy_smoothing = None;
    reusable.advanced.forest_material_mode = None;
    reusable.advanced.carve_forest_roads = None;
    reusable.advanced.keep_isolated_trees = None;
    reusable.advanced.road_elevation_m = None;
    reusable.advanced.orthophoto_storage = None;
    reusable.advanced.orthophoto_resolution_px = None;
    if let Some(colors) = reusable.advanced.colors.as_mut() {
        colors.walls = None;
        colors.roofs = None;
        colors.forest_low = None;
        colors.forest_high = None;
    }
    let serialized = serde_json::to_vec(&reusable)
        .map_err(|error| AppError::storage(format!("clé de cache Blender : {error}")))?;
    let mut digest = Sha256::new();
    digest.update(b"geo-platform-blender-base-v4");
    digest.update(source_fingerprint.as_bytes());
    digest.update(serialized);
    Ok(hex::encode(digest.finalize()))
}

/// Retourne l'emplacement isolé d'une scène Blender avant finitions visuelles.
fn blender_base_cache_path(
    config: &AppConfig,
    request: &ExportRequest,
    source_fingerprint: &str,
) -> AppResult<PathBuf> {
    Ok(config
        .storage
        .root
        .join("cache")
        .join("blender-base")
        .join(format!(
            "{}.blend",
            blender_base_cache_key(request, source_fingerprint)?
        )))
}

/// Calcule la clé d'une scène entièrement finie en ignorant seulement son nom de sortie.
fn blender_final_cache_key(request: &ExportRequest, source_fingerprint: &str) -> AppResult<String> {
    let mut reusable = request.clone();
    reusable.output_name = None;
    reusable.force = false;
    let serialized = serde_json::to_vec(&reusable)
        .map_err(|error| AppError::storage(format!("clé de cache Blender final : {error}")))?;
    let mut digest = Sha256::new();
    digest.update(b"geo-platform-blender-final-v11");
    digest.update(source_fingerprint.as_bytes());
    digest.update(serialized);
    Ok(hex::encode(digest.finalize()))
}

/// Retourne l'emplacement d'une scène finale prête à être téléchargée.
fn blender_final_cache_path(
    config: &AppConfig,
    request: &ExportRequest,
    source_fingerprint: &str,
) -> AppResult<PathBuf> {
    Ok(config
        .storage
        .root
        .join("cache")
        .join("blender-final")
        .join(format!(
            "{}.blend",
            blender_final_cache_key(request, source_fingerprint)?
        )))
}

/// Copie une base existante vers le nouvel export sans modifier le fichier partagé.
async fn restore_blender_base(cache_path: &Path, output_path: &Path) -> AppResult<bool> {
    let metadata = match tokio::fs::metadata(cache_path).await {
        Ok(metadata) if metadata.is_file() && metadata.len() > 0 => metadata,
        Ok(_) => return Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    tokio::fs::copy(cache_path, output_path).await?;
    Ok(metadata.len() > 0)
}

/// Publie atomiquement la scène de base puis limite le nombre de variantes conservées.
async fn store_blender_base(output_path: &Path, cache_path: &Path) -> AppResult<()> {
    store_blender_cache(output_path, cache_path, BLENDER_BASE_CACHE_LIMIT).await
}

/// Publie une scène finale dans son cache plus court.
async fn store_blender_final(output_path: &Path, cache_path: &Path) -> AppResult<()> {
    store_blender_cache(output_path, cache_path, BLENDER_FINAL_CACHE_LIMIT).await
}

/// Copie atomiquement une scène puis borne le nombre de variantes du dossier.
async fn store_blender_cache(output_path: &Path, cache_path: &Path, limit: usize) -> AppResult<()> {
    let directory = cache_path
        .parent()
        .ok_or_else(|| AppError::storage("dossier de cache Blender absent"))?;
    tokio::fs::create_dir_all(directory).await?;
    let temporary = directory.join(format!("{}.part", uuid::Uuid::new_v4().simple()));
    tokio::fs::copy(output_path, &temporary).await?;
    match tokio::fs::remove_file(cache_path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    tokio::fs::rename(&temporary, cache_path).await?;
    let _ = prune_blender_cache(directory, limit).await;
    Ok(())
}

/// Garde les scènes les plus récentes afin que le cache ne grossisse pas sans limite.
async fn prune_blender_cache(directory: &Path, limit: usize) -> AppResult<()> {
    let mut reader = tokio::fs::read_dir(directory).await?;
    let mut entries = Vec::new();
    while let Some(entry) = reader.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("blend") {
            continue;
        }
        let metadata = entry.metadata().await?;
        let modified = metadata
            .modified()
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        entries.push((modified, path));
    }
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    for (_, path) in entries.into_iter().skip(limit) {
        match tokio::fs::remove_file(path).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Relance Blender une seconde fois pour appliquer les options absentes du moteur historique.
async fn postprocess_blender(
    request: &ExportRequest,
    job_root: &Path,
    output_path: &Path,
    cancellation: CancellationToken,
    progress: ProgressSender,
    canopy_cache: Option<&Path>,
) -> AppResult<(String, PathBuf)> {
    let executable = find_blender_executable().ok_or_else(|| {
        AppError::engine("Blender est introuvable pour la finition des matériaux")
    })?;
    let colors = request.advanced.colors.as_ref();
    let settings = BlenderPostprocessConfig {
        output: output_path.to_path_buf(),
        canopy_cache: canopy_cache.map(Path::to_path_buf),
        vertical_exaggeration: request.advanced.vertical_exaggeration.unwrap_or(1.0),
        road_elevation_m: request.advanced.road_elevation_m.unwrap_or(0.0),
        wall_color: colors
            .and_then(|colors| colors.walls)
            .unwrap_or([0.83, 0.80, 0.73, 1.0]),
        roof_color: colors
            .and_then(|colors| colors.roofs)
            .unwrap_or([0.72, 0.49, 0.27, 1.0]),
        texture_roofs: request.advanced.texture_mode.unwrap_or_default()
            == TextureMode::GroundAndRoofs,
        orthophoto_storage: match request.advanced.orthophoto_storage.unwrap_or_default() {
            OrthophotoStorage::Atlas => "atlas",
            OrthophotoStorage::Native => "native",
        }
        .to_owned(),
        orthophoto_resolution_px: request.advanced.orthophoto_resolution_px.unwrap_or(2_048),
        water_color: colors
            .and_then(|colors| colors.water)
            .unwrap_or([0.05, 0.42, 0.68, 1.0]),
        tree_shape: request
            .advanced
            .tree_shape
            .unwrap_or_default()
            .blender_name()
            .to_owned(),
        procedural_materials: request.advanced.procedural_materials.unwrap_or(true),
        forest_quality: match request.advanced.forest_quality.unwrap_or_default() {
            ExportQuality::Draft => "draft",
            ExportQuality::Standard => "standard",
            ExportQuality::High => "high",
        }
        .to_owned(),
        canopy_smoothing: request.advanced.canopy_smoothing.unwrap_or(0.7),
        forest_material_mode: match request.advanced.forest_material_mode.unwrap_or_default() {
            ForestMaterialMode::Texture => "texture",
            ForestMaterialMode::Procedural => "procedural",
        }
        .to_owned(),
        forest_color_low: colors
            .and_then(|colors| colors.forest_low)
            .unwrap_or([0.025, 0.12, 0.02, 1.0]),
        forest_color_high: colors
            .and_then(|colors| colors.forest_high)
            .unwrap_or([0.08, 0.42, 0.06, 1.0]),
        carve_forest_roads: request.advanced.carve_forest_roads.unwrap_or(false),
        keep_isolated_trees: request.advanced.keep_isolated_trees.unwrap_or(true),
    };
    let settings_path = job_root.join("blender-postprocess.json");
    let script_path = job_root.join("blender-postprocess.py");
    write_json(&settings_path, &settings).await?;
    tokio::fs::write(
        &script_path,
        include_str!("../blender/postprocess_scene.py"),
    )
    .await?;
    let arguments = vec![
        "--background".to_owned(),
        output_path.to_string_lossy().into_owned(),
        "--disable-autoexec".to_owned(),
        "--python-exit-code".to_owned(),
        "1".to_owned(),
        "--python".to_owned(),
        script_path.to_string_lossy().into_owned(),
        "--".to_owned(),
        settings_path.to_string_lossy().into_owned(),
    ];
    let mut postprocess_progress = BlenderPostprocessProgress::default();
    let log = run_command_streaming(&executable, &arguments, cancellation, |line| {
        postprocess_progress.observe(line, &progress);
    })
    .await?;
    if request.advanced.tree_shape == Some(TreeShape::Canopy)
        && !log.contains("GEO_PLATFORM_CANOPY_OK")
    {
        return Err(AppError::engine(
            "Blender n'a pas confirmé la création de la canopée LiDAR".to_owned(),
        ));
    }
    if request.advanced.tree_shape == Some(TreeShape::Canopy)
        && request.advanced.carve_forest_roads.unwrap_or(false)
        && !log.contains("GEO_PLATFORM_ROAD_CUT_OK")
    {
        return Err(AppError::engine(
            "Blender n'a pas confirmé la découpe de la forêt au droit des routes".to_owned(),
        ));
    }
    Ok((log, settings_path))
}

/// Retrouve l'installation Blender déjà utilisée par le moteur, sans télécharger de programme.
fn find_blender_executable() -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["blender.exe"]
    } else {
        &["blender"]
    };
    if let Some(paths) = std::env::var_os("PATH") {
        for folder in std::env::split_paths(&paths) {
            for name in names {
                let candidate = folder.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    #[cfg(windows)]
    if let Some(programs) = std::env::var_os("ProgramFiles") {
        let root = PathBuf::from(programs).join("Blender Foundation");
        let mut candidates = std::fs::read_dir(root)
            .ok()?
            .filter_map(|entry| entry.ok().map(|entry| entry.path().join("blender.exe")))
            .filter(|path| path.is_file())
            .collect::<Vec<_>>();
        candidates.sort();
        return candidates.pop();
    }
    None
}

/// Prépare le programme et les options spécifiques au format, sans les exécuter.
fn export_command(request: &ExportRequest, engine_config: &Path, output: &Path) -> Vec<String> {
    let mut arguments = vec![
        "--config".to_owned(),
        engine_config.to_string_lossy().into_owned(),
    ];
    if matches!(request.format, ExportFormat::Stl | ExportFormat::StlLayers) {
        arguments.extend([
            "--output".to_owned(),
            output.to_string_lossy().into_owned(),
            "--width-mm".to_owned(),
            request.advanced.width_mm.unwrap_or(150.0).to_string(),
            "--base-mm".to_owned(),
            request.advanced.base_mm.unwrap_or(3.0).to_string(),
            "--quality".to_owned(),
            request.quality.legacy_stl_name().to_owned(),
            "--overwrite".to_owned(),
        ]);
        let all = request.layers.is_empty();
        if request.advanced.include_buildings.unwrap_or(true)
            && (all || request.layers.contains(&DataLayer::Buildings))
        {
            arguments.push("--buildings".to_owned());
        }
        if request.advanced.include_roads.unwrap_or(true)
            && (all || request.layers.contains(&DataLayer::Transport))
        {
            arguments.push("--roads".to_owned());
            arguments.extend([
                "--road-min-width-mm".to_owned(),
                request
                    .advanced
                    .road_min_width_mm
                    .unwrap_or(1.0)
                    .to_string(),
                "--road-height-mm".to_owned(),
                request.advanced.road_height_mm.unwrap_or(0.6).to_string(),
            ]);
        }
        if request.advanced.include_trees.unwrap_or(true)
            && (all || request.layers.contains(&DataLayer::Lidar))
            && request.advanced.tree_shape.unwrap_or_default() != TreeShape::Canopy
        {
            arguments.push("--trees".to_owned());
            arguments.extend([
                "--tree-min-height-mm".to_owned(),
                request
                    .advanced
                    .tree_min_height_mm
                    .unwrap_or(5.0)
                    .to_string(),
                "--tree-height-scale".to_owned(),
                request
                    .advanced
                    .tree_height_scale
                    .unwrap_or(1.0)
                    .to_string(),
                "--tree-trunk-diameter-mm".to_owned(),
                request
                    .advanced
                    .tree_trunk_diameter_mm
                    .unwrap_or(1.2)
                    .to_string(),
                "--tree-crown-diameter-mm".to_owned(),
                request
                    .advanced
                    .tree_crown_diameter_mm
                    .unwrap_or(3.0)
                    .to_string(),
                "--tree-shape".to_owned(),
                request
                    .advanced
                    .tree_shape
                    .unwrap_or_default()
                    .legacy_name()
                    .to_owned(),
            ]);
        }
    }
    if request.format == ExportFormat::StlLayers {
        arguments.extend([
            "--layer-height-m".to_owned(),
            request.advanced.layer_height_m.unwrap_or(1.0).to_string(),
        ]);
    }
    arguments
}

/// Indique si la végétation doit être ajoutée comme une nappe continue après le moteur STL.
fn uses_stl_canopy(request: &ExportRequest) -> bool {
    matches!(request.format, ExportFormat::Stl | ExportFormat::StlLayers)
        && request.advanced.include_trees.unwrap_or(true)
        && (request.layers.is_empty() || request.layers.contains(&DataLayer::Lidar))
        && request.advanced.tree_shape.unwrap_or_default() == TreeShape::Canopy
}

/// Indique si Blender doit recevoir le masque complet plutôt que les arbres d'aperçu.
fn uses_blender_canopy(request: &ExportRequest) -> bool {
    request.format == ExportFormat::Blender
        && request.advanced.include_trees.unwrap_or(true)
        && (request.layers.is_empty() || request.layers.contains(&DataLayer::Lidar))
        && request.advanced.tree_shape.unwrap_or_default() == TreeShape::Canopy
}

/// Prépare ou relit le cache LiDAR complet utilisé par la forêt Blender.
async fn ensure_blender_canopy_cache(
    request: &ExportRequest,
    engine_config: &Path,
    job_root: &Path,
    cancellation: CancellationToken,
    progress: &ProgressSender,
) -> AppResult<(PathBuf, String)> {
    let probe = job_root.join("canopy-cache-probe.stl");
    let arguments = vec![
        "--config".to_owned(),
        engine_config.to_string_lossy().into_owned(),
        "--output".to_owned(),
        probe.to_string_lossy().into_owned(),
        "--width-mm".to_owned(),
        "60".to_owned(),
        "--base-mm".to_owned(),
        "1".to_owned(),
        "--quality".to_owned(),
        request.quality.legacy_stl_name().to_owned(),
        "--overwrite".to_owned(),
    ];
    let path = run_engine_stl_cache(&arguments, cancellation).await;
    let _ = tokio::fs::remove_file(&probe).await;
    let path = path?;
    if tokio::fs::metadata(&path).await.is_err() {
        return Err(AppError::engine(format!(
            "cache LiDAR de la forêt absent : {}",
            path.display()
        )));
    }
    report_progress(progress, 84, "Masque forestier LiDAR mis en cache");
    Ok((path.clone(), format!("Cache LiDAR : {}\n", path.display())))
}

/// Indique si un export STL peut être sauvé en reconstruisant les routes en rubans segmentés.
fn is_road_topology_error(request: &ExportRequest, arguments: &[String], error: &AppError) -> bool {
    let message = error.to_string();
    matches!(request.format, ExportFormat::Stl | ExportFormat::StlLayers)
        && arguments.iter().any(|argument| argument == "--roads")
        && (message.contains("maillage de route non manifold")
            || message.contains("chaussée trop dense"))
}

/// Retire le drapeau des routes et ses deux réglages associés avant une nouvelle tentative.
fn without_road_arguments(arguments: &[String]) -> Vec<String> {
    let mut filtered = Vec::with_capacity(arguments.len());
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--roads" => index += 1,
            "--road-min-width-mm" | "--road-height-mm" => index += 2,
            _ => {
                filtered.push(arguments[index].clone());
                index += 1;
            }
        }
    }
    filtered
}

/// Convertit les alias courts en valeur reconnue par le moteur actuel.
fn legacy_crs(area: &crate::domain::Area) -> &str {
    if area.is_wgs84() {
        "latlon"
    } else {
        area.crs()
    }
}

/// Télécharge le cadastre par carrés bornés afin d'éviter les expirations du WFS IGN.
async fn download_cadastre_in_chunks(
    data_root: PathBuf,
    request: &DownloadRequest,
    cancellation: CancellationToken,
    progress: &ProgressSender,
    start_percent: u8,
    end_percent: u8,
) -> AppResult<String> {
    let chunks = cadastre_chunks(&request.area);
    let total = chunks.len().max(1);
    let mut combined_log =
        format!("## CADASTRE — emprise découpée en {total} carré(s) de 5 km maximum\n");
    for (index, (center, side)) in chunks.into_iter().enumerate() {
        let percent = start_percent.saturating_add(
            ((usize::from(end_percent.saturating_sub(start_percent)) * index) / total) as u8,
        );
        report_progress(
            progress,
            percent,
            format!("CADASTRE · secteur {}/{total}", index + 1),
        );
        let mut attempt = 1_u8;
        loop {
            let mut chunk_request = request.clone();
            chunk_request.area.center = center;
            chunk_request.area.size_m = side;
            chunk_request.force = request.force && attempt == 1;
            match run_engine_download(
                "CADASTRE",
                &chunk_request,
                &data_root,
                cancellation.clone(),
                progress,
                percent,
                end_percent,
            )
            .await
            {
                Ok(output) => {
                    combined_log.push_str(&format!(
                        "### Secteur {}/{} — tentative {attempt}\n{output}\n",
                        index + 1,
                        total
                    ));
                    break;
                }
                Err(error) if is_retryable_download_error(&error) && attempt < 4 => {
                    combined_log.push_str(&format!(
                        "### Secteur {}/{} — tentative {attempt} incomplète\n{error}\n",
                        index + 1,
                        total
                    ));
                    attempt += 1;
                    report_progress(
                        progress,
                        percent,
                        format!(
                            "CADASTRE · secteur {}/{} · reprise {attempt}/4",
                            index + 1,
                            total
                        ),
                    );
                    sleep(Duration::from_secs(u64::from(attempt))).await;
                }
                Err(error) => return Err(error),
            }
        }
    }
    report_progress(
        progress,
        end_percent,
        "CADASTRE · tous les secteurs sont disponibles",
    );
    Ok(combined_log)
}

/// Calcule les centres de carrés contigus, en GPS ou dans le repère projeté demandé.
fn cadastre_chunks(area: &crate::domain::Area) -> Vec<([f64; 2], f64)> {
    let columns = (area.size_m / CADASTRE_WFS_CHUNK_SIZE_M).ceil() as usize;
    let side = area.size_m / columns.max(1) as f64;
    let half = area.size_m / 2.0;
    let latitude_radians = area.center[0].to_radians();
    let longitude_meters = 111_320.0 * latitude_radians.cos().abs().max(0.01);
    let mut chunks = Vec::with_capacity(columns * columns);
    for row in 0..columns {
        for column in 0..columns {
            let offset_x = -half + side * (column as f64 + 0.5);
            let offset_y = -half + side * (row as f64 + 0.5);
            let center = if area.is_wgs84() {
                [
                    area.center[0] + offset_y / 111_320.0,
                    area.center[1] + offset_x / longitude_meters,
                ]
            } else {
                [area.center[0] + offset_x, area.center[1] + offset_y]
            };
            chunks.push((center, side));
        }
    }
    chunks
}

/// Reconnaît les échecs réseau pour lesquels une reprise bénéficie du cache déjà rempli.
fn is_retryable_download_error(error: &AppError) -> bool {
    let message = error.to_string();
    message.contains("tuile(s) n'ont pas pu être téléchargées")
        || message.contains("dalle(s) LiDAR n'ont pas pu être téléchargées")
        || message.contains("error reading a body from connection")
        || message.contains("HTTP status client error")
        || message.contains("HTTP status server error")
        || message.contains("erreur HTTP WFS")
        || message.contains("Gateway Time-out")
        || message.contains("timed out")
}

/// Accepte quelques trous d'orthophoto après les reprises sans perdre toute la zone.
fn tolerable_orthophoto_gaps(dataset: &str, error: &AppError) -> Option<(usize, usize)> {
    if dataset != "BD_ORTHO" {
        return None;
    }
    let message = error.to_string();
    let missing = metric_from_log(&message, "Échecs")?;
    let total = metric_from_log(&message, "Tuiles")?;
    let limit = total.div_ceil(50).max(1);
    (missing <= limit).then_some((missing, total))
}

/// Lit une ligne numérique du journal historique, quels que soient ses espacements.
fn metric_from_log(log: &str, label: &str) -> Option<usize> {
    log.lines().find_map(|line| {
        let label_start = line.find(label)?;
        let remainder = &line[label_start + label.len()..];
        let (_, value) = remainder.split_once(':')?;
        value.trim().parse::<usize>().ok()
    })
}

/// Formate une durée sans exposer les unités techniques du chronomètre.
fn readable_duration(duration: std::time::Duration) -> String {
    let seconds = duration.as_secs_f64();
    if seconds >= 60.0 {
        format!(
            "{} min {:.1} s",
            (seconds / 60.0).floor() as u64,
            seconds % 60.0
        )
    } else {
        format!("{seconds:.1} s")
    }
}

/// Retourne l'heure locale avec son décalage afin de comparer le journal à l'interface.
fn local_timestamp() -> String {
    Local::now().to_rfc3339_opts(SecondsFormat::Secs, false)
}

/// Exécute le mode moteur du binaire courant et coupe l’enfant lors d’une annulation.
async fn run_command(
    program: &Path,
    arguments: &[String],
    cancellation: CancellationToken,
) -> AppResult<String> {
    let started_at = local_timestamp();
    let started = Instant::now();
    let mut command = Command::new(program);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = tokio::select! {
        result = command.output() => result.map_err(|error| {
            AppError::engine(format!("lancement du moteur intégré : {error}"))
        })?,
        _ = cancellation.cancelled() => {
            return Err(AppError::Conflict("travail annulé".to_owned()));
        }
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let executable = program
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("geo-worker");
    let log = format!(
        "[{started_at}] [PROCESSUS] Démarrage de {executable}\n{stdout}{stderr}\n[{}] [PROCESSUS] Fin de {executable} · durée {}\n",
        local_timestamp(),
        readable_duration(started.elapsed())
    );
    if !output.status.success() {
        return Err(AppError::engine(format!(
            "moteur intégré terminé avec {}\n{log}",
            output.status
        )));
    }
    Ok(log)
}

/// Suit une sortie de processus ligne par ligne pour alimenter le dashboard pendant son exécution.
async fn run_command_streaming<F>(
    program: &Path,
    arguments: &[String],
    cancellation: CancellationToken,
    mut on_line: F,
) -> AppResult<String>
where
    F: FnMut(&str),
{
    if tokio::fs::metadata(program).await.is_err() {
        return Err(AppError::engine(format!(
            "exécutable absent : {}",
            program.display()
        )));
    }
    let started_at = local_timestamp();
    let started = Instant::now();
    let mut command = Command::new(program);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|error| {
        AppError::engine(format!("lancement de {} : {error}", program.display()))
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::engine("sortie standard indisponible"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| AppError::engine("sortie d'erreur indisponible"))?;
    let (line_sender, mut line_receiver) = mpsc::unbounded_channel();
    let stdout_task = tokio::spawn(forward_process_lines(stdout, false, line_sender.clone()));
    let stderr_task = tokio::spawn(forward_process_lines(stderr, true, line_sender.clone()));
    drop(line_sender);
    let mut stdout_log = String::new();
    let mut stderr_log = String::new();
    let status = loop {
        tokio::select! {
            _ = cancellation.cancelled() => {
                return Err(AppError::Conflict("travail annulé".to_owned()));
            }
            result = child.wait() => {
                break result.map_err(|error| {
                    AppError::engine(format!("attente de {} : {error}", program.display()))
                })?;
            }
            Some((is_stderr, line)) = line_receiver.recv() => {
                on_line(&line);
                if is_stderr {
                    stderr_log.push_str(&line);
                    stderr_log.push('\n');
                } else {
                    stdout_log.push_str(&line);
                    stdout_log.push('\n');
                }
            }
        }
    };
    let _ = stdout_task.await;
    let _ = stderr_task.await;
    while let Ok((is_stderr, line)) = line_receiver.try_recv() {
        on_line(&line);
        if is_stderr {
            stderr_log.push_str(&line);
            stderr_log.push('\n');
        } else {
            stdout_log.push_str(&line);
            stdout_log.push('\n');
        }
    }
    let ended_at = local_timestamp();
    let executable = program
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("processus");
    let log = format!(
        "[{started_at}] [PROCESSUS] Démarrage de {executable}\n{stdout_log}{stderr_log}\n[{ended_at}] [PROCESSUS] Fin de {executable} · durée {}\n",
        readable_duration(started.elapsed())
    );
    if !status.success() {
        return Err(AppError::engine(format!(
            "{} a terminé avec {}\n{}",
            program.display(),
            status,
            log
        )));
    }
    Ok(log)
}

/// Transmet chaque ligne d'un flux sans attendre la fin du téléchargement complet.
async fn forward_process_lines<R>(
    reader: R,
    is_stderr: bool,
    sender: mpsc::UnboundedSender<(bool, String)>,
) -> std::io::Result<()>
where
    R: AsyncRead + Unpin,
{
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await? {
        if sender.send((is_stderr, line)).is_err() {
            break;
        }
    }
    Ok(())
}

/// Rend visibles les sous-étapes du post-traitement Blender pendant qu'elles s'exécutent.
#[derive(Default)]
struct BlenderPostprocessProgress {
    last_percent: u8,
}

impl BlenderPostprocessProgress {
    /// Transmet les jalons horodatés du script Python au dashboard et à la console serveur.
    fn observe(&mut self, line: &str, progress: &ProgressSender) {
        if !line.contains("] [BLENDER] ") {
            return;
        }
        println!("{line}");
        let message = line
            .split_once("] [BLENDER] ")
            .map_or(line, |(_, message)| message);
        let phase_percent = if message.contains("Atlas orthophoto") {
            87
        } else if message.contains("Optimisation de l'orthophoto terminée") {
            89
        } else if message.contains("Toits :") || message.contains("Bâtiments :") {
            90
        } else if message.contains("Matériaux des bâtiments terminés") {
            91
        } else if message.contains("Végétation terminée") {
            92
        } else if message.contains("Fichier Blender enregistré") {
            93
        } else {
            86
        };
        let percent = phase_percent.max(self.last_percent);
        self.last_percent = percent;
        report_progress(progress, percent, message);
    }
}

/// Sérialise un manifeste avec une mise en forme adaptée à sa consultation humaine.
async fn write_json(path: &Path, value: &impl Serialize) -> AppResult<()> {
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|error| AppError::storage(error.to_string()))?;
    tokio::fs::write(path, bytes).await?;
    Ok(())
}

/// Construit les métadonnées après avoir vérifié la présence du fichier.
async fn artifact(job: &Job, path: &Path, kind: ArtifactKind) -> AppResult<Artifact> {
    let metadata = tokio::fs::metadata(path).await?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AppError::storage("nom de fichier non UTF-8"))?
        .to_owned();
    Ok(Artifact {
        id: uuid::Uuid::new_v4().to_string(),
        job_id: job.id.clone(),
        media_type: mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string(),
        name,
        kind,
        size_bytes: metadata.len(),
        created_at: Utc::now(),
        path: path.to_string_lossy().into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{CacheWarmupConfig, RenderConfig, ServerConfig, StorageConfig, WorkerConfig},
        domain::{AdvancedExport, Area, ExportQuality, TreeShape},
    };

    /// Construit une configuration minimale sans toucher au moteur historique.
    fn test_config(root: &Path) -> AppConfig {
        AppConfig {
            server: ServerConfig {
                bind: "127.0.0.1:0".parse().expect("adresse"),
            },
            storage: StorageConfig {
                root: root.join("storage"),
            },
            workers: WorkerConfig {
                downloads: 1,
                exports: 1,
            },
            cache_warmup: CacheWarmupConfig::default(),
            render: RenderConfig::default(),
            source_path: root.join("geo.toml"),
        }
    }

    /// Construit une requête STL suffisante pour vérifier la traduction historique.
    fn stl_request() -> ExportRequest {
        ExportRequest {
            area: Area {
                center: [48.0, 4.0],
                size_m: 1_000.0,
                advanced: None,
            },
            format: ExportFormat::Stl,
            layers: vec![DataLayer::Lidar],
            quality: ExportQuality::Standard,
            output_name: Some("modele.stl".to_owned()),
            advanced: AdvancedExport {
                tree_shape: Some(TreeShape::Conifer),
                ..AdvancedExport::default()
            },
            force: false,
        }
    }

    /// Empêche la régression où le lecteur TOML rejetait un export STL avant son lancement.
    #[test]
    fn stl_uses_blender_placeholder_but_keeps_real_output_argument() {
        let root = PathBuf::from("test-root");
        let config = test_config(&root);
        let request = stl_request();
        let job_root = root.join("job");
        let output = job_root.join("modele.stl");
        let legacy = legacy_export_config(&request, &config, &job_root, &output);
        assert!(
            !legacy.view.collapse_outliner,
            "la finition cosmétique ne doit pas relancer une seconde instance Blender"
        );
        assert_eq!(
            legacy
                .paths
                .output_file
                .extension()
                .and_then(|value| value.to_str()),
            Some("blend")
        );
        let arguments = export_command(&request, Path::new("engine.toml"), &output);
        assert!(
            arguments
                .windows(2)
                .any(|values| { values[0] == "--output" && values[1].ends_with("modele.stl") })
        );
        assert!(
            arguments
                .windows(2)
                .any(|values| { values[0] == "--tree-shape" && values[1] == "conifer" })
        );
    }

    /// La canopée est ajoutée depuis le cache LiDAR et ne doit pas doubler les arbres historiques.
    #[test]
    fn stl_canopy_replaces_individual_tree_arguments() {
        let mut request = stl_request();
        request.advanced.tree_shape = Some(TreeShape::Canopy);
        let arguments = export_command(&request, Path::new("engine.toml"), Path::new("canopy.stl"));

        assert!(uses_stl_canopy(&request));
        assert!(!arguments.iter().any(|argument| argument == "--trees"));
        assert!(!arguments.iter().any(|argument| argument == "--tree-shape"));
    }

    /// Vérifie que la reprise d'un STL ne conserve aucune option routière orpheline.
    #[test]
    fn road_retry_removes_all_road_arguments() {
        let arguments = vec![
            "--output".to_owned(),
            "model.stl".to_owned(),
            "--roads".to_owned(),
            "--road-min-width-mm".to_owned(),
            "1".to_owned(),
            "--road-height-mm".to_owned(),
            "0.6".to_owned(),
            "--trees".to_owned(),
        ];

        assert_eq!(
            without_road_arguments(&arguments),
            vec![
                "--output".to_owned(),
                "model.stl".to_owned(),
                "--trees".to_owned(),
            ]
        );
    }

    /// Une chaussée trop dense utilise la même reconstruction bornée qu'un maillage non manifold.
    #[test]
    fn dense_stl_roads_trigger_segmented_repair() {
        let request = stl_request();
        let arguments = vec!["--roads".to_owned()];
        let error = AppError::engine(
            "chaussée trop dense : augmenter vectors.max_road_vertices ou line_sample_step_m",
        );

        assert!(is_road_topology_error(&request, &arguments, &error));
    }

    /// Une poignée de tuiles IGN absentes ne doit plus annuler des milliers de réussites.
    #[test]
    fn small_orthophoto_gap_is_tolerated_after_retries() {
        let error = AppError::engine(
            "Tuiles        : 3721\nTéléchargées   : 3718\nÉchecs         : 3\n\
             Error: 3 tuile(s) n'ont pas pu être téléchargées",
        );

        assert_eq!(
            tolerable_orthophoto_gaps("BD_ORTHO", &error),
            Some((3, 3721))
        );
        assert_eq!(tolerable_orthophoto_gaps("LIDAR_HD", &error), None);
    }

    /// Une couche largement incomplète reste une erreur explicite.
    #[test]
    fn large_orthophoto_gap_still_fails() {
        let error = AppError::engine(
            "Tuiles        : 1000\nTéléchargées   : 950\nÉchecs         : 50\n\
             Error: 50 tuile(s) n'ont pas pu être téléchargées",
        );

        assert_eq!(tolerable_orthophoto_gaps("BD_ORTHO", &error), None);
    }

    /// La limite grandit avec la qualité, puis le pas protège les zones vraiment immenses.
    #[test]
    fn terrain_plan_is_automatically_bounded_by_vertex_limit() {
        let (regular_step, regular_limit) = automatic_terrain_plan(3_000.0, 5.0);
        let regular_axis = (3_000.0 / regular_step).ceil() as usize + 1;
        let (large_step, large_limit) = automatic_terrain_plan(10_000.0, 5.0);
        let large_axis = (10_000.0 / large_step).ceil() as usize + 1;

        assert_eq!(regular_step, 5.0);
        assert!(regular_limit >= regular_axis * regular_axis);
        assert!(regular_limit <= TERRAIN_HARD_MAX_VERTICES);
        assert!(large_step > 5.0);
        assert!(large_axis * large_axis <= large_limit);
        assert_eq!(large_limit, TERRAIN_HARD_MAX_VERTICES);
    }

    /// Le curseur direct doit remplacer le preset sans exposer la limite de sommets.
    #[test]
    fn explicit_terrain_grid_step_reaches_legacy_engine() {
        let root = PathBuf::from("test-root");
        let config = test_config(&root);
        let mut request = stl_request();
        request.advanced.terrain_grid_step_m = Some(75.0);
        let legacy = legacy_export_config(
            &request,
            &config,
            Path::new("job"),
            Path::new("job/model.stl"),
        );

        assert_eq!(legacy.terrain.grid_step_m, 75.0);
    }

    /// Le réglage Blender est la seule hauteur de route : aucun ancien +0,20 m caché.
    #[test]
    fn blender_road_offset_starts_at_the_terrain_surface() {
        let root = PathBuf::from("test-root");
        let config = test_config(&root);
        let mut request = stl_request();
        request.format = ExportFormat::Blender;
        request.advanced.road_elevation_m = Some(0.0);
        let legacy = legacy_export_config(
            &request,
            &config,
            Path::new("job"),
            Path::new("job/model.blend"),
        );

        assert_eq!(
            legacy
                .layers
                .get("bd_topo.troncon_de_route")
                .expect("couche route")
                .z_offset_m,
            Some(0.0)
        );
    }

    /// Les finitions visuelles réutilisent la base, contrairement à un changement de terrain.
    #[test]
    fn blender_base_cache_ignores_postprocess_options_only() {
        assert!(may_restore_blender_base(false, true));
        assert!(!may_restore_blender_base(true, true));

        let mut first = stl_request();
        first.format = ExportFormat::Blender;
        first.output_name = Some("premier.blend".to_owned());
        first.advanced.tree_shape = Some(TreeShape::Canopy);
        first.advanced.forest_quality = Some(ExportQuality::Draft);
        first.advanced.forest_material_mode = Some(ForestMaterialMode::Texture);

        let mut restyled = first.clone();
        restyled.output_name = Some("second.blend".to_owned());
        restyled.force = true;
        restyled.advanced.forest_quality = Some(ExportQuality::High);
        restyled.advanced.forest_material_mode = Some(ForestMaterialMode::Procedural);
        restyled.advanced.keep_isolated_trees = Some(true);
        restyled.advanced.road_elevation_m = Some(0.35);

        assert_eq!(
            blender_base_cache_key(&first, "sources").expect("clé"),
            blender_base_cache_key(&restyled, "sources").expect("clé")
        );

        restyled.advanced.terrain_grid_step_m = Some(50.0);
        assert_ne!(
            blender_base_cache_key(&first, "sources").expect("clé"),
            blender_base_cache_key(&restyled, "sources").expect("clé")
        );
    }

    /// La canopée continue ne doit pas transformer cultures et végétation basse en forêt.
    #[test]
    fn blender_canopy_uses_only_high_vegetation_lidar_class() {
        let root = PathBuf::from("test-root");
        let config = test_config(&root);
        let mut canopy = stl_request();
        canopy.format = ExportFormat::Blender;
        canopy.advanced.tree_shape = Some(TreeShape::Canopy);
        let legacy = legacy_export_config(
            &canopy,
            &config,
            Path::new("job"),
            Path::new("job/canopy.blend"),
        );

        assert_eq!(legacy.vegetation.classes, vec![5]);

        let mut individual_trees = canopy.clone();
        individual_trees.advanced.tree_shape = Some(TreeShape::Deciduous);
        let individual_legacy = legacy_export_config(
            &individual_trees,
            &config,
            Path::new("job"),
            Path::new("job/trees.blend"),
        );
        assert_eq!(individual_legacy.vegetation.classes, vec![3, 4, 5]);
        assert_ne!(
            blender_base_cache_key(&canopy, "sources").expect("clé"),
            blender_base_cache_key(&individual_trees, "sources").expect("clé")
        );
    }

    /// La modification d'une entrée géographique change l'empreinte des caches Blender.
    #[test]
    fn blender_source_fingerprint_changes_with_input_files() {
        let directory =
            tempfile::tempdir_in(std::env::current_dir().expect("répertoire courant disponible"))
                .expect("dossier temporaire");
        let source_root = directory.path().join("data");
        std::fs::create_dir_all(source_root.join("BD_ORTHO")).expect("dossier orthophoto");
        let first = blender_source_fingerprint(&source_root).expect("première empreinte");
        std::fs::create_dir_all(source_root.join("LIDAR_HD/STL_CACHE")).expect("cache STL");
        std::fs::write(
            source_root.join("LIDAR_HD/STL_CACHE/surface.gstl"),
            b"derived",
        )
        .expect("surface précalculée");
        assert_eq!(
            blender_source_fingerprint(&source_root).expect("empreinte après cache dérivé"),
            first
        );
        std::fs::write(source_root.join("BD_ORTHO/tile.jpg"), b"new source").expect("tuile");
        let second = blender_source_fingerprint(&source_root).expect("seconde empreinte");

        assert_ne!(first, second);
    }

    /// Une grande emprise cadastrale devient plusieurs requêtes bornées et contiguës.
    #[test]
    fn large_cadastre_area_is_split_into_five_kilometer_chunks() {
        let area = Area {
            center: [45.771, 2.963],
            size_m: 20_000.0,
            advanced: None,
        };
        let chunks = cadastre_chunks(&area);

        assert_eq!(chunks.len(), 16);
        assert!(chunks.iter().all(|(_, side)| *side <= 5_000.0));
        let average_latitude =
            chunks.iter().map(|(center, _)| center[0]).sum::<f64>() / chunks.len() as f64;
        let average_longitude =
            chunks.iter().map(|(center, _)| center[1]).sum::<f64>() / chunks.len() as f64;
        assert!((average_latitude - area.center[0]).abs() < 1e-10);
        assert!((average_longitude - area.center[1]).abs() < 1e-10);
    }

    /// Les réponses 5xx du WFS doivent déclencher une reprise, pas faire échouer toute la zone.
    #[test]
    fn wfs_gateway_timeout_is_retryable() {
        let error =
            AppError::engine("erreur HTTP WFS: HTTP status server error (504 Gateway Time-out)");

        assert!(is_retryable_download_error(&error));
    }
}
