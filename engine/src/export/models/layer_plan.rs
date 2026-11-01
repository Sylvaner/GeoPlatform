//! Contexte : une couche est une collection indépendante, avec son style et ses objets.
//! Usage : les collections vides sont omises ; les sources partielles sont signalées.

use super::{LayerConfig, ObjectPlan};
use serde::Serialize;
#[derive(Debug, Serialize)]
pub struct LayerPlan {
    pub name: String,
    pub style: LayerConfig,
    pub sources: Vec<String>,
    pub partial: bool,
    pub objects: Vec<ObjectPlan>,
}
