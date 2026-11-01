//! Contexte : assembler puis réduire une orthophoto détruit une partie des détails disponibles.
//! Usage : transmettre les JPEG originaux et reprojeter les UV des maillages à leur place.
//! Blender découpe les faces aux frontières des tuiles ; les images ne sont jamais réencodées.

use super::{
    files,
    models::{ExportConfig, LayerPlan, OrthoPlan, OrthoTile, Projection, RasterCache, TerrainGrid},
};
use crate::cache;
use anyhow::{Context, Result, bail};
use geo::{Area, BooleanOps, Coord, LineString, Polygon, Rect};
use std::path::{Path, PathBuf};

pub fn prepare(
    root: &Path,
    c: &ExportConfig,
    t: &TerrainGrid,
    layers: &mut [LayerPlan],
) -> Result<OrthoPlan> {
    let path = files::raster(&root.join("BD_ORTHO"))?
        .context("aucun matrix.json BD_ORTHO dans le cache")?;
    let raster = RasterCache::open(&path, c.ortho.max_loaded_tiles)?;
    let metadata = &raster.metadata;
    if !metadata.format.contains("jpeg") {
        bail!("BD ORTHO : des tuiles JPEG natives sont attendues");
    }
    let projection = Projection::new("EPSG:2154", &metadata.crs)?;
    let m = &metadata.matrix;
    let uv = |x: f64, y: f64| -> Result<[f64; 2]> {
        let (x, y) = projection.point(x, y)?;
        // Ici (0,0) est un COIN de tuile ; pas de décalage de demi-pixel.
        Ok([
            (x - m.top_left_x) / (m.resolution_units_per_px * m.tile_width as f64),
            (m.top_left_y - y) / (m.resolution_units_per_px * m.tile_height as f64),
        ])
    };
    let mut terrain_uv = Vec::with_capacity(t.heights.len());
    for row in 0..t.count {
        for col in 0..t.count {
            terrain_uv.push(uv(
                t.xmin + col as f64 * t.side / (t.count - 1) as f64,
                t.ymin + row as f64 * t.side / (t.count - 1) as f64,
            )?);
        }
    }
    // Suivre tous les sommets du bord évite de demander des JPEG complètement
    // à l'extérieur du carré reprojeté, dans les coins du rectangle englobant.
    let n = t.count;
    let indices = (0..n)
        .chain((1..n).map(|r| r * n + n - 1))
        .chain((0..n - 1).rev().map(|k| (n - 1) * n + k))
        .chain((1..n - 1).rev().map(|r| r * n));
    let mut border: Vec<_> = indices
        .map(|i| Coord {
            x: terrain_uv[i][0],
            y: terrain_uv[i][1],
        })
        .collect();
    border.push(border[0]);
    let footprint = Polygon::new(LineString::new(border), Vec::new());
    let umin = terrain_uv
        .iter()
        .map(|p| p[0])
        .fold(f64::INFINITY, f64::min)
        .floor();
    let umax = terrain_uv
        .iter()
        .map(|p| p[0])
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil();
    let vmin = terrain_uv
        .iter()
        .map(|p| p[1])
        .fold(f64::INFINITY, f64::min)
        .floor();
    let vmax = terrain_uv
        .iter()
        .map(|p| p[1])
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil();
    if umin < 0. || vmin < 0. || umax > m.matrix_width as f64 || vmax > m.matrix_height as f64 {
        bail!("zone hors de la grille BD ORTHO");
    }
    if (umax - umin) * (vmax - vmin) > (c.ortho.max_tiles as f64) * 2. {
        bail!("trop de tuiles orthophoto : réduire la zone ou augmenter ortho.max_tiles");
    }
    let mut tiles = Vec::new();
    let mut no_data_tiles = 0usize;
    let mut no_data_image = None;
    for row in vmin as u64..vmax as u64 {
        for col in umin as u64..umax as u64 {
            let tile_box = Rect::new(
                Coord {
                    x: col as f64,
                    y: row as f64,
                },
                Coord {
                    x: col as f64 + 1.,
                    y: row as f64 + 1.,
                },
            )
            .to_polygon();
            if footprint.intersection(&tile_box).unsigned_area() < 1e-12 {
                continue;
            }
            if tiles.len() >= c.ortho.max_tiles {
                bail!("limite ortho.max_tiles atteinte");
            }
            let native_path = raster
                .directory
                .join(row.to_string())
                .join(format!("{col}.jpg"));
            let path = if cache::is_no_data_tile(&native_path) {
                no_data_tiles += 1;
                no_data_image
                    .get_or_insert(prepare_no_data_image(
                        &raster.directory,
                        m.tile_width,
                        m.tile_height,
                    )?)
                    .clone()
            } else {
                native_path
            };
            let (width, height) = image::image_dimensions(&path)
                .with_context(|| format!("JPEG natif absent ou invalide : {}", path.display()))?;
            if u64::from(width) != m.tile_width || u64::from(height) != m.tile_height {
                bail!("dimensions JPEG incohérentes : {}", path.display());
            }
            tiles.push(OrthoTile {
                col,
                row,
                path: super::paths::text(&path),
                width: m.tile_width,
                height: m.tile_height,
            });
        }
    }
    if c.buildings.texture_roofs {
        for layer in layers {
            for object in &mut layer.objects {
                if !object.roof_faces.is_empty() {
                    object.uv = object
                        .vertices
                        .iter()
                        .map(|p| uv(p[0] + t.origin[0], p[1] + t.origin[1]))
                        .collect::<Result<_>>()?;
                }
            }
        }
    }
    println!(
        "[EXPORT orthophoto] {} JPEG, dont {} remplacement(s) sans donnée",
        tiles.len(),
        no_data_tiles
    );
    Ok(OrthoPlan { tiles, terrain_uv })
}

/// Une seule petite image est partagée par toutes les tuiles 404 de la scène.
fn prepare_no_data_image(directory: &Path, width: u64, height: u64) -> Result<PathBuf> {
    let path = directory.join("no-data.jpg");
    if path.is_file() && image::image_dimensions(&path).ok() == Some((width as u32, height as u32))
    {
        return Ok(path);
    }
    let image =
        image::RgbImage::from_pixel(width as u32, height as u32, image::Rgb(cache::NO_DATA_RGB));
    image
        .save(&path)
        .with_context(|| format!("création du JPEG sans donnée : {}", path.display()))?;
    Ok(path)
}
