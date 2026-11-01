//! Contexte : configuration courte de l'infrastructure, séparée des requêtes métier.
//! Usage : charger `geo.toml` au démarrage ; les chemins relatifs partent de ce fichier.

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    domain::ExportColors,
    error::{AppError, AppResult},
};

/// Configuration complète du serveur et des CLI locales.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub storage: StorageConfig,
    pub workers: WorkerConfig,
    #[serde(default)]
    pub cache_warmup: CacheWarmupConfig,
    /// Valeurs visuelles utilisées par défaut quand un export ne fournit pas sa palette.
    #[serde(default)]
    pub render: RenderConfig,
    /// Emplacement d'origine, utilisé lors d'une mise à jour via l'API.
    #[serde(skip)]
    pub source_path: PathBuf,
}

/// Réglages réseau volontairement limités à l'adresse d'écoute.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    pub bind: SocketAddr,
}

/// Racine unique contenant cache, données, exports et états.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StorageConfig {
    pub root: PathBuf,
}

/// Taille des deux files de travail indépendantes.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerConfig {
    pub downloads: usize,
    pub exports: usize,
}

/// Préréglages de grilles STL dont la surface LiDAR doit être précalculée.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct CacheWarmupConfig {
    /// Une liste vide désactive le worker. Les valeurs sont des mètres par cellule.
    pub stl_grid_steps_m: Vec<f64>,
    /// Atlas Blender à préchauffer ; 8192 px peut demander environ 1 Gio de buffer.
    pub blender_atlas_resolutions_px: Vec<u32>,
}

impl Default for CacheWarmupConfig {
    fn default() -> Self {
        Self {
            stl_grid_steps_m: vec![50.0, 80.0, 100.0],
            blender_atlas_resolutions_px: Vec::new(),
        }
    }
}

/// Palette centrale utilisée à la fois par l'interface et les moteurs d'export.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RenderConfig {
    #[serde(default = "default_export_colors")]
    pub colors: ExportColors,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            colors: default_export_colors(),
        }
    }
}

fn default_export_colors() -> ExportColors {
    ExportColors {
        ground: Some([0.470_588, 0.607_843, 0.447_059, 1.0]),
        walls: Some([0.831_373, 0.803_922, 0.733_333, 1.0]),
        roofs: Some([0.721_569, 0.580_392, 0.384_314, 1.0]),
        trees: Some([0.247_059, 0.470_588, 0.337_255, 1.0]),
        roads: Some([0.105_882, 0.121_569, 0.113_725, 1.0]),
        water: Some([0.184_314, 0.560_784, 0.741_176, 1.0]),
        forest_low: Some([0.023_529, 0.121_569, 0.019_608, 1.0]),
        forest_high: Some([0.082_353, 0.419_608, 0.058_824, 1.0]),
    }
}

impl AppConfig {
    /// Lit le TOML, résout ses chemins et contrôle les bornes simples.
    pub fn load(path: &Path) -> AppResult<Self> {
        let text = std::fs::read_to_string(path).map_err(|error| {
            AppError::configuration(format!("lecture de {} : {error}", path.display()))
        })?;
        let mut config: Self = toml::from_str(&text).map_err(|error| {
            AppError::configuration(format!("TOML {} invalide : {error}", path.display()))
        })?;
        let source_path = absolute_path(path)?;
        let base = source_path.parent().unwrap_or_else(|| Path::new("."));
        config.storage.root = resolve(base, &config.storage.root);
        config.source_path = source_path;
        config.validate()?;
        Ok(config)
    }

    /// Vérifie que chaque file possède au moins un worker.
    pub fn validate(&self) -> AppResult<()> {
        if self.workers.downloads == 0 || self.workers.exports == 0 {
            return Err(AppError::configuration(
                "workers.downloads et workers.exports doivent être >= 1",
            ));
        }
        if self.cache_warmup.stl_grid_steps_m.len() > 8
            || self
                .cache_warmup
                .stl_grid_steps_m
                .iter()
                .any(|step| !step.is_finite() || !(5.0..=100.0).contains(step))
        {
            return Err(AppError::configuration(
                "cache_warmup.stl_grid_steps_m doit contenir au plus 8 pas entre 5 et 100 m",
            ));
        }
        let atlas_resolutions = &self.cache_warmup.blender_atlas_resolutions_px;
        if atlas_resolutions.len() > 4
            || atlas_resolutions.iter().any(|resolution| {
                *resolution < 512 || *resolution > 8192 || !resolution.is_multiple_of(512)
            })
            || atlas_resolutions
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != atlas_resolutions.len()
        {
            return Err(AppError::configuration(
                "cache_warmup.blender_atlas_resolutions_px accepte au plus 4 résolutions de 512 à 8192 px, par pas de 512",
            ));
        }
        self.render.colors.validate()?;
        Ok(())
    }

    /// Enregistre une version portable dont les chemins absolus restent explicites.
    pub fn save(&self) -> AppResult<()> {
        let text = toml::to_string_pretty(self)
            .map_err(|error| AppError::configuration(error.to_string()))?;
        std::fs::write(&self.source_path, text).map_err(|error| {
            AppError::configuration(format!(
                "écriture de {} : {error}",
                self.source_path.display()
            ))
        })
    }

    /// Chemin du cache géographique partagé par tous les traitements.
    pub fn data_root(&self) -> PathBuf {
        self.storage.root.join("data")
    }

    /// Chemin dans lequel les exports finaux sont regroupés par job.
    pub fn exports_root(&self) -> PathBuf {
        self.storage.root.join("exports")
    }
}

/// Résout un chemin relativement au fichier de configuration sans exiger qu'il existe.
fn resolve(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

/// Rend absolu le chemin du fichier en conservant un fichier encore inexistant.
fn absolute_path(path: &Path) -> AppResult<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    std::env::current_dir()
        .map(|current| current.join(path))
        .map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vérifie que les valeurs nulles ne peuvent pas désactiver silencieusement une file.
    #[test]
    fn worker_count_must_be_positive() {
        let config = AppConfig {
            server: ServerConfig {
                bind: "127.0.0.1:8080".parse().expect("adresse valide"),
            },
            storage: StorageConfig { root: "x".into() },
            workers: WorkerConfig {
                downloads: 0,
                exports: 1,
            },
            cache_warmup: CacheWarmupConfig::default(),
            render: RenderConfig::default(),
            source_path: "geo.toml".into(),
        };
        assert!(config.validate().is_err());
    }

    /// Une ancienne configuration sans section render reçoit la palette centrale.
    #[test]
    fn render_palette_has_backward_compatible_defaults() {
        let config: AppConfig = toml::from_str(
            r#"
                [server]
                bind = "127.0.0.1:8080"
                [storage]
                root = "storage"
                [workers]
                downloads = 1
                exports = 1
            "#,
        )
        .expect("configuration rétrocompatible");

        assert_eq!(
            config.render.colors.roads,
            Some([0.105_882, 0.121_569, 0.113_725, 1.0])
        );
        assert!(config.render.colors.forest_low.is_some());
        assert_eq!(
            config.cache_warmup.stl_grid_steps_m,
            vec![50.0, 80.0, 100.0]
        );
    }

    /// Les pas de préchauffage restent dans les bornes que le moteur sait représenter.
    #[test]
    fn cache_warmup_steps_are_bounded_and_can_be_disabled() {
        let mut config = toml::from_str::<AppConfig>(
            r#"
                [server]
                bind = "127.0.0.1:8080"
                [storage]
                root = "storage"
                [workers]
                downloads = 1
                exports = 1
            "#,
        )
        .expect("valeurs par défaut");
        assert!(config.validate().is_ok());
        config.cache_warmup.stl_grid_steps_m.clear();
        config.cache_warmup.blender_atlas_resolutions_px = vec![512, 2048, 4096, 8192];
        assert!(config.validate().is_ok());
        config.cache_warmup.stl_grid_steps_m = vec![101.0];
        assert!(config.validate().is_err());
        config.cache_warmup.stl_grid_steps_m.clear();
        config.cache_warmup.blender_atlas_resolutions_px = vec![8193];
        assert!(config.validate().is_err());
    }

    /// Vérifie que le fichier de configuration distribué reste un TOML valide.
    #[test]
    fn repository_geo_config_is_valid() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("geo.toml");
        let config = AppConfig::load(&path).expect("geo.toml valide");
        assert_eq!(
            config.cache_warmup.stl_grid_steps_m,
            vec![50.0, 80.0, 100.0]
        );
        assert_eq!(
            config.cache_warmup.blender_atlas_resolutions_px,
            vec![512, 2048, 4096, 8192]
        );
    }
}
