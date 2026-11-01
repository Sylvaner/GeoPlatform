//! Contexte : les tuiles restent dans leur grille native, sans grand assemblage en mémoire.
//! Usage : sample reçoit des mètres Lambert-93 puis lit le bon pixel du cache.
//! Le demi-pixel distingue le coin d'une tuile du centre de son premier pixel.

use super::{Projection, RasterMetadata, TileData};
use crate::cache;
use anyhow::{Context, Result, bail};
use std::{
    collections::{BTreeMap, VecDeque},
    path::{Path, PathBuf},
};

pub struct RasterCache {
    pub directory: PathBuf,
    pub metadata: RasterMetadata,
    projection: Projection,
    tiles: BTreeMap<(u64, u64), TileData>,
    order: VecDeque<(u64, u64)>,
    limit: usize,
}
impl RasterCache {
    pub fn open(path: &Path, limit: usize) -> Result<Self> {
        let metadata: RasterMetadata = serde_json::from_slice(&std::fs::read(path)?)?;
        if metadata.schema_version != 2 {
            bail!("cache WMTS ancien ou incompatible : {}", path.display());
        }
        if !["EPSG:2154", "EPSG:4326", "EPSG:3857"].contains(&metadata.crs.as_str()) {
            bail!("CRS de raster non pris en charge");
        }
        if metadata.crs == "EPSG:4326" && metadata.coordinate_order != "longitude_latitude" {
            bail!("ordre des axes du cache géographique inconnu");
        }
        if metadata.matrix.resolution_units_per_px <= 0.
            || metadata.matrix.tile_width == 0
            || metadata.matrix.tile_height == 0
        {
            bail!("grille de cache invalide");
        }
        let projection = Projection::new("EPSG:2154", &metadata.crs)?;
        Ok(Self {
            directory: path.parent().unwrap().to_owned(),
            metadata,
            projection,
            tiles: BTreeMap::new(),
            order: VecDeque::new(),
            limit: limit.max(4),
        })
    }
    fn pixel(&mut self, x: i64, y: i64) -> Result<[f64; 3]> {
        let m = &self.metadata.matrix;
        if x < 0
            || y < 0
            || x as u64 >= m.matrix_width * m.tile_width
            || y as u64 >= m.matrix_height * m.tile_height
        {
            bail!("pixel hors de la grille");
        }
        let (col, row) = (x as u64 / m.tile_width, y as u64 / m.tile_height);
        let (px, py) = (
            (x as u64 % m.tile_width) as u32,
            (y as u64 % m.tile_height) as u32,
        );
        let key = (row, col);
        if !self.tiles.contains_key(&key) {
            let extension = if self.metadata.format.contains("bil") {
                "bil"
            } else {
                "jpg"
            };
            let path = self
                .directory
                .join(row.to_string())
                .join(format!("{col}.{extension}"));
            let tile = if cache::is_no_data_tile(&path) {
                if extension == "bil" {
                    TileData::Altitudes(vec![0.; (m.tile_width * m.tile_height) as usize])
                } else {
                    TileData::Colors(image::RgbImage::from_pixel(
                        m.tile_width as u32,
                        m.tile_height as u32,
                        image::Rgb(cache::NO_DATA_RGB),
                    ))
                }
            } else if extension == "bil" {
                let bytes = std::fs::read(&path)
                    .with_context(|| format!("tuile absente du cache : {}", path.display()))?;
                if bytes.len() != (m.tile_width * m.tile_height * 4) as usize {
                    bail!("BIL incomplet : {}", path.display());
                }
                TileData::Altitudes(
                    bytes
                        .chunks_exact(4)
                        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                        .collect(),
                )
            } else {
                let bytes = std::fs::read(&path)
                    .with_context(|| format!("tuile absente du cache : {}", path.display()))?;
                let image = image::load_from_memory(&bytes)?.to_rgb8();
                if image.width() as u64 != m.tile_width || image.height() as u64 != m.tile_height {
                    bail!("dimensions JPEG du cache incohérentes");
                }
                TileData::Colors(image)
            };
            if self.tiles.len() >= self.limit
                && let Some(old) = self.order.pop_front()
            {
                self.tiles.remove(&old);
            }
            self.tiles.insert(key, tile);
            self.order.push_back(key);
        }
        match &self.tiles[&key] {
            TileData::Altitudes(values) => {
                let z = values[py as usize * self.metadata.matrix.tile_width as usize + px as usize]
                    as f64;
                Ok([z, z, z])
            }
            TileData::Colors(image) => Ok(image.get_pixel(px, py).0.map(|v| v as f64)),
        }
    }
    pub fn sample(&mut self, x: f64, y: f64, bilinear: bool) -> Result<[f64; 3]> {
        let (x, y) = self.projection.point(x, y)?;
        let m = &self.metadata.matrix;
        let u = (x - m.top_left_x) / m.resolution_units_per_px - 0.5;
        let v = (m.top_left_y - y) / m.resolution_units_per_px - 0.5;
        if !bilinear {
            return self.pixel(u.round() as i64, v.round() as i64);
        }
        let (ix, iy) = (u.floor() as i64, v.floor() as i64);
        let (a, b) = (u - ix as f64, v - iy as f64);
        let p = [
            self.pixel(ix, iy)?,
            self.pixel(ix + 1, iy)?,
            self.pixel(ix, iy + 1)?,
            self.pixel(ix + 1, iy + 1)?,
        ];
        Ok(std::array::from_fn(|i| {
            p[0][i] * (1. - a) * (1. - b)
                + p[1][i] * a * (1. - b)
                + p[2][i] * (1. - a) * b
                + p[3][i] * a * b
        }))
    }
}
