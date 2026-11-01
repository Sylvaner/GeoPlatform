//! Contexte : un objet préparé contient sa géométrie locale et les attributs de sa source.
//! Usage : Blender crée un maillage, une ligne ou un repère selon kind.

use serde::Serialize;
#[derive(Debug, Serialize)]
pub struct ObjectPlan {
    pub name: String,
    pub kind: String,
    pub vertices: Vec<[f64; 3]>,
    pub faces: Vec<Vec<usize>>,
    /// Indices des faces de toit : les murs conservent leur matériau simple.
    pub roof_faces: Vec<usize>,
    /// Une coordonnée native par sommet ; vide pour un objet sans orthophoto.
    pub uv: Vec<[f64; 2]>,
    pub lines: Vec<Vec<[f64; 3]>>,
    pub properties: serde_json::Value,
}
