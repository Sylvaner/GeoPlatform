//! Contexte : WFS sert des objets (bâtiments, cadastre, hydrographie, LiDAR), par pages.
//! Usage : download_layer conserve chaque réponse GeoJSON sans modifier les objets.
//! Le carré sert de filtre d'intersection : les objets entiers peuvent en dépasser.
//! Aucun découpage, conversion de géométrie ou export supplémentaire n'est effectué.

use anyhow::{Context, Result, anyhow, bail};
use reqwest::Client;
use serde_json::Value;
use std::{collections::BTreeSet, path::Path};
use tokio::fs;

use crate::models::{Bbox, WfsLayer, WfsManifest, WfsRequest};

pub const URL: &str = "https://data.geopf.fr/wfs/ows";
const PAGE_SIZE: usize = 500;

/// Parcourt toutes les pages, ou relit un téléchargement précédemment terminé.
pub async fn download_layer(
    client: &Client,
    endpoint: &str,
    layer: &WfsLayer,
    bbox: Bbox,
    directory: &Path,
    force: bool,
) -> Result<Vec<Value>> {
    let bounds = [bbox.xmin, bbox.ymin, bbox.xmax, bbox.ymax];
    if bounds.iter().any(|value| !value.is_finite())
        || bbox.xmin >= bbox.xmax
        || bbox.ymin >= bbox.ymax
    {
        bail!("emprise WFS invalide");
    }
    // Lire le cache seulement sans --force ; Some signifie que toutes les pages sont valides.
    if !force && let Some(features) = read_complete(directory, endpoint, layer, bounds).await {
        println!("[CACHE] {} : {} objet(s)", layer.label, features.len());
        return Ok(features);
    }
    fs::create_dir_all(directory).await?;
    // Invalider l'ancien reçu AVANT de remplacer une page. Une interruption ne
    // doit jamais laisser croire qu'un mélange d'anciennes et nouvelles pages est complet.
    let complete = directory.join("complete.json");
    match fs::remove_file(&complete).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    // Ce petit fichier reste présent même si le serveur répond 504 ensuite.
    // L'export peut ainsi identifier les pages reçues avant une interruption.
    // Il ne doit jamais interpréter request.json comme un téléchargement terminé.
    let request = WfsRequest {
        source: endpoint.to_owned(),
        layer: layer.identifier.to_owned(),
        bbox: bounds,
        sort_by: layer.sort_by.to_owned(),
        crs: "EPSG:2154".into(),
    };
    crate::cache::write_tile(
        &directory.join("request.json"),
        &serde_json::to_vec_pretty(&request)?,
    )
    .await?;
    let bbox_parameter = format!(
        "{},{},{},{},EPSG:2154",
        bbox.xmin, bbox.ymin, bbox.xmax, bbox.ymax
    );
    let mut all_features = Vec::new();
    let mut pages = Vec::new();
    let mut seen_pages = BTreeSet::new();
    let mut seen_ids = BTreeSet::new();
    let mut expected_total = None;
    loop {
        let start = all_features.len();
        let response = client
            .get(endpoint)
            .query(&[
                ("SERVICE", "WFS"),
                ("VERSION", "2.0.0"),
                ("REQUEST", "GetFeature"),
                ("TYPENAMES", layer.identifier),
                ("SRSNAME", "EPSG:2154"),
                ("BBOX", bbox_parameter.as_str()),
                ("OUTPUTFORMAT", layer.output_format),
                ("COUNT", &PAGE_SIZE.to_string()),
                ("STARTINDEX", &start.to_string()),
                ("SORTBY", layer.sort_by),
            ])
            .send()
            .await
            .with_context(|| format!("requête WFS {}", layer.identifier))?
            .error_for_status()
            .context("erreur HTTP WFS")?;
        let text = response.text().await?;
        let page = parse_page(&text)?;
        // parse_page a vérifié la présence de la liste : unwrap est donc sûr ici.
        let features = page["features"].as_array().unwrap();
        let total = page["numberMatched"].as_u64().or_else(|| {
            page["numberMatched"]
                .as_str()
                .and_then(|value| value.parse().ok())
        });
        if let Some(total) = total {
            if expected_total.is_some_and(|previous| previous != total) {
                bail!("le nombre d'objets WFS a changé pendant le téléchargement ; relancer");
            }
            expected_total = Some(total);
        }
        if !features.is_empty() && !seen_pages.insert(serde_json::to_string(features)?) {
            bail!("le serveur WFS répète une page : pagination interrompue");
        }
        // Un objet répété peut signaler une modification du catalogue entre pages.
        // Ne pas prétendre avoir reçu autant d'objets distincts que le total annoncé.
        for feature in features {
            // GeoServer IGN écrit id à la racine. Le GeoJSON du MapServer Sandre
            // ne le fait pas : gid, dans properties, identifie alors l'objet.
            let id = if !feature["id"].is_null() {
                &feature["id"]
            } else {
                &feature["properties"]["gid"]
            };
            if !id.is_null() && !seen_ids.insert(id.to_string()) {
                bail!("objet WFS répété entre les pages : {id}");
            }
        }
        crate::cache::write_tile(
            &directory.join(format!("page-{start:06}.geojson")),
            text.as_bytes(),
        )
        .await?;
        pages.push(start);
        let empty = features.is_empty();
        all_features.extend(features.iter().cloned());
        if let Some(total) = expected_total {
            if all_features.len() as u64 > total {
                bail!("WFS incohérent : davantage d'objets que le total annoncé");
            }
            if all_features.len() as u64 == total {
                break;
            }
            if empty {
                bail!("WFS incomplet : {} objets sur {total}", all_features.len());
            }
        } else if empty {
            break;
        }
        // Même une petite page n'est pas une preuve de fin : le serveur peut
        // limiter COUNT. Continuer avec le nombre effectivement reçu, pas +500.
    }
    let manifest = WfsManifest {
        source: endpoint.to_owned(),
        layer: layer.identifier.to_owned(),
        bbox: bounds,
        sort_by: layer.sort_by.to_owned(),
        pages,
        features: all_features.len(),
    };
    crate::cache::write_tile(&complete, &serde_json::to_vec_pretty(&manifest)?).await?;
    println!("[OK] {} : {} objet(s)", layer.label, all_features.len());
    Ok(all_features)
}

/// Rejette notamment les erreurs XML OGC parfois renvoyées avec HTTP 200.
fn parse_page(text: &str) -> Result<Value> {
    let page: Value =
        serde_json::from_str(text).context("réponse WFS non GeoJSON (erreur XML possible)")?;
    if page["type"] != "FeatureCollection" {
        bail!("la réponse WFS n'est pas une FeatureCollection");
    }
    let features = page["features"]
        .as_array()
        .ok_or_else(|| anyhow!("liste features WFS absente"))?;
    if features
        .iter()
        .any(|feature| feature["type"] != "Feature" || !feature["properties"].is_object())
    {
        bail!("objet GeoJSON WFS invalide");
    }
    if let Some(returned) = page["numberReturned"].as_u64()
        && returned != features.len() as u64
    {
        bail!("numberReturned WFS incohérent");
    }
    Ok(page)
}

/// Un fichier manquant, tronqué ou appartenant à une autre zone impose une nouvelle requête.
async fn read_complete(
    directory: &Path,
    endpoint: &str,
    layer: &WfsLayer,
    bbox: [f64; 4],
) -> Option<Vec<Value>> {
    let manifest: WfsManifest =
        serde_json::from_slice(&fs::read(directory.join("complete.json")).await.ok()?).ok()?;
    if manifest.source != endpoint
        || manifest.layer != layer.identifier
        || manifest.bbox != bbox
        || manifest.sort_by != layer.sort_by
        || manifest.pages.is_empty()
    {
        return None;
    }
    let mut features = Vec::new();
    for start in manifest.pages {
        if start != features.len() {
            return None;
        }
        let text = fs::read_to_string(directory.join(format!("page-{start:06}.geojson")))
            .await
            .ok()?;
        let page = parse_page(&text).ok()?;
        features.extend(page["features"].as_array()?.iter().cloned());
    }
    (features.len() == manifest.features).then_some(features)
}

#[cfg(test)]
mod tests;
