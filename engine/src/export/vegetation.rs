//! Contexte : les cellules de canopée ne sont pas toutes des arbres suffisamment hauts.
//! Usage : filtrer les hauteurs AVANT de limiter le nombre de volumes exportés.
//! Sinon l'échantillonnage peut garder seulement des cellules trop basses et perdre les arbres.

use super::models::{ExportConfig, LidarSamples, TerrainGrid, TreePlan};
use anyhow::Result;

pub fn prepare(
    t: &TerrainGrid,
    samples: &mut LidarSamples,
    c: &ExportConfig,
) -> Result<Vec<TreePlan>> {
    if !c.layers["vegetation"].enabled {
        return Ok(Vec::new());
    }
    let mut eligible = Vec::new();
    for top in samples.canopy.values() {
        let ground = t.height(top[0], top[1])?;
        let height = top[2] - ground;
        if height >= c.vegetation.min_height_m && height <= c.vegetation.max_height_m {
            eligible.push(TreePlan {
                position: t.local(top[0], top[1], ground),
                height: height * t.exaggeration,
                radius: height * c.vegetation.crown_radius_ratio,
            });
        }
    }
    samples.report.trees_eligible = eligible.len();
    samples.report.trees_filtered_height = samples.canopy.len() - eligible.len();
    let stride = eligible
        .len()
        .div_ceil(c.vegetation.max_trees.max(1))
        .max(1);
    let trees: Vec<_> = eligible
        .into_iter()
        .step_by(stride)
        .take(c.vegetation.max_trees)
        .collect();
    samples.report.trees_exported = trees.len();
    println!(
        "[EXPORT végétation] {} cellules ; {} arbres admissibles ; {} écartées par hauteur ; {} arbres exportés",
        samples.canopy.len(),
        samples.report.trees_eligible,
        samples.report.trees_filtered_height,
        trees.len()
    );
    Ok(trees)
}
