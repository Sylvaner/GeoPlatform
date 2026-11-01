//! Contexte : API HTTP v1 consommée par le futur front et par d'autres automatisations.
//! Usage : toutes les routes longues créent un job et répondent immédiatement.

use std::sync::Arc;

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_util::io::ReaderStream;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::{
    AppConfig, AppService, dashboard,
    domain::{
        Artifact, DataLayer, DownloadRequest, ExportPreset, ExportPresetDraft, ExportRequest, Job,
    },
    error::{AppError, AppResult},
    logs::ServerLogEntry,
    map,
};

/// État cloné à faible coût par Axum pour chaque requête.
#[derive(Clone)]
struct ApiState {
    service: Arc<AppService>,
}

/// Construit le routeur complet avec une version explicite du contrat.
pub fn router(service: Arc<AppService>) -> Router {
    let state = ApiState { service };
    Router::new()
        .route("/", get(frontend_index))
        .route("/assets/app.css", get(frontend_css))
        .route("/assets/app.js", get(frontend_javascript))
        .route("/assets/export-wizard.js", get(frontend_export_wizard))
        .route(
            "/assets/export-preview-3d.js",
            get(frontend_export_preview_3d),
        )
        .route("/assets/stl-viewer.js", get(frontend_stl_viewer))
        .route("/assets/vendor/three.module.js", get(frontend_three_module))
        .route("/assets/vendor/three.core.js", get(frontend_three_core))
        .route("/favicon.svg", get(frontend_favicon))
        .route("/api/v1", get(api_root))
        .route("/api/v1/health", get(health))
        .route("/api/v1/openapi.json", get(openapi))
        .route("/api/v1/layers", get(layers))
        .route("/api/v1/downloads", post(create_download))
        .route("/api/v1/zones/{id}", delete(delete_zone).put(rename_zone))
        .route("/api/v1/exports", post(create_export))
        .route(
            "/api/v1/export-presets",
            get(list_export_presets).post(create_export_preset),
        )
        .route(
            "/api/v1/export-presets/{id}",
            axum::routing::delete(delete_export_preset),
        )
        .route("/api/v1/jobs", get(list_jobs))
        .route("/api/v1/jobs/{id}", get(get_job).delete(delete_job))
        .route("/api/v1/jobs/{id}/cancel", post(cancel_job))
        .route("/api/v1/logs", get(list_server_logs))
        .route("/api/v1/files", get(list_files))
        .route("/api/v1/files/{id}", get(get_file).delete(delete_file))
        .route("/api/v1/files/{id}/content", get(file_content))
        .route("/api/v1/dashboard", get(get_dashboard))
        .route("/api/v1/map", get(get_map))
        .route("/api/v1/config", get(get_config).put(update_config))
        .with_state(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}

/// Sert la page cartographique sans ajouter un second serveur au projet.
async fn frontend_index() -> Html<&'static str> {
    Html(include_str!("../../web/index.html"))
}

/// Sert la feuille de style compilée directement dans l'exécutable.
async fn frontend_css() -> Response {
    static_asset("text/css; charset=utf-8", include_str!("../../web/app.css"))
}

/// Sert le contrôleur de l'interface avec un type MIME explicite.
async fn frontend_javascript() -> Response {
    static_asset(
        "text/javascript; charset=utf-8",
        include_str!("../../web/app.js"),
    )
}

/// Sert le wizard d'export séparément pour garder le contrôleur de carte lisible.
async fn frontend_export_wizard() -> Response {
    static_asset(
        "text/javascript; charset=utf-8",
        include_str!("../../web/export-wizard.js"),
    )
}

/// Sert le rendu Three.js séparément du wizard afin de garder chaque responsabilité lisible.
async fn frontend_export_preview_3d() -> Response {
    static_asset(
        "text/javascript; charset=utf-8",
        include_str!("../../web/export-preview-3d.js"),
    )
}

/// Sert le visualiseur STL local sans dépendance JavaScript externe.
async fn frontend_stl_viewer() -> Response {
    static_asset(
        "text/javascript; charset=utf-8",
        include_str!("../../web/stl-viewer.js"),
    )
}

/// Sert le module Three.js localement pour que l'aperçu reste disponible hors connexion.
async fn frontend_three_module() -> Response {
    static_asset(
        "text/javascript; charset=utf-8",
        include_str!("../../web/vendor/three.module.js"),
    )
}

/// Sert le cœur importé par le module Three.js local.
async fn frontend_three_core() -> Response {
    static_asset(
        "text/javascript; charset=utf-8",
        include_str!("../../web/vendor/three.core.js"),
    )
}

/// Sert la petite identité visuelle utilisée par les onglets du navigateur.
async fn frontend_favicon() -> Response {
    static_asset("image/svg+xml", include_str!("../../web/favicon.svg"))
}

/// Construit une réponse statique légère avec son type de contenu.
fn static_asset(content_type: &'static str, content: &'static str) -> Response {
    let mut response = Response::new(Body::from(content));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
}

/// Présente le service sans imposer une interface HTML provisoire.
async fn api_root() -> Json<Value> {
    Json(json!({
        "name": "geo-platform",
        "api": "/api/v1",
        "health": "/api/v1/health",
        "documentation": "/api/v1/openapi.json"
    }))
}

/// Confirme que le processus HTTP et son stockage ont été initialisés.
async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

/// Retourne une description OpenAPI minimale et directement exploitable.
async fn openapi() -> Json<Value> {
    Json(json!({
        "openapi": "3.1.0",
        "info": { "title": "Geo Platform API", "version": env!("CARGO_PKG_VERSION") },
        "paths": {
            "/api/v1/downloads": { "post": { "summary": "Planifier un téléchargement", "responses": { "202": { "description": "Job créé" } } } },
            "/api/v1/zones/{id}": {
                "put": { "summary": "Renommer une zone sans modifier ses données" },
                "delete": { "summary": "Supprimer une zone et ses extractions privées, sans toucher aux tuiles partagées" }
            },
            "/api/v1/exports": { "post": { "summary": "Planifier un export", "responses": { "202": { "description": "Job créé" } } } },
            "/api/v1/export-presets": { "get": { "summary": "Lister les profils d’export" }, "post": { "summary": "Sauvegarder un profil d’export" } },
            "/api/v1/export-presets/{id}": { "delete": { "summary": "Supprimer un profil d’export" } },
            "/api/v1/jobs": { "get": { "summary": "Lister les jobs" } },
            "/api/v1/jobs/{id}": { "get": { "summary": "Lire un job" } },
            "/api/v1/logs": { "get": { "summary": "Lire le journal du serveur" } },
            "/api/v1/files": { "get": { "summary": "Lister les fichiers générés" } },
            "/api/v1/dashboard": { "get": { "summary": "Lire la synthèse du dashboard" } },
            "/api/v1/map": { "get": { "summary": "Lire les emprises en GeoJSON" } },
            "/api/v1/config": { "get": { "summary": "Lire la configuration" }, "put": { "summary": "Mettre à jour la configuration" } }
        }
    }))
}

/// Décrit les couches publiques et leur libellé sans exposer les services sources.
async fn layers() -> Json<Vec<LayerDescription>> {
    Json(
        DataLayer::ALL
            .into_iter()
            .map(|layer| LayerDescription {
                id: layer,
                label: layer.label(),
            })
            .collect(),
    )
}

/// Ajoute un téléchargement à sa file dédiée.
async fn create_download(
    State(state): State<ApiState>,
    Json(request): Json<DownloadRequest>,
) -> AppResult<impl IntoResponse> {
    let job = state.service.submit_download(request).await?;
    Ok((StatusCode::ACCEPTED, Json(job)))
}

/// Retire une zone terminée de la carte et conserve le cache de données partagé.
async fn delete_zone(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    state.service.delete_zone(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Modifie le libellé d'une zone sans recalcul ni déplacement de fichiers.
async fn rename_zone(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Json(request): Json<RenameZoneRequest>,
) -> AppResult<Json<Job>> {
    Ok(Json(state.service.rename_zone(&id, request.name).await?))
}

/// Corps volontairement minimal de l'action de renommage.
#[derive(Deserialize)]
struct RenameZoneRequest {
    name: String,
}

/// Ajoute un export à sa file dédiée.
async fn create_export(
    State(state): State<ApiState>,
    Json(request): Json<ExportRequest>,
) -> AppResult<impl IntoResponse> {
    let job = state.service.submit_export(request).await?;
    Ok((StatusCode::ACCEPTED, Json(job)))
}

/// Retourne les profils d'export réutilisables.
async fn list_export_presets(State(state): State<ApiState>) -> AppResult<Json<Vec<ExportPreset>>> {
    Ok(Json(state.service.list_export_presets().await?))
}

/// Sauvegarde un profil d'export nommé.
async fn create_export_preset(
    State(state): State<ApiState>,
    Json(draft): Json<ExportPresetDraft>,
) -> AppResult<impl IntoResponse> {
    let preset = state.service.create_export_preset(draft).await?;
    Ok((StatusCode::CREATED, Json(preset)))
}

/// Supprime un profil sans supprimer les fichiers générés avec lui.
async fn delete_export_preset(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    state.service.delete_export_preset(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Retourne l'historique des travaux.
async fn list_jobs(
    State(state): State<ApiState>,
    Query(query): Query<JobQuery>,
) -> AppResult<Json<Vec<Job>>> {
    let mut jobs = state.service.list_jobs().await?;
    if let Some(status) = query.status {
        jobs.retain(|job| job.status == status);
    }
    if let Some(kind) = query.kind {
        jobs.retain(|job| kind.matches(job));
    }
    jobs.truncate(query.limit.unwrap_or(100).min(1_000));
    Ok(Json(jobs))
}

/// Retourne un travail et sa configuration d'origine.
async fn get_job(State(state): State<ApiState>, Path(id): Path<String>) -> AppResult<Json<Job>> {
    Ok(Json(state.service.get_job(&id).await?))
}

/// Annule un travail en attente ou son processus enfant actif.
async fn cancel_job(State(state): State<ApiState>, Path(id): Path<String>) -> AppResult<Json<Job>> {
    Ok(Json(state.service.cancel_job(&id).await?))
}

/// Filtre facultatif du journal durable.
#[derive(Deserialize)]
struct LogQuery {
    limit: Option<usize>,
}

/// Retourne les dernières actions importantes du serveur.
async fn list_server_logs(
    State(state): State<ApiState>,
    Query(query): Query<LogQuery>,
) -> AppResult<Json<Vec<ServerLogEntry>>> {
    Ok(Json(
        state
            .service
            .list_server_logs(query.limit.unwrap_or(100))
            .await?,
    ))
}

/// Retourne tous les fichiers générés avec leur taille et leur type.
async fn list_files(State(state): State<ApiState>) -> AppResult<Json<Vec<Artifact>>> {
    Ok(Json(state.service.list_artifacts().await?))
}

/// Retourne les métadonnées d'un fichier sans charger son contenu.
async fn get_file(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> AppResult<Json<Artifact>> {
    Ok(Json(state.service.get_artifact(&id).await?))
}

/// Supprime un export terminé, son fichier et sa ligne d'historique.
async fn delete_job(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    state.service.delete_export(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Supprime le contenu demandé et invalide son résultat de cache.
async fn delete_file(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    state.service.delete_artifact(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Sert un gros fichier sous forme de flux pour garder la mémoire du serveur stable.
async fn file_content(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> AppResult<Response> {
    let artifact = state.service.get_artifact(&id).await?;
    let file = tokio::fs::File::open(&artifact.path)
        .await
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                AppError::not_found(format!("contenu du fichier {id}"))
            } else {
                AppError::from(error)
            }
        })?;
    let disposition = format!(
        "attachment; filename=\"{}\"",
        safe_download_name(&artifact.name)
    );
    let mut response = Response::new(Body::from_stream(ReaderStream::new(file)));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&artifact.media_type)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&disposition)
            .map_err(|_| AppError::storage("nom de fichier non représentable en HTTP"))?,
    );
    Ok(response)
}

/// Produit un nom ASCII sûr pour l'en-tête HTTP, sans modifier le fichier stocké.
fn safe_download_name(name: &str) -> String {
    name.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

/// Calcule à la demande la synthèse d'espace disque et d'activité.
async fn get_dashboard(State(state): State<ApiState>) -> AppResult<Json<dashboard::Dashboard>> {
    Ok(Json(dashboard::build(&state.service).await?))
}

/// Filtre facultatif pour n'afficher qu'une emprise.
#[derive(Deserialize)]
struct MapQuery {
    job_id: Option<String>,
    kind: Option<JobTypeFilter>,
    status: Option<crate::domain::JobStatus>,
    limit: Option<usize>,
}

/// Retourne une FeatureCollection directement affichable par MapLibre ou Leaflet.
async fn get_map(
    State(state): State<ApiState>,
    Query(query): Query<MapQuery>,
) -> AppResult<Json<Value>> {
    let mut jobs = if let Some(id) = query.job_id {
        vec![state.service.get_job(&id).await?]
    } else {
        state.service.list_jobs().await?
    };
    if let Some(kind) = query.kind {
        jobs.retain(|job| kind.matches(job));
    }
    if let Some(status) = query.status {
        jobs.retain(|job| job.status == status);
    }
    jobs.truncate(query.limit.unwrap_or(500).min(2_000));
    Ok(Json(map::jobs_geojson(&jobs)?))
}

/// Filtres légers de la liste d'activité.
#[derive(Deserialize)]
struct JobQuery {
    kind: Option<JobTypeFilter>,
    status: Option<crate::domain::JobStatus>,
    limit: Option<usize>,
}

/// Type de travail reconnu dans les chaînes de requête.
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum JobTypeFilter {
    Download,
    Export,
}

impl JobTypeFilter {
    /// Indique si un job appartient à la catégorie demandée.
    fn matches(self, job: &Job) -> bool {
        matches!(
            (self, &job.kind),
            (Self::Download, crate::domain::JobKind::Download(_))
                | (Self::Export, crate::domain::JobKind::Export(_))
        )
    }
}

/// Lit les paramètres actifs, chemins résolus compris.
async fn get_config(State(state): State<ApiState>) -> Json<AppConfig> {
    Json(state.service.configuration().await)
}

/// Sauvegarde la configuration et indique clairement qu'un redémarrage ajuste les pools.
async fn update_config(
    State(state): State<ApiState>,
    Json(config): Json<AppConfig>,
) -> AppResult<Json<ConfigUpdate>> {
    let config = state.service.update_configuration(config).await?;
    Ok(Json(ConfigUpdate {
        config,
        restart_required: true,
    }))
}

/// Élément de catalogue d'une couche.
#[derive(Serialize)]
struct LayerDescription {
    id: DataLayer,
    label: &'static str,
}

/// Réponse explicite de mise à jour de configuration.
#[derive(Serialize)]
struct ConfigUpdate {
    config: AppConfig,
    restart_required: bool,
}
