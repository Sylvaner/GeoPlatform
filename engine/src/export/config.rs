//! Contexte : le TOML contient les réglages d'export et accepte des commentaires explicatifs.
//! Usage : les sections fournies complètent export.default.toml puis le résultat est validé.
//! Une faute de nom reste une erreur : une option mal orthographiée ne doit pas être ignorée.

use super::models::ExportConfig;
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const DEFAULT_TOML: &str = include_str!("../../export.default.toml");

/// Complète les sections absentes, tout en refusant une clé inconnue ou un mauvais type.
fn merge(default: &mut Value, provided: Value, path: &str) -> Result<()> {
    if let (Some(destination), Some(source)) = (default.as_object_mut(), provided.as_object()) {
        for (key, value) in source {
            let field = destination
                .get_mut(key)
                .with_context(|| format!("paramètre inconnu : {path}{key}"))?;
            merge(field, value.clone(), &format!("{path}{key}."))?;
        }
    } else {
        *default = provided;
    }
    Ok(())
}

pub fn parse(text: &str) -> Result<ExportConfig> {
    // Value est seulement un arbre interne pour fusionner simplement les sections.
    // La configuration de l'utilisateur reste entièrement en TOML.
    let mut value = serde_json::to_value(toml::from_str::<toml::Value>(DEFAULT_TOML)?)?;
    merge(
        &mut value,
        serde_json::to_value(
            toml::from_str::<toml::Value>(text).context("TOML d'export invalide")?,
        )?,
        "",
    )?;
    let config: ExportConfig =
        serde_json::from_value(value).context("type de paramètre d'export invalide")?;
    validate(&config)?;
    Ok(config)
}

pub fn read(path: &Path) -> Result<ExportConfig> {
    parse(&std::fs::read_to_string(path).with_context(|| {
        format!(
            "lecture de {} ; utiliser --init-config pour le créer",
            path.display()
        )
    })?)
}

pub fn resolve(base: &Path, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    let resolved = if path.is_absolute() {
        path
    } else {
        base.join(path)
    };
    // Ne pas rattacher un autre disque ou un partage réseau au dossier du TOML.
    // Retirer aussi le préfixe étendu produit par canonicalize sur Windows.
    super::paths::for_blender(&resolved)
}

fn positive(name: &str, value: f64) -> Result<()> {
    if !value.is_finite() || value <= 0. {
        bail!("{name} doit être fini et strictement positif");
    }
    Ok(())
}

pub fn validate(c: &ExportConfig) -> Result<()> {
    if c.schema_version != 1 {
        bail!("version de configuration non prise en charge");
    }
    crate::coordinates::to_lambert93(c.area.center[0], c.area.center[1], &c.area.crs)?;
    for (name, value) in [
        ("area.side_m", c.area.side_m),
        ("terrain.grid_step_m", c.terrain.grid_step_m),
        (
            "terrain.vertical_exaggeration",
            c.terrain.vertical_exaggeration,
        ),
        ("vectors.line_sample_step_m", c.vectors.line_sample_step_m),
        ("vegetation.cell_size_m", c.vegetation.cell_size_m),
        ("camera.framing_margin", c.camera.framing_margin),
        ("camera.focal_length_mm", c.camera.focal_length_mm),
        ("camera.clip_end_m", c.camera.clip_end_m),
        ("view.clip_end_m", c.view.clip_end_m),
        ("water.surface_clearance_m", c.water.surface_clearance_m),
        ("water.thickness_m", c.water.thickness_m),
    ] {
        positive(name, value)?;
    }
    if !["auto", "alti", "lidar"].contains(&c.terrain.source.as_str()) {
        bail!("terrain.source : auto, alti ou lidar attendu");
    }
    if !["min", "absolute"].contains(&c.terrain.altitude_origin.as_str()) {
        bail!("terrain.altitude_origin : min ou absolute attendu");
    }
    if !["nearest", "bilinear"].contains(&c.ortho.interpolation.as_str()) {
        bail!("ortho.interpolation : nearest ou bilinear attendu");
    }
    if !["skip", "error"].contains(&c.vectors.missing_layers.as_str()) {
        bail!("vectors.missing_layers : skip ou error attendu");
    }
    if !["orthographic", "perspective"].contains(&c.camera.projection.as_str()) {
        bail!("projection de caméra inconnue");
    }
    if !["BLENDER_EEVEE", "CYCLES"].contains(&c.render.engine.as_str()) {
        bail!("moteur de rendu non pris en charge");
    }
    if c.terrain.valid_altitude_min_m >= c.terrain.valid_altitude_max_m {
        bail!("intervalle d'altitudes invalide");
    }
    if c.ortho.max_tiles == 0 || c.ortho.max_loaded_tiles == 0 {
        bail!("limite de tuiles invalide");
    }
    let b = &c.buildings;
    if !["auto", "bd_topo", "lidar"].contains(&b.source.as_str()) {
        bail!("buildings.source : auto, bd_topo ou lidar attendu");
    }
    for (name, value) in [
        ("buildings.cell_size_m", b.cell_size_m),
        ("buildings.min_density_per_m2", b.min_density_per_m2),
        ("buildings.min_height_m", b.min_height_m),
        ("buildings.max_cell_spread_m", b.max_cell_spread_m),
    ] {
        positive(name, value)?;
    }
    if !b.min_coverage.is_finite()
        || b.min_coverage <= 0.
        || b.min_coverage > 1.
        || !b.max_height_m.is_finite()
        || b.max_height_m < b.min_height_m
        || b.min_points == 0
        || b.max_cells == 0
        || b.max_cells_per_building == 0
        || b.fill_radius_cells > 20
    {
        bail!("seuils de sélection des bâtiments invalides");
    }
    if c.terrain.max_vertices < 4
        || c.water.max_vertices < 3
        || c.vectors.max_road_vertices < 3
        || c.lidar.display_stride == 0
        || c.render.samples == 0
        || c.blender.threads == 0
        || c.view.outliner_timeout_seconds == 0
        || c.render.resolution_x == 0
        || c.render.resolution_y == 0
    {
        bail!("une limite ou résolution est nulle");
    }
    if c.camera.elevation_deg <= 0. || c.camera.elevation_deg > 90. || c.camera.framing_margin < 1.
    {
        bail!("cadrage de caméra invalide");
    }
    if c.vegetation.min_height_m <= 0.
        || c.vegetation.max_height_m < c.vegetation.min_height_m
        || c.vegetation.canopy_subdivisions > 3
    {
        bail!("dimensions de végétation invalides");
    }
    if Path::new(&c.paths.output_file)
        .extension()
        .and_then(|s| s.to_str())
        != Some("blend")
    {
        bail!("output_file doit se terminer par .blend");
    }
    for (name, layer) in &c.layers {
        if layer
            .color
            .iter()
            .any(|v| !v.is_finite() || *v < 0. || *v > 1.)
            || layer.line_width_m <= 0.
            || !layer.z_offset_m.is_finite()
            || !layer.fallback_height_m.is_finite()
        {
            bail!("style invalide : {name}");
        }
    }
    if !c.layers.values().any(|layer| layer.enabled) {
        bail!("aucune couche activée");
    }
    Ok(())
}
