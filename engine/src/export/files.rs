//! Contexte : l'export cherche uniquement des fichiers locaux, jamais des URL distantes.
//! Usage : list trouve les pages vectorielles ; raster choisit une grille sans parcourir ses tuiles.
//! Aucun parcours ne suit les liens symboliques rencontrés dans les sous-dossiers.

use anyhow::Result;
use std::path::{Path, PathBuf};

pub fn list(root: &Path, name: &str) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    if !root.is_dir() {
        return Ok(found);
    }
    let mut folders = vec![root.to_owned()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(folder)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                folders.push(entry.path());
            } else if entry.file_name() == name {
                found.push(entry.path());
            }
        }
    }
    found.sort();
    Ok(found)
}

pub fn raster(root: &Path) -> Result<Option<PathBuf>> {
    let mut candidates = Vec::new();
    if !root.is_dir() {
        return Ok(None);
    }
    let mut folders = vec![root.to_owned()];
    while let Some(folder) = folders.pop() {
        let path = folder.join("matrix.json");
        if path.is_file() {
            let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
            let resolution = value["matrix"]["nominal_resolution_m_per_px"]
                .as_f64()
                .unwrap_or(f64::INFINITY);
            candidates.push((resolution, path));
            // Ce dossier est une grille : ses sous-dossiers sont les lignes de
            // tuiles. Les parcourir n'aide pas à choisir une matrice et devient
            // très lent sur un partage réseau contenant des milliers d'images.
            continue;
        }
        for entry in std::fs::read_dir(folder)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() && !kind.is_symlink() {
                folders.push(entry.path());
            }
        }
    }
    // Le chemin départage deux résolutions identiques de manière reproductible.
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    Ok(candidates.into_iter().next().map(|(_, path)| path))
}
