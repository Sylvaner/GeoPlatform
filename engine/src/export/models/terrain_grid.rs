//! Contexte : le terrain est une grille du sud vers le nord, dans le repère Lambert-93.
//! Usage : height interpole l'altitude pour poser un objet ; local produit les coordonnées Blender.

use anyhow::{Result, bail};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct TerrainGrid {
    pub xmin: f64,
    pub ymin: f64,
    pub side: f64,
    pub count: usize,
    pub heights: Vec<f64>,
    pub origin: [f64; 3],
    pub exaggeration: f64,
    pub source: String,
}
impl TerrainGrid {
    pub fn step(&self) -> f64 {
        self.side / (self.count - 1) as f64
    }
    pub fn height(&self, x: f64, y: f64) -> Result<f64> {
        let u = ((x - self.xmin) / self.step()).clamp(0., (self.count - 1) as f64);
        let v = ((y - self.ymin) / self.step()).clamp(0., (self.count - 1) as f64);
        let col = (u.floor() as usize).min(self.count - 2);
        let row = (v.floor() as usize).min(self.count - 2);
        let a = u - col as f64;
        let b = v - row as f64;
        let z = self.heights[row * self.count + col] * (1. - a) * (1. - b)
            + self.heights[row * self.count + col + 1] * a * (1. - b)
            + self.heights[(row + 1) * self.count + col] * (1. - a) * b
            + self.heights[(row + 1) * self.count + col + 1] * a * b;
        if !z.is_finite() {
            bail!("altitude du terrain manquante");
        }
        Ok(z)
    }
    pub fn local(&self, x: f64, y: f64, z: f64) -> [f64; 3] {
        [
            x - self.origin[0],
            y - self.origin[1],
            (z - self.origin[2]) * self.exaggeration,
        ]
    }

    /// Altitude du maillage visible, avec la diagonale sud-ouest / nord-est.
    /// Une interpolation bilinéaire convient à un relevé, mais ne décrit pas
    /// exactement deux triangles plats. L'eau doit épouser ces mêmes triangles.
    pub fn mesh_height(&self, x: f64, y: f64) -> f64 {
        let u = ((x - self.xmin) / self.step()).clamp(0., (self.count - 1) as f64);
        let v = ((y - self.ymin) / self.step()).clamp(0., (self.count - 1) as f64);
        let col = (u.floor() as usize).min(self.count - 2);
        let row = (v.floor() as usize).min(self.count - 2);
        let a = u - col as f64;
        let b = v - row as f64;
        let sw = self.heights[row * self.count + col];
        let se = self.heights[row * self.count + col + 1];
        let nw = self.heights[(row + 1) * self.count + col];
        let ne = self.heights[(row + 1) * self.count + col + 1];
        if b <= a {
            sw * (1. - a) + se * (a - b) + ne * b
        } else {
            sw * (1. - b) + ne * a + nw * (b - a)
        }
    }
}
