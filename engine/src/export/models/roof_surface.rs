//! Contexte : une surface de toit utilise uniquement les cellules LiDAR du bâtiment courant.
//! Usage : interpoler localement les mesures ; None interdit de combler un grand vide.

use std::collections::BTreeMap;
#[derive(Debug)]
pub struct RoofSurface {
    pub cells: BTreeMap<(i64, i64), [f64; 3]>,
    pub cell_size: f64,
    pub radius: usize,
}

impl RoofSurface {
    pub fn height(&self, x: f64, y: f64) -> Option<f64> {
        let col = (x / self.cell_size).floor() as i64;
        let row = (y / self.cell_size).floor() as i64;
        let radius = self.radius as i64;
        let mut neighbours = Vec::new();
        for a in col - radius..=col + radius {
            for b in row - radius..=row + radius {
                if let Some(p) = self.cells.get(&(a, b)) {
                    let distance = (p[0] - x).hypot(p[1] - y);
                    if distance <= self.cell_size * (self.radius as f64 + 0.5) {
                        if distance < 1e-8 {
                            return Some(p[2]);
                        }
                        neighbours.push((distance, p[2]));
                    }
                }
            }
        }
        // Quatre voisins proches conservent les pentes sans lisser tout le bâtiment.
        neighbours.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut weights = 0.;
        let mut sum = 0.;
        for (distance, z) in neighbours.into_iter().take(4) {
            let weight = 1. / distance.powi(2);
            weights += weight;
            sum += weight * z;
        }
        if weights > 0. {
            Some(sum / weights)
        } else {
            None
        }
    }
}
