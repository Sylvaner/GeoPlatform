//! Contexte : les pages WFS locales peuvent être complètes, interrompues ou voisines.
//! Usage : réunir les caches utiles, découper à la zone et supprimer les doublons.
//! Les manques restent signalés dans le .blend ; aucun téléchargement n'est lancé.

use super::{
    buildings, geometry,
    models::{ExportConfig, LayerPlan, LidarSamples, TerrainGrid},
    roads, vector_cache, water,
};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::{collections::BTreeSet, path::Path};

fn layer_source(key: &str, c: &ExportConfig) -> Option<(String, String)> {
    let (product, name) = key.split_once('.')?;
    match product {
        "bd_topo" => Some(("BD_TOPO".into(), format!("BDTOPO_V3:{name}"))),
        "cadastre" => Some((
            "CADASTRE".into(),
            "CADASTRALPARCELS.PARCELLAIRE_EXPRESS:parcelle".into(),
        )),
        "bd_topage" => Some((
            format!("BD_TOPAGE/{}", c.vectors.topage_edition),
            format!("sa:{name}_FXX_Topage{}", c.vectors.topage_edition),
        )),
        _ => None,
    }
}

/// Privilégier les identifiants métier stables entre deux sélections du service.
fn identity(feature: &Value) -> String {
    for value in [
        &feature["properties"]["idu"],
        &feature["properties"]["cleabs"],
        &feature["properties"]["gid"],
        &feature["id"],
    ] {
        if value.is_string() || value.is_number() {
            return value.to_string();
        }
    }
    // Sans identifiant, seules des réponses identiques peuvent être fusionnées sûrement.
    feature.to_string()
}

fn read_page(path: &Path) -> Result<Vec<Value>> {
    let page: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    if page["type"] != "FeatureCollection" {
        bail!("page non GeoJSON");
    }
    let features = page["features"]
        .as_array()
        .context("features absent du cache")?;
    if let Some(returned) = page["numberReturned"].as_u64()
        && returned != features.len() as u64
    {
        bail!("numberReturned incohérent");
    }
    Ok(features.clone())
}

pub fn prepare(
    root: &Path,
    c: &ExportConfig,
    t: &TerrainGrid,
    samples: Option<&LidarSamples>,
    warnings: &mut Vec<String>,
) -> Result<Vec<LayerPlan>> {
    let mut layers = Vec::new();
    let mut covered_water = geo::MultiPolygon(Vec::new());
    let mut ordered_layers: Vec<_> = c.layers.iter().collect();
    ordered_layers.sort_by_key(|(key, _)| (water::priority(key), key.as_str()));
    for (key, style) in ordered_layers {
        if !style.enabled {
            continue;
        }
        let Some((product, id)) = layer_source(key, c) else {
            continue;
        };
        let caches = vector_cache::find(
            &root.join(product),
            &id,
            t,
            c.vectors.allow_partial_cache,
            warnings,
        )?;
        let mut layer = LayerPlan {
            name: key.clone(),
            style: style.clone(),
            sources: Vec::new(),
            partial: !vector_cache::covers(&caches, t),
            objects: Vec::new(),
        };
        let mut seen = BTreeSet::new();
        for cache in &caches {
            layer.sources.push(cache.source_path.display().to_string());
            layer.partial |= !cache.complete;
            let mut read = 0;
            for (start, path) in &cache.pages {
                if *start != read {
                    if !c.vectors.allow_partial_cache {
                        bail!("ordre des pages incohérent : {}", path.display());
                    }
                    layer.partial = true;
                }
                let features = match read_page(path) {
                    Ok(features) => features,
                    Err(error) if c.vectors.allow_partial_cache => {
                        warnings.push(format!("{key} : page ignorée {} : {error}", path.display()));
                        layer.partial = true;
                        continue;
                    }
                    Err(error) => {
                        return Err(error)
                            .with_context(|| format!("page de cache {}", path.display()));
                    }
                };
                read += features.len();
                for feature in features {
                    if feature["type"] != "Feature" || !feature["properties"].is_object() {
                        if !c.vectors.allow_partial_cache {
                            bail!("objet GeoJSON invalide");
                        }
                        warnings.push(format!(
                            "{key} : objet invalide ignoré dans {}",
                            path.display()
                        ));
                        layer.partial = true;
                        continue;
                    }
                    if !seen.insert(identity(&feature)) {
                        continue;
                    }
                    let feature = if water::is_water(key) {
                        let Some(remaining) = water::uncovered(&feature, &mut covered_water, t)?
                        else {
                            continue;
                        };
                        remaining
                    } else {
                        feature
                    };
                    let Some(mut object) = geometry::prepare(&feature, key, style, t, c)? else {
                        continue;
                    };
                    if key == "bd_topo.batiment"
                        && let Some(samples) = samples
                    {
                        buildings::improve(&feature, &mut object, t, c, samples)?;
                    }
                    if layer.objects.len() >= c.vectors.max_objects_per_layer {
                        bail!("limite d'objets atteinte pour {key}");
                    }
                    layer.objects.push(object);
                }
            }
            if cache
                .expected_features
                .is_some_and(|expected| expected != read)
            {
                if !c.vectors.allow_partial_cache {
                    bail!("cache WFS incomplet : {}", cache.source_path.display());
                }
                warnings.push(format!(
                    "{key} : cache annoncé complet mais pages incomplètes ({read} objets lus)"
                ));
                layer.partial = true;
            }
        }
        if caches.is_empty() {
            let warning = format!("{key} : aucune donnée locale utilisable dans la zone");
            if c.vectors.missing_layers == "error" {
                bail!("{warning}");
            }
            warnings.push(warning);
        } else if layer.partial {
            warnings.push(format!("{key} : données partielles, {} objet(s) dans la zone ; les zones sans données restent absentes",layer.objects.len()));
        }
        println!(
            "[EXPORT] {key} : {} objet(s){}",
            layer.objects.len(),
            if layer.partial {
                " (cache partiel)"
            } else {
                ""
            }
        );
        roads::join(&mut layer, t, c)?;
        if !layer.objects.is_empty() {
            layers.push(layer);
        }
    }
    Ok(layers)
}
