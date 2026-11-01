//! Contexte : résumer les points de bâtiment d'un carré sans garder tout le nuage.
//! Usage : la moyenne donne une mesure locale ; l'étendue signale une cellule incohérente.

#[derive(Debug, Default, Clone)]
pub struct RoofCell {
    pub sum: [f64; 3],
    pub count: usize,
    pub min_z: f64,
    pub max_z: f64,
}

impl RoofCell {
    pub fn add(&mut self, point: [f64; 3]) {
        if self.count == 0 {
            self.min_z = point[2];
            self.max_z = point[2];
        }
        for (sum, value) in self.sum.iter_mut().zip(point) {
            *sum += value;
        }
        self.count += 1;
        self.min_z = self.min_z.min(point[2]);
        self.max_z = self.max_z.max(point[2]);
    }

    pub fn mean(&self) -> [f64; 3] {
        self.sum.map(|v| v / self.count as f64)
    }
}
