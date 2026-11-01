//! Contexte : le plan intermédiaire transmet des données locales et bornées à Blender.
//! Usage : il sert uniquement pendant l'export ; la scène finale embarque ses ressources.

use super::{ExportConfig, LayerPlan, LidarReport, OrthoPlan, TerrainGrid, TreePlan};
use serde::Serialize;
#[derive(Serialize)]
pub struct ScenePlan {
    pub config: ExportConfig,
    pub terrain: TerrainGrid,
    pub ortho: Option<OrthoPlan>,
    pub configuration_toml: String,
    pub configuration_source: String,
    pub layers: Vec<LayerPlan>,
    pub trees: Vec<TreePlan>,
    pub points: Vec<([f64; 3], u8)>,
    pub warnings: Vec<String>,
    pub lidar_files: Vec<String>,
    pub lidar_report: LidarReport,
    pub output: String,
    pub preview: String,
}
