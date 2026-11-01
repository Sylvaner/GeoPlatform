//! Contexte : les produits sont téléchargés par HTTP depuis l'IGN et le Sandre.
//! Usage : app.rs, vector.rs et lidar/mod.rs prêtent un client aux téléchargeurs.
//! Les réglages de transport sont communs ; les protocoles WMTS et WFS restent séparés.

use anyhow::{Context, Result};
use reqwest::Client;
use std::time::Duration;

/// Crée un client réutilisable, avec décompression HTTP et délai de connexion borné.
pub fn create_client() -> Result<Client> {
    // deflate/gzip sont activés dans Cargo.toml : IGN compresse ses réponses BIL.
    // Cette décompression de transport ne décompresse pas les fichiers LAZ eux-mêmes.
    // Le délai porte seulement sur la connexion, pas sur le transfert complet d'un LAZ.
    Client::builder()
        .user_agent("geo-data/0.1 (cache local IGN)")
        .connect_timeout(Duration::from_secs(30))
        .build()
        .context("création du client HTTP")
}
