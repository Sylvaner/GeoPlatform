//! Contexte : les rasters du cache peuvent être en degrés ou en Web Mercator.
//! Usage : Projection convertit des X/Y Lambert-93 vers le CRS annoncé dans matrix.json.
//! Les calculs restent en coordonnées absolues ; Blender reçoit ensuite un repère local.

use anyhow::{Result, bail};
use proj4rs::{Proj, transform::transform};

pub struct Projection {
    source: Proj,
    target: Proj,
    same_crs: bool,
}
impl Projection {
    pub fn new(source: &str, target: &str) -> Result<Self> {
        Ok(Self {
            source: Proj::from_user_string(source)?,
            target: Proj::from_user_string(target)?,
            same_crs: source == target,
        })
    }
    pub fn point(&self, x: f64, y: f64) -> Result<(f64, f64)> {
        // Une identité doit rester exacte, sans aller-retour numérique dans une projection.
        if self.same_crs {
            return Ok((x, y));
        }
        let mut point = (x, y, 0.);
        if self.source.is_latlong() {
            point.0 = point.0.to_radians();
            point.1 = point.1.to_radians();
        }
        transform(&self.source, &self.target, &mut point)?;
        if self.target.is_latlong() {
            point.0 = point.0.to_degrees();
            point.1 = point.1.to_degrees();
        }
        if !point.0.is_finite() || !point.1.is_finite() {
            bail!("coordonnée non projetable");
        }
        Ok((point.0, point.1))
    }
}
