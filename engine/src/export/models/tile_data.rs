//! Contexte : une tuile décodée contient soit des altitudes, soit des couleurs.
//! Usage : RasterCache conserve un nombre borné de ces tuiles pour lire de grandes zones.

pub enum TileData {
    Altitudes(Vec<f32>),
    Colors(image::RgbImage),
}
