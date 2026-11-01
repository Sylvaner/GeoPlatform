//! Contexte : les bâtiments BD TOPO peuvent déjà contenir des sommets de toit en 3D.
//! Usage : retrouver une altitude à l'intérieur d'un triangle sans aplatir ces données.

#[derive(Debug)]
pub struct SourceRoofTriangle {
    pub points: [[f64; 3]; 3],
}
impl SourceRoofTriangle {
    pub fn height(&self, x: f64, y: f64) -> Option<f64> {
        let [a, b, c] = self.points;
        let denominator = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
        if denominator.abs() < 1e-12 {
            return None;
        }
        let u = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / denominator;
        let v = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / denominator;
        let w = 1. - u - v;
        if [u, v, w]
            .iter()
            .all(|value| *value >= -1e-6 && *value <= 1. + 1e-6)
        {
            Some(u * a[2] + v * b[2] + w * c[2])
        } else {
            None
        }
    }
}
