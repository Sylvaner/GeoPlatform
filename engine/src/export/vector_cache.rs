//! Contexte : une erreur WFS ne rend pas inutiles les pages déjà téléchargées.
//! Usage : trouver les caches qui recoupent la zone, identifier leurs pages et leur statut.
//! Un cache complet couvrant toute la zone reste prioritaire. Sinon les morceaux sont réunis.

use super::{
    files,
    models::{TerrainGrid, VectorCache},
};
use crate::models::{WfsManifest, WfsRequest};
use anyhow::{Context, Result, bail};
use geo::{Area, BooleanOps, Coord, MultiPolygon, Rect};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub fn contains(b: [f64; 4], t: &TerrainGrid) -> bool {
    b[0] <= t.xmin + 1e-6
        && b[1] <= t.ymin + 1e-6
        && b[2] >= t.xmin + t.side - 1e-6
        && b[3] >= t.ymin + t.side - 1e-6
}

/// Plusieurs caches voisins terminés peuvent, ensemble, couvrir toute la zone.
pub fn covers(caches: &[VectorCache], t: &TerrainGrid) -> bool {
    let mut missing = MultiPolygon(vec![super::geometry::bounds(t)]);
    for cache in caches.iter().filter(|cache| cache.complete) {
        let b = cache.bbox;
        let polygon =
            Rect::new(Coord { x: b[0], y: b[1] }, Coord { x: b[2], y: b[3] }).to_polygon();
        missing = missing.difference(&polygon);
    }
    missing.unsigned_area() <= (t.side * t.side * 1e-10).max(1e-8)
}

fn intersects(b: [f64; 4], t: &TerrainGrid) -> bool {
    b[0] < t.xmin + t.side && b[1] < t.ymin + t.side && b[2] > t.xmin && b[3] > t.ymin
}

/// Les anciens téléchargements n'avaient pas request.json : leur dossier nomme
/// exactement les quatre bornes Lambert-93, sans arrondi. Aucun nom libre n'est deviné.
fn legacy_bbox(directory: &Path) -> Option<[f64; 4]> {
    let name = directory.parent()?.file_name()?.to_str()?;
    let values = name
        .split('_')
        .map(str::parse::<f64>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .ok()?;
    values.try_into().ok()
}

fn local_pages(directory: &Path) -> Result<Vec<(usize, PathBuf)>> {
    let mut pages = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if let Some(number) = name
            .strip_prefix("page-")
            .and_then(|s| s.strip_suffix(".geojson"))
            && let Ok(start) = number.parse::<usize>()
        {
            pages.push((start, entry.path()));
        }
    }
    pages.sort();
    Ok(pages)
}

fn identify(directory: &Path, id: &str) -> Result<Option<VectorCache>> {
    let complete = directory.join("complete.json");
    let request = directory.join("request.json");
    let cache = if complete.is_file() {
        let m: WfsManifest = serde_json::from_slice(&std::fs::read(&complete)?)?;
        if m.layer != id {
            return Ok(None);
        }
        VectorCache {
            directory: directory.into(),
            source_path: complete,
            bbox: m.bbox,
            complete: true,
            pages: m
                .pages
                .into_iter()
                .map(|start| (start, directory.join(format!("page-{start:06}.geojson"))))
                .collect(),
            expected_features: Some(m.features),
        }
    } else {
        let (bbox, source_path) = if request.is_file() {
            let m: WfsRequest = serde_json::from_slice(&std::fs::read(&request)?)?;
            if m.layer != id {
                return Ok(None);
            }
            if m.crs != "EPSG:2154" {
                bail!("cache WFS partiel : CRS non pris en charge");
            }
            (m.bbox, request)
        } else {
            let Some(bbox) = legacy_bbox(directory) else {
                return Ok(None);
            };
            (bbox, directory.to_owned())
        };
        VectorCache {
            directory: directory.into(),
            source_path,
            bbox,
            complete: false,
            pages: local_pages(directory)?,
            expected_features: None,
        }
    };
    let b = cache.bbox;
    if b.iter().any(|v| !v.is_finite()) || b[0] >= b[2] || b[1] >= b[3] {
        bail!("emprise de cache invalide");
    }
    Ok(Some(cache))
}

pub fn find(
    root: &Path,
    id: &str,
    t: &TerrainGrid,
    allow_partial: bool,
    warnings: &mut Vec<String>,
) -> Result<Vec<VectorCache>> {
    let mut directories = BTreeSet::new();
    for name in ["complete.json", "request.json"] {
        for path in files::list(root, name)? {
            directories.insert(path.parent().context("cache sans dossier")?.to_owned());
        }
    }
    // Compatibilité des caches interrompus avant l'ajout de request.json.
    // Le dossier de produit et le nom exact de couche évitent de mélanger les sources.
    if allow_partial && root.is_dir() {
        let name = id
            .rsplit(':')
            .next()
            .context("identifiant de couche vide")?;
        for entry in std::fs::read_dir(root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let directory = entry.path().join(name);
            if std::fs::symlink_metadata(&directory)
                .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
            {
                directories.insert(directory);
            }
        }
    }
    let mut caches = Vec::new();
    for directory in directories {
        match identify(&directory, id) {
            Ok(Some(cache))
                if intersects(cache.bbox, t)
                    && (cache.complete || (allow_partial && !cache.pages.is_empty())) =>
            {
                caches.push(cache)
            }
            Ok(_) => {}
            Err(error) if allow_partial => {
                warnings.push(format!("cache ignoré {} : {error}", directory.display()))
            }
            Err(error) => {
                return Err(error).with_context(|| format!("cache {}", directory.display()));
            }
        }
    }
    // Une sélection complète suffit ; ne pas ajouter des objets d'une vieille
    // sélection partielle lorsqu'on possède déjà toute la zone demandée.
    caches.sort_by(|a, b| {
        b.complete
            .cmp(&a.complete)
            .then_with(|| {
                let area = |b: [f64; 4]| (b[2] - b[0]) * (b[3] - b[1]);
                area(a.bbox).total_cmp(&area(b.bbox))
            })
            .then_with(|| a.directory.cmp(&b.directory))
    });
    if let Some(index) = caches
        .iter()
        .position(|cache| cache.complete && contains(cache.bbox, t))
    {
        return Ok(vec![caches.remove(index)]);
    }
    if !allow_partial {
        caches.clear();
    }
    Ok(caches)
}
