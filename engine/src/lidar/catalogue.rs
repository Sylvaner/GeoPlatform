//! Contexte : le WFS IGN décrit les emprises LiDAR et les liens des fichiers à télécharger.
//! Usage : find_tiles interroge la zone en Lambert-93 et conserve les pages GeoJSON reçues.
//! La couche actuelle est IGNF_LIDAR-HD_METADONNEE:metadata ; les anciens tableaux
//! d'assemblage ont changé de nom ou ont été retirés. Le champ url_npl désigne le LAZ.
//! Source vérifiable : https://data.geopf.fr/wfs/ows?SERVICE=WFS&REQUEST=GetCapabilities

use crate::{
    models::{Bbox, LidarTile, WfsLayer},
    wfs,
};
use anyhow::{Context, Result, anyhow, bail};
use reqwest::{Client, Url};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub const WFS_URL: &str = wfs::URL;
pub const LIDAR_LAYER: &str = "IGNF_LIDAR-HD_METADONNEE:metadata";

/// Le même lecteur de pages sert aux bâtiments et au catalogue LiDAR : les tests
/// de pagination protègent ainsi les deux usages. Le catalogue est réinterrogé
/// pour découvrir de nouvelles éditions ; les gros LAZ restent en cache.
pub async fn find_tiles(client: &Client, bbox: Bbox, root: &Path) -> Result<Vec<LidarTile>> {
    let layer = WfsLayer {
        identifier: LIDAR_LAYER,
        label: "Catalogue LiDAR",
        sort_by: "coordonnees_nw A,code_mission A,date_edition A",
        output_format: "application/json",
    };
    let directory = root.join("catalogue").join(format!(
        "{}_{}_{}_{}",
        bbox.xmin, bbox.ymin, bbox.xmax, bbox.ymax
    ));
    let features = wfs::download_layer(client, WFS_URL, &layer, bbox, &directory, true).await?;
    let mut seen_urls = BTreeSet::new();
    let mut tiles = Vec::new();
    for feature in features {
        // Sans url_npl, cette métadonnée ne publie pas de nuage de points.
        let Some(tile) = tile_from_feature(&feature)? else {
            continue;
        };
        if seen_urls.insert(tile.url.clone()) {
            tiles.push(tile);
        }
    }
    Ok(tiles)
}

/// Lit le lien officiel et construit un chemin local conservant le dossier d'édition.
fn tile_from_feature(feature: &Value) -> Result<Option<LidarTile>> {
    let properties = &feature["properties"];
    let Some(url_text) = properties["url_npl"]
        .as_str()
        .filter(|url| !url.trim().is_empty())
    else {
        return Ok(None);
    };
    let url = Url::parse(url_text).context("URL de nuage de points invalide")?;
    if url.scheme() != "https" || url.host_str() != Some("data.geopf.fr") {
        bail!("source de fichier LiDAR non prise en charge : {url}");
    }
    let segments: Vec<_> = url
        .path_segments()
        .ok_or_else(|| anyhow!("chemin LiDAR absent"))?
        .collect();
    if segments.len() < 2 {
        bail!("chemin de téléchargement LiDAR incomplet");
    }
    let filename = segments[segments.len() - 1];
    let edition = segments[segments.len() - 2];
    for component in [filename, edition] {
        // Les noms viennent du réseau : refuser séparateurs, .. et caractères spéciaux.
        if component.is_empty()
            || component == "."
            || component == ".."
            || !component
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        {
            bail!("nom de fichier ou d'édition LiDAR invalide : {component}");
        }
    }
    if !filename.to_ascii_lowercase().ends_with(".laz") {
        bail!("le lien url_npl ne désigne pas un fichier LAZ : {filename}");
    }
    Ok(Some(LidarTile {
        id: feature["id"].as_str().unwrap_or(filename).to_owned(),
        url: url_text.to_owned(),
        relative_path: PathBuf::from(edition).join(filename),
        properties: properties.clone(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    const PAGE: &str = include_str!("../../tests/fixtures/lidar-page.json");

    #[test]
    fn official_url_and_acquisition_metadata_are_preserved() {
        let page: Value = serde_json::from_str(PAGE).unwrap();
        let tile = tile_from_feature(&page["features"][0]).unwrap().unwrap();
        assert!(
            tile.relative_path
                .ends_with("LHD_FXX_0827_6796_PTS_LAMB93_IGN69.copc.laz")
        );
        assert!(
            tile.relative_path
                .starts_with("NUALHD_1-0__LAZ_LAMB93_NF_2025-03-12")
        );
        assert_eq!(tile.properties["date_debut_acquisition"], "2023-04-04Z");
    }

    #[test]
    fn missing_point_cloud_is_skipped_and_unexpected_source_is_rejected() {
        let page: Value = serde_json::from_str(PAGE).unwrap();
        let mut feature = page["features"][0].clone();
        feature["properties"]["url_npl"] = Value::Null;
        assert!(tile_from_feature(&feature).unwrap().is_none());
        feature["properties"]["url_npl"] = "https://example.com/edition/tile.laz".into();
        assert!(tile_from_feature(&feature).is_err());
    }
}
