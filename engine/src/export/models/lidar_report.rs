//! Contexte : « terrain ALTI » ne signifie pas que le LiDAR est ignoré pour les arbres.
//! Usage : conserver un bilan lisible de la lecture et de la végétation dans le .blend.

use serde::Serialize;
use std::collections::BTreeMap;
#[derive(Debug, Default, Serialize)]
pub struct LidarReport {
    /// Fichiers du cache dont la taille correspond au reçu de fin de transfert.
    pub cached_files: usize,
    /// Dalles qui recoupent effectivement l'emprise exportée.
    pub used_files: usize,
    /// Points décompressés, puis points valides à l'intérieur de la zone.
    pub points_read: u64,
    pub points_used: u64,
    /// Effectif des classes LAS parmi les points conservés dans la zone.
    pub classes: BTreeMap<u8, u64>,
    /// Fraction couverte par les emprises des dalles : pas une garantie de densité.
    pub coverage_fraction: f64,
    pub canopy_cells: usize,
    pub trees_eligible: usize,
    pub trees_filtered_height: usize,
    pub trees_exported: usize,
}
