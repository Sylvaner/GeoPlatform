//! Contexte : types métier échangés de façon identique par l'API, les workers et les CLI.
//! Usage : ajouter ici un modèle seulement s'il décrit une notion visible du produit.

mod area;
mod artifact;
mod export_preset;
mod job;
mod layer;
mod request;

pub use area::{AdvancedArea, Area};
pub use artifact::{Artifact, ArtifactKind};
pub use export_preset::{ExportPreset, ExportPresetDraft};
pub use job::{Job, JobKind, JobStatus};
pub use layer::DataLayer;
pub use request::{
    AdvancedExport, DownloadRequest, ExportColors, ExportFormat, ExportQuality, ExportRequest,
    ForestMaterialMode, OrthophotoStorage, TextureMode, TreeShape,
};
