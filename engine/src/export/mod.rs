//! Contexte : export hors ligne du cache vers un fichier Blender autonome.
//! Usage : geo-export lit export.toml ; voir EXPORT.md pour les paramètres.
//! Étapes : configuration, terrain, texture, objets vectoriels, végétation, Blender.
//! Aucun client HTTP n'est utilisé par ce module, même en cas de données manquantes.

mod blender;
mod building_source;
mod buildings;
pub mod config;
mod files;
mod geometry;
mod lidar;
pub mod models;
mod ortho;
mod paths;
mod roads;
mod terrain;
mod vector_cache;
pub(crate) mod vectors;
mod vegetation;
mod water;

use anyhow::{Context, Result, bail};
use models::{ExportArguments, ScenePlan};
use std::{io::Write, path::Path};

pub fn run(arguments: ExportArguments) -> Result<()> {
    if let Some(path) = arguments.init_config {
        // create_new évite d'écraser une configuration déjà personnalisée.
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.write_all(config::DEFAULT_TOML.as_bytes())?;
        println!("Configuration créée : {}", path.display());
        return Ok(());
    }
    let config_path =
        paths::for_blender(&std::fs::canonicalize(&arguments.config).with_context(|| {
            format!(
                "configuration inaccessible : {} ; utiliser --init-config pour la créer",
                arguments.config.display()
            )
        })?);
    let base = config_path.parent().unwrap();
    let c = config::read(&config_path)?;
    let requested_root = config::resolve(base, &c.paths.cache_root);
    let root = paths::for_blender(&std::fs::canonicalize(&requested_root).with_context(|| {
        format!(
            "racine du cache inaccessible : {}",
            requested_root.display()
        )
    })?);
    let output = config::resolve(base, &c.paths.output_file);
    let preview = config::resolve(base, &c.render.preview_file);
    if output.exists() && !c.paths.overwrite {
        bail!(
            "le fichier existe déjà : {} ; activer paths.overwrite pour le remplacer",
            output.display()
        );
    }
    if c.render.preview && preview.exists() && !c.paths.overwrite {
        bail!("l'image d'aperçu existe déjà ; activer paths.overwrite");
    }
    println!("[EXPORT] Cache : {}", root.display());
    println!("[EXPORT] Sortie : {}", output.display());
    // Trouver Blender avant de préparer des fichiers volumineux. prepare_only
    // permet de vérifier le cache même sur un poste où Blender n'est pas installé.
    let executable = if c.paths.prepare_only {
        None
    } else {
        Some(blender::executable(&c, base)?)
    };
    let (grid, mut samples, mut warnings) = terrain::prepare(&root, &c)?;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let work = config::resolve(base, &c.paths.work_directory)
        .join(format!("export-{}-{unique}", std::process::id()));
    std::fs::create_dir_all(&work)
        .with_context(|| format!("création du dossier de travail : {}", work.display()))?;
    let mut layers = vectors::prepare(&root, &c, &grid, Some(&samples), &mut warnings)?;
    let ortho = if c.layers["orthophoto"].enabled
        || (c.buildings.texture_roofs && layers.iter().any(|l| l.name == "bd_topo.batiment"))
    {
        Some(ortho::prepare(&root, &c, &grid, &mut layers)?)
    } else {
        None
    };
    let trees = vegetation::prepare(&grid, &mut samples, &c)?;
    if !samples.files.is_empty() && samples.report.coverage_fraction < 0.999 {
        warnings.push(format!("Les dalles LiDAR locales couvrent {:.1} % de la zone : la végétation et les toits LiDAR ne sont disponibles que dans cette couverture.",samples.report.coverage_fraction*100.));
    }
    if c.layers["vegetation"].enabled && trees.is_empty() && !samples.files.is_empty() {
        warnings.push(format!("Aucun arbre créé : {} cellules de canopée, seuils de hauteur {}..{} m ; consulter le bilan LiDAR dans le .blend.",samples.canopy.len(),c.vegetation.min_height_m,c.vegetation.max_height_m));
    }
    let points = samples
        .display
        .into_iter()
        .map(|(p, class)| (grid.local(p[0], p[1], p[2]), class))
        .collect();
    for warning in &warnings {
        eprintln!("[EXPORT] {warning}");
    }
    let partial = output.with_extension("part.blend");
    let plan = ScenePlan {
        config: c.clone(),
        terrain: grid,
        ortho,
        configuration_toml: toml::to_string_pretty(&c)?,
        configuration_source: std::fs::read_to_string(&config_path)?,
        layers,
        trees,
        points,
        warnings,
        lidar_files: samples.files,
        lidar_report: samples.report,
        output: paths::text(&partial),
        preview: paths::text(&preview),
    };
    let plan_path = work.join("scene-plan.json");
    let mut file = std::io::BufWriter::new(std::fs::File::create(&plan_path)?);
    serde_json::to_writer(&mut file, &plan)?;
    file.flush()?;
    drop(file);
    if c.paths.prepare_only {
        println!("Plan préparé : {}", plan_path.display());
        return Ok(());
    }
    let script = blender::write_scripts(&work)?;
    blender::launch(&executable.unwrap(), &script, &plan_path, &c)?;
    if !partial.is_file() {
        bail!("Blender n'a pas produit le fichier attendu");
    }
    // Le nom final apparaît seulement après la réussite de Blender et l'intégration des images.
    std::fs::rename(&partial, &output)
        .with_context(|| format!("publication du fichier : {}", output.display()))?;
    println!("Fichier Blender autonome : {}", output.display());
    if !c.paths.keep_work_files {
        cleanup_work(&work)?;
    }
    Ok(())
}

/// Efface uniquement notre sous-dossier unique, jamais le cache ni le dossier configuré lui-même.
fn cleanup_work(work: &Path) -> Result<()> {
    let canonical = std::fs::canonicalize(work)?;
    let parent = std::fs::canonicalize(work.parent().context("travail sans parent")?)?;
    let name = work.file_name().and_then(|v| v.to_str()).unwrap_or("");
    if !canonical.starts_with(&parent)
        || !name.starts_with(&format!("export-{}-", std::process::id()))
    {
        bail!("dossier de travail non reconnu : suppression refusée");
    }
    std::fs::remove_dir_all(canonical)?;
    Ok(())
}

#[cfg(test)]
mod partial_tests;
#[cfg(test)]
mod path_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod water_tests;
