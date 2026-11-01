//! Contexte : une interruption peut laisser des pages valides sans reçu complete.json.
//! Usage : écrire cette identité avant la requête, pour savoir quelle couche et quelle zone
//! correspondent aux pages déjà reçues. Ce fichier ne prouve jamais que le transfert est fini.

use serde::{Deserialize, Serialize};
#[derive(Debug, Deserialize, Serialize)]
pub struct WfsRequest {
    pub source: String,
    pub layer: String,
    pub bbox: [f64; 4],
    pub sort_by: String,
    pub crs: String,
}
