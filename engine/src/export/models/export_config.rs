//! Contexte : ce type décrit tout le fichier TOML d'export et refuse les clés inconnues.
//! Usage : les valeurs par défaut ont une seule source : export.default.toml.

use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExportConfig {
    pub schema_version: u32,
    pub area: AreaConfig,
    pub paths: PathsConfig,
    pub terrain: TerrainConfig,
    pub ortho: OrthoConfig,
    pub layers: BTreeMap<String, LayerConfig>,
    pub vectors: VectorsConfig,
    pub buildings: BuildingsConfig,
    pub vegetation: VegetationConfig,
    pub lidar: LidarConfig,
    pub camera: CameraConfig,
    pub view: ViewConfig,
    pub water: WaterConfig,
    pub lighting: LightingConfig,
    pub render: RenderConfig,
    pub blender: BlenderConfig,
}
