//! Contexte : construire un terrain réellement renseigné, sans remplacer des trous par zéro.
//! Usage : ALTI est prioritaire en auto ; les points sol LiDAR comblent les cellules absentes.
//! L'interpolation des cellules LiDAR vides reste limitée au voisinage configuré.

use super::{
    files, lidar,
    models::{ExportConfig, LidarSamples, RasterCache, TerrainGrid},
};
use anyhow::{Result, bail};
use std::path::Path;

pub fn prepare(root: &Path, c: &ExportConfig) -> Result<(TerrainGrid, LidarSamples, Vec<String>)> {
    let (cx, cy) =
        crate::coordinates::to_lambert93(c.area.center[0], c.area.center[1], &c.area.crs)?;
    let xmin = cx - c.area.side_m / 2.;
    let ymin = cy - c.area.side_m / 2.;
    let count_float = (c.area.side_m / c.terrain.grid_step_m).ceil() + 1.;
    if !count_float.is_finite()
        || count_float < 2.
        || count_float * count_float > c.terrain.max_vertices as f64
    {
        bail!("terrain trop dense : augmenter grid_step_m ou max_vertices");
    }
    let count = count_float as usize;
    let step = c.area.side_m / (count - 1) as f64;
    let mut heights = vec![None; count * count];
    let mut notes = Vec::new();
    let mut used_alti = false;
    let mut used_lidar = false;
    if c.terrain.source != "lidar"
        && let Some(path) = files::raster(&root.join("RGE_ALTI"))?
    {
        let mut raster = RasterCache::open(&path, c.ortho.max_loaded_tiles)?;
        for row in 0..count {
            for col in 0..count {
                if let Ok(pixel) =
                    raster.sample(xmin + col as f64 * step, ymin + row as f64 * step, false)
                {
                    let z = pixel[0];
                    if z.is_finite()
                        && z >= c.terrain.valid_altitude_min_m
                        && z <= c.terrain.valid_altitude_max_m
                    {
                        heights[row * count + col] = Some(z);
                        used_alti = true;
                    }
                }
            }
        }
    }
    let need_lidar = c.terrain.source == "lidar"
        || (c.terrain.source == "auto" && heights.iter().any(Option::is_none))
        || c.layers["vegetation"].enabled
        || (c.layers["bd_topo.batiment"].enabled && c.buildings.source != "bd_topo")
        || c.layers["lidar_points"].enabled;
    let samples = if need_lidar {
        lidar::read(root, c, xmin, ymin, count)?
    } else {
        LidarSamples {
            report: Default::default(),
            ground: vec![None; count * count],
            canopy: Default::default(),
            roofs: Default::default(),
            display: Vec::new(),
            files: Vec::new(),
        }
    };
    if c.terrain.source != "alti" {
        for (index, height) in heights.iter_mut().enumerate() {
            if height.is_none() {
                *height = samples.ground[index];
                used_lidar |= height.is_some();
            }
        }
    }
    // Lire les voisins dans la grille originale : ne pas propager artificiellement
    // une altitude à travers une grande zone sans points.
    let original = heights.clone();
    let radius = c.terrain.lidar_fill_radius_cells;
    if c.terrain.source != "alti" {
        for row in 0..count {
            for col in 0..count {
                let index = row * count + col;
                if heights[index].is_some() {
                    continue;
                }
                for distance in 1..=radius {
                    let mut sum = 0.;
                    let mut total = 0;
                    for r in row.saturating_sub(distance)..=(row + distance).min(count - 1) {
                        for k in col.saturating_sub(distance)..=(col + distance).min(count - 1) {
                            if let Some(z) = original[r * count + k] {
                                sum += z;
                                total += 1;
                            }
                        }
                    }
                    if total > 0 {
                        heights[index] = Some(sum / total as f64);
                        break;
                    }
                }
            }
        }
    }
    let missing = heights.iter().filter(|z| z.is_none()).count();
    if missing > 0 {
        bail!(
            "terrain incomplet dans le cache : {missing} sommets sans altitude. Réduire la zone ou compléter le cache ALTI/LiDAR."
        );
    }
    let heights: Vec<f64> = heights.into_iter().map(Option::unwrap).collect();
    let origin_z = if c.terrain.altitude_origin == "min" {
        heights.iter().copied().fold(f64::INFINITY, f64::min)
    } else {
        0.
    };
    let source = if used_alti {
        if used_lidar { "ALTI + LiDAR" } else { "ALTI" }
    } else {
        "LiDAR sol"
    };
    if need_lidar && samples.files.is_empty() {
        notes.push(
            "Aucun LiDAR complet sur la zone : végétation et points non disponibles.".to_owned(),
        );
    }
    Ok((
        TerrainGrid {
            xmin,
            ymin,
            side: c.area.side_m,
            count,
            heights,
            origin: [cx, cy, origin_z],
            exaggeration: c.terrain.vertical_exaggeration,
            source: source.into(),
        },
        samples,
        notes,
    ))
}
