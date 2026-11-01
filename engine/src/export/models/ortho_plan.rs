//! Contexte : les coordonnées UV sont exprimées en numéros de tuiles, pas en pixels réduits.
//! Usage : découper chaque face aux frontières des JPEG et employer des UV locaux 0..1.

use super::OrthoTile;
use serde::Serialize;
#[derive(Debug, Serialize)]
pub struct OrthoPlan {
    pub tiles: Vec<OrthoTile>,
    pub terrain_uv: Vec<[f64; 2]>,
}
