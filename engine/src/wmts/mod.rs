//! Contexte : WMTS est le protocole utilisé par l'IGN pour servir des tuiles.
//! Usage : `app.rs` lit le catalogue avec `get_capabilities` ; `download.rs` demande
//! les octets d'une tuile avec `get_tile`. Les fichiers sont écrits par `cache.rs`.
//! Ce fichier contient les paramètres du service et les requêtes HTTP ; la lecture
//! du XML est dans capabilities.rs et les structures sont dans models/.

// Ce sous-module reste interne ; seule sa fonction principale est réexportée.
mod capabilities;

pub use capabilities::parse_capabilities;

use crate::models::{TileMatrix, WmtsLayer};
use anyhow::{Context, Result, bail};
use reqwest::{Client, StatusCode};

/// Adresse du service de tuiles de la Géoplateforme IGN.
const WMTS_URL: &str = "https://data.geopf.fr/wmts";

/// Récupère le catalogue du serveur sous forme de texte XML.
pub async fn get_capabilities(client: &Client) -> Result<String> {
    // get prépare une requête HTTP GET ; query ajoute les paramètres à l'URL.
    // &[...] est une liste de couples (nom, valeur) empruntée par reqwest.
    let response = client
        .get(WMTS_URL)
        .query(&[
            ("SERVICE", "WMTS"),
            ("VERSION", "1.0.0"),
            ("REQUEST", "GetCapabilities"),
        ])
        // send lance la requête ; await attend la réponse ; ? propage une erreur réseau.
        .send()
        .await
        .context("appel GetCapabilities")?
        // Convertir aussi les statuts HTTP d'échec (par exemple 404 ou 500) en erreurs.
        .error_for_status()
        .context("GetCapabilities a renvoyé une erreur HTTP")?;

    // Lire le corps de la réponse comme du texte, plutôt que comme des octets bruts.
    response.text().await.context("lecture GetCapabilities")
}

/// Demande une tuile au serveur et renvoie ses octets, sans toucher au disque.
pub async fn get_tile(
    client: &Client,
    layer: &WmtsLayer,
    matrix: &TileMatrix,
    row: u64,
    col: u64,
) -> Result<Option<Vec<u8>>> {
    get_tile_at(client, WMTS_URL, layer, matrix, row, col).await
}

// L'URL est injectable pour tester le vrai transport HTTP avec un serveur local.
async fn get_tile_at(
    client: &Client,
    endpoint: &str,
    layer: &WmtsLayer,
    matrix: &TileMatrix,
    row: u64,
    col: u64,
) -> Result<Option<Vec<u8>>> {
    // Les paramètres de grille et de format proviennent du catalogue XML.
    // as_str prête le texte des String ; to_string convertit les indices en texte.
    let response = client
        .get(endpoint)
        .query(&[
            ("SERVICE", "WMTS"),
            ("VERSION", "1.0.0"),
            ("REQUEST", "GetTile"),
            ("LAYER", layer.identifier.as_str()),
            ("STYLE", layer.style.as_str()),
            ("FORMAT", layer.format.as_str()),
            // Reprendre exactement le jeu de grilles et le niveau sélectionnés.
            // Un nom de grille inventé ou celui d'une autre couche ferait échouer GetTile.
            ("TILEMATRIXSET", matrix.matrix_set.as_str()),
            ("TILEMATRIX", matrix.identifier.as_str()),
            ("TILEROW", &row.to_string()),
            ("TILECOL", &col.to_string()),
        ])
        .send()
        .await
        .with_context(|| format!("GetTile row={row} col={col}"))?;
    // Les limites WMTS décrivent une grille rectangulaire, alors que la couverture
    // réelle de l'orthophoto et de l'altimétrie peut être irrégulière (côtes, îles).
    // Dans ce cas IGN répond 404 : c'est une absence de donnée, pas une panne.
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let response = response
        .error_for_status()
        .with_context(|| format!("HTTP GetTile row={row} col={col}"))?;

    // Certains services OGC renvoient une erreur XML même avec un statut HTTP 200.
    // Il faut la refuser, sinon elle serait enregistrée comme une fausse tuile BIL.
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    // bytes contient ici le corps décompressé par reqwest, pas les octets compressés
    // du transport HTTP. La vérification de taille ci-dessous porte sur les pixels.
    let bytes = response.bytes().await?;
    if bytes.is_empty() {
        bail!("tuile vide");
    }
    // Une réponse JPEG doit commencer par sa signature binaire. Un simple nom
    // de fichier .jpg ne prouve pas que la réponse du serveur est une photographie.
    if layer.format.contains("jpeg") && !bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        bail!("la réponse reçue n'est pas une image JPEG");
    }
    if content_type.contains("xml") || content_type.contains("html") || bytes.starts_with(b"<?xml")
    {
        bail!(
            "le serveur a renvoyé une erreur au lieu d'une tuile : {}",
            String::from_utf8_lossy(&bytes)
        );
    }
    // Une tuile BIL float32 contient un nombre connu de pixels, chacun sur 4 octets.
    // Vérifier la taille évite aussi de mettre en cache une réponse tronquée.
    if layer.format.to_ascii_lowercase().contains("bil") && layer.format.contains("32") {
        let expected = matrix.tile_width * matrix.tile_height * 4;
        if bytes.len() as u64 != expected {
            bail!(
                "taille BIL incorrecte : {} octets reçus, {expected} attendus",
                bytes.len()
            );
        }
    }
    // Vec<u8> est une liste d'octets possédée par l'appelant après le retour.
    Ok(Some(bytes.to_vec()))
}

#[cfg(test)]
mod tests;
