//! Contexte : seuls les LAS/LAZ complets du cache, dans la zone, sont lus.
//! Usage : les classes de points gardent leur sens IGN ; les positions restent Lambert-93.
//! La lecture se fait point par point. Elle ne charge pas un nuage complet en mémoire.

use super::models::{ExportConfig, LidarSamples};
use anyhow::{Context, Result, bail};
use geo::{Area, BooleanOps, Coord, MultiPolygon, Rect};
use las::Reader;
use serde_json::Value;
use std::{collections::BTreeSet, path::Path};

pub fn read(
    root: &Path,
    c: &ExportConfig,
    xmin: f64,
    ymin: f64,
    count: usize,
) -> Result<LidarSamples> {
    let mut out = LidarSamples {
        report: Default::default(),
        ground: vec![None; count * count],
        canopy: Default::default(),
        roofs: Default::default(),
        display: Vec::new(),
        files: Vec::new(),
    };
    // selection.json est un inventaire, pas une obligation : les reçus locaux
    // suffisent aussi si une ancienne sélection a été remplacée depuis le transfert.
    let lidar_root = root.join("LIDAR_HD");
    let mut folders = vec![lidar_root.clone()];
    let mut candidates = Vec::new();
    while let Some(folder) = folders.pop() {
        if !folder.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(folder)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                folders.push(entry.path());
            } else if kind.is_file()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".laz.download.json")
            {
                let receipt: Value = serde_json::from_slice(&std::fs::read(entry.path())?)?;
                let path = entry.path().with_file_name(
                    entry
                        .file_name()
                        .to_string_lossy()
                        .trim_end_matches(".download.json"),
                );
                if std::fs::metadata(&path).is_ok_and(|m| Some(m.len()) == receipt["size"].as_u64())
                {
                    candidates.push(path);
                }
            }
        }
    }
    // Les éditions ISO récentes sont triées avant les anciennes, puis une seule
    // dalle de même nom géographique est utilisée pour éviter les doublons d'édition.
    candidates.sort_by(|a, b| {
        let edition = |p: &std::path::PathBuf| {
            p.parent()
                .and_then(|p| p.file_name())
                .map(|s| {
                    s.to_string_lossy()
                        .rsplit('_')
                        .next()
                        .unwrap_or("")
                        .to_owned()
                })
                .unwrap_or_default()
        };
        edition(b).cmp(&edition(a)).then_with(|| b.cmp(a))
    });
    let mut seen = BTreeSet::new();
    out.report.cached_files = candidates.len();
    let area = Rect::new(
        Coord { x: xmin, y: ymin },
        Coord {
            x: xmin + c.area.side_m,
            y: ymin + c.area.side_m,
        },
    )
    .to_polygon();
    let mut coverage = MultiPolygon::<f64>(Vec::new());
    let mut sampled = 0usize;
    let mut eligible = 0usize;
    let mut random_state = c.vegetation.seed;
    let step = c.area.side_m / (count - 1) as f64;
    for path in candidates {
        let mut reader =
            Reader::from_path(&path).with_context(|| format!("lecture LAZ {}", path.display()))?;
        let bounds = reader.header().bounds();
        if bounds.max.x < xmin
            || bounds.min.x > xmin + c.area.side_m
            || bounds.max.y < ymin
            || bounds.min.y > ymin + c.area.side_m
        {
            continue;
        }
        let key = path.file_name().unwrap().to_owned();
        if !seen.insert(key) {
            continue;
        }
        println!("[EXPORT LiDAR] {}", path.display());
        out.files.push(path.display().to_string());
        let extent = Rect::new(
            Coord {
                x: bounds.min.x,
                y: bounds.min.y,
            },
            Coord {
                x: bounds.max.x,
                y: bounds.max.y,
            },
        )
        .to_polygon();
        coverage = coverage.union(&extent.intersection(&area));
        for point in reader.points() {
            let point = point?;
            out.report.points_read += 1;
            if out.report.points_read.is_multiple_of(1_000_000) {
                println!(
                    "[EXPORT LiDAR] {} millions de points lus, {} points dans la zone",
                    out.report.points_read / 1_000_000,
                    out.report.points_used
                );
            }
            if !point.x.is_finite()
                || !point.y.is_finite()
                || !point.z.is_finite()
                || point.is_withheld
                || point.z < c.terrain.valid_altitude_min_m
                || point.z > c.terrain.valid_altitude_max_m
                || point.x < xmin
                || point.x > xmin + c.area.side_m
                || point.y < ymin
                || point.y > ymin + c.area.side_m
            {
                continue;
            }
            let class: u8 = point.classification.into();
            out.report.points_used += 1;
            *out.report.classes.entry(class).or_default() += 1;
            // La classe 6 contient les retours de bâtiments. Elle reste indépendante
            // du pas du terrain et de l'échantillonnage des points d'aperçu.
            if class == 6 && c.layers["bd_topo.batiment"].enabled && c.buildings.source != "bd_topo"
            {
                let key = (
                    (point.x / c.buildings.cell_size_m).floor() as i64,
                    (point.y / c.buildings.cell_size_m).floor() as i64,
                );
                if !out.roofs.contains_key(&key) && out.roofs.len() >= c.buildings.max_cells {
                    bail!(
                        "trop de cellules de toit LiDAR ; réduire la zone ou augmenter buildings.max_cells"
                    );
                }
                out.roofs
                    .entry(key)
                    .or_default()
                    .add([point.x, point.y, point.z]);
            }
            if class == c.terrain.lidar_ground_class {
                let col = ((point.x - xmin) / step).round() as usize;
                let row = ((point.y - ymin) / step).round() as usize;
                let cell = &mut out.ground[row * count + col];
                *cell = Some(cell.map_or(point.z, |z| z.min(point.z)));
            }
            if c.layers["vegetation"].enabled && c.vegetation.classes.contains(&class) {
                let key = (
                    ((point.x - xmin) / c.vegetation.cell_size_m).floor() as i64,
                    ((point.y - ymin) / c.vegetation.cell_size_m).floor() as i64,
                );
                let top = out.canopy.entry(key).or_insert([point.x, point.y, point.z]);
                if point.z > top[2] {
                    *top = [point.x, point.y, point.z];
                }
            }
            if c.layers["lidar_points"].enabled {
                if sampled.is_multiple_of(c.lidar.display_stride) {
                    eligible += 1;
                    let value = ([point.x, point.y, point.z], class);
                    if out.display.len() < c.lidar.max_display_points {
                        out.display.push(value);
                    } else {
                        // Réservoir déterministe : répartir l'aperçu sur toutes
                        // les dalles lues, pas seulement sur la première.
                        random_state = random_state
                            .wrapping_mul(6364136223846793005)
                            .wrapping_add(1442695040888963407);
                        let index = (random_state % (eligible as u64)) as usize;
                        if index < out.display.len() {
                            out.display[index] = value;
                        }
                    }
                }
                sampled += 1;
            }
        }
    }
    out.report.used_files = out.files.len();
    out.report.canopy_cells = out.canopy.len();
    out.report.coverage_fraction = (coverage.unsigned_area() / c.area.side_m.powi(2)).clamp(0., 1.);
    println!(
        "[EXPORT LiDAR] {} dalle(s) utilisée(s) sur {} complète(s) en cache ; couverture {:.1} % ; classes {:?}",
        out.report.used_files,
        out.report.cached_files,
        out.report.coverage_fraction * 100.,
        out.report.classes
    );
    Ok(out)
}
