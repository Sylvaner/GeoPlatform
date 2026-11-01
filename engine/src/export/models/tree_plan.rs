//! Contexte : chaque houppier représente un sommet de cellule végétalisée du LiDAR.
//! Usage : height et radius donnent un volume simplifié ; ce n'est pas un inventaire d'arbres.

use serde::Serialize;
#[derive(Debug, Serialize)]
pub struct TreePlan {
    pub position: [f64; 3],
    pub height: f64,
    pub radius: f64,
}
