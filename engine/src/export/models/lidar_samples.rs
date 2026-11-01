//! Contexte : l'export n'a besoin que de petits résumés des millions de points LiDAR.
//! Usage : les points sol alimentent le terrain ; les sommets de végétation les houppiers.

use super::{LidarReport, RoofCell};
use std::collections::BTreeMap;
#[derive(Debug)]
pub struct LidarSamples {
    pub report: LidarReport,
    pub ground: Vec<Option<f64>>,
    pub canopy: BTreeMap<(i64, i64), [f64; 3]>,
    pub roofs: BTreeMap<(i64, i64), RoofCell>,
    pub display: Vec<([f64; 3], u8)>,
    pub files: Vec<String>,
}
