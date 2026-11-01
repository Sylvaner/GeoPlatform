//! Contexte : options courtes des deux exécutables locaux.
//! Usage : convertir les arguments en contrats identiques à ceux de l'API.

use std::path::PathBuf;

use clap::Parser;

use crate::{
    AppConfig, AppService,
    domain::{
        AdvancedExport, Area, DataLayer, DownloadRequest, ExportFormat, ExportQuality,
        ExportRequest, Job, JobStatus, TreeShape,
    },
    error::{AppError, AppResult},
};

/// Arguments du téléchargeur local.
#[derive(Debug, Parser)]
#[command(
    name = "geo-data",
    about = "Télécharge et met en cache des données géographiques"
)]
pub struct DownloadCli {
    /// Nom lisible affiché sur la carte.
    #[arg(long)]
    pub name: Option<String>,
    /// Centre GPS : latitude puis longitude.
    #[arg(long, required = true, num_args = 2, allow_hyphen_values = true, value_names = ["LAT", "LON"])]
    pub center: Vec<f64>,
    /// Longueur du côté du carré en mètres.
    #[arg(long)]
    pub size: f64,
    /// Couches séparées par des virgules. L'absence signifie toutes les couches.
    #[arg(long, value_delimiter = ',')]
    pub layers: Vec<DataLayer>,
    /// Ignore le résultat final du cache.
    #[arg(long)]
    pub force: bool,
    /// Configuration du serveur local.
    #[arg(long, default_value = "geo.toml")]
    pub config: PathBuf,
}

impl DownloadCli {
    /// Produit le même JSON métier qu'un appel HTTP.
    pub fn request(&self) -> AppResult<DownloadRequest> {
        Ok(DownloadRequest {
            name: self.name.clone(),
            area: simple_area(&self.center, self.size)?,
            layers: self.layers.clone(),
            force: self.force,
        })
    }
}

/// Arguments communs aux trois formats d'export.
#[derive(Debug, Parser)]
#[command(
    name = "geo-export",
    about = "Exporte le cache en STL, STL par couches ou Blender"
)]
pub struct ExportCli {
    /// Centre GPS : latitude puis longitude.
    #[arg(long, required = true, num_args = 2, allow_hyphen_values = true, value_names = ["LAT", "LON"])]
    pub center: Vec<f64>,
    /// Longueur du côté du carré en mètres.
    #[arg(long)]
    pub size: f64,
    /// Format final.
    #[arg(long, value_enum, required = false)]
    pub format: Option<ExportFormat>,
    /// Couches séparées par des virgules. L'absence laisse le format choisir.
    #[arg(long, value_delimiter = ',')]
    pub layers: Vec<DataLayer>,
    /// Compromis global entre vitesse et précision.
    #[arg(long, value_enum, default_value_t = ExportQuality::Standard)]
    pub quality: ExportQuality,
    /// Nom du fichier final, sans répertoire.
    #[arg(long)]
    pub output: Option<String>,
    /// Largeur physique du STL en millimètres.
    #[arg(long)]
    pub width_mm: Option<f64>,
    /// Hauteur réelle d'une strate, uniquement pour stl_layers.
    #[arg(long)]
    pub layer_height_m: Option<f64>,
    /// Silhouette des arbres imprimés : feuillu ou conifère.
    #[arg(long, value_enum)]
    pub tree_shape: Option<TreeShape>,
    /// Ignore le résultat final du cache.
    #[arg(long)]
    pub force: bool,
    /// Configuration du serveur local.
    #[arg(long, default_value = "geo.toml")]
    pub config: PathBuf,
}

impl ExportCli {
    /// Produit le même JSON métier qu'un appel HTTP.
    pub fn request(&self) -> AppResult<ExportRequest> {
        Ok(ExportRequest {
            area: simple_area(&self.center, self.size)?,
            format: self.format.unwrap_or(ExportFormat::Blender),
            layers: self.layers.clone(),
            quality: self.quality,
            output_name: self.output.clone(),
            advanced: AdvancedExport {
                width_mm: self.width_mm,
                base_mm: None,
                layer_height_m: self.layer_height_m,
                vertical_exaggeration: None,
                tree_shape: self.tree_shape,
                ..AdvancedExport::default()
            },
            force: self.force,
        })
    }
}

/// Exécute localement un téléchargement dans les mêmes workers que le serveur.
pub async fn run_download(arguments: DownloadCli) -> AppResult<Job> {
    let config = AppConfig::load(&arguments.config)?;
    let service = AppService::start(config).await?;
    let job = service.submit_download(arguments.request()?).await?;
    finish(&service, job).await
}

/// Exécute localement un export dans les mêmes workers que le serveur.
pub async fn run_export(arguments: ExportCli) -> AppResult<Job> {
    let config = AppConfig::load(&arguments.config)?;
    let service = AppService::start(config).await?;
    let job = service.submit_export(arguments.request()?).await?;
    finish(&service, job).await
}

/// Attend la fin puis transforme un échec asynchrone en code d'erreur de la CLI.
async fn finish(service: &AppService, job: Job) -> AppResult<Job> {
    let job = service.wait_for_job(&job.id).await?;
    match job.status {
        JobStatus::Completed => Ok(job),
        JobStatus::Failed | JobStatus::Cancelled => Err(AppError::engine(job.message)),
        JobStatus::Queued | JobStatus::Running => {
            unreachable!("wait_for_job attend un état terminal")
        }
    }
}

/// Valide le nombre de coordonnées avant de créer la zone simple.
fn simple_area(center: &[f64], size_m: f64) -> AppResult<Area> {
    let center: [f64; 2] = center
        .try_into()
        .map_err(|_| AppError::validation("--center attend exactement deux valeurs"))?;
    Ok(Area {
        center,
        size_m,
        advanced: None,
    })
}
