//! Contexte : transformer l'enveloppe supérieure du LiDAR en bloc STL étanche.
//! Usage : la zone, la résolution et l'exagération verticale viennent de export.toml ;
//! la largeur physique, le socle et le fichier de sortie sont des options de geo-stl.
//! Aucune altitude RGE ALTI et aucun traitement Blender ne sont utilisés ici.

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use copc_core::Bounds as CopcBounds;
use copc_reader::{BoundsSelection, CopcReader, LodSelection};
use las::{Point, Reader};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs::File,
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use crate::export::{
    config,
    models::{ExportConfig, ObjectPlan, TerrainGrid},
};

/// Options propres à l'objet imprimable. Les données géographiques restent dans export.toml.
#[derive(Parser, Debug)]
#[command(
    name = "geo-stl",
    about = "Crée un bloc STL imprimable à partir de la surface LiDAR locale"
)]
pub struct StlArguments {
    /// Fichier TOML définissant le centre, la zone, le cache et la résolution LiDAR.
    #[arg(long, default_value = "export.toml")]
    pub config: PathBuf,

    /// Fichier STL binaire à produire, relatif au dossier du fichier TOML.
    #[arg(long, default_value = "exports/modele.stl")]
    pub output: PathBuf,

    /// Largeur physique du carré imprimé, en millimètres.
    #[arg(long)]
    pub width_mm: f64,

    /// Épaisseur du socle sous le point LiDAR le plus bas, en millimètres.
    #[arg(long, default_value_t = 3.0)]
    pub base_mm: f64,

    /// Niveau de détail LiDAR : fast, balanced ou full.
    #[arg(long, value_enum, default_value_t = StlQuality::Balanced)]
    pub quality: StlQuality,

    /// Ajoute les volumes de bâtiments BD TOPO sur la surface LiDAR.
    #[arg(long)]
    pub buildings: bool,

    /// Ajoute les routes BD TOPO sous forme de rubans imprimables en relief.
    #[arg(long)]
    pub roads: bool,

    /// Largeur physique minimale des routes, en millimètres.
    #[arg(long, default_value_t = 1.0)]
    pub road_min_width_mm: f64,

    /// Relief physique des routes au-dessus du terrain, en millimètres.
    #[arg(long, default_value_t = 0.6)]
    pub road_height_mm: f64,

    /// Ajoute des arbres simplifiés aux sommets de végétation LiDAR.
    #[arg(long)]
    pub trees: bool,

    /// Hauteur physique minimale d'un arbre imprimé, en millimètres.
    #[arg(long, default_value_t = 5.0)]
    pub tree_min_height_mm: f64,

    /// Multiplie la hauteur mesurée des arbres pour améliorer leur lisibilité.
    #[arg(long, default_value_t = 1.3)]
    pub tree_height_scale: f64,

    /// Forme du feuillage : rounded (feuillu arrondi) ou conifer (sapin).
    #[arg(long, value_enum, default_value_t = TreeShape::Rounded)]
    pub tree_shape: TreeShape,

    /// Diamètre physique minimal des troncs, en millimètres.
    #[arg(long, default_value_t = 1.2)]
    pub tree_trunk_diameter_mm: f64,

    /// Diamètre physique minimal des houppiers, en millimètres.
    #[arg(long, default_value_t = 3.0)]
    pub tree_crown_diameter_mm: f64,

    /// Lisse géométriquement le relief. Sans valeur, applique deux passes.
    #[arg(long, num_args = 0..=1, default_missing_value = "2", value_name = "PASSES")]
    pub smooth: Option<usize>,

    /// Intensité de chaque passe de lissage, entre 0 (aucun) et 1 (maximal).
    #[arg(long, default_value_t = 0.35)]
    pub smooth_strength: f64,

    /// Ignore le cache de surface existant et le reconstruit.
    #[arg(long)]
    pub rebuild_cache: bool,

    /// Autorise le remplacement d'un fichier STL existant.
    #[arg(long)]
    pub overwrite: bool,
}

/// Options du binaire consacré aux maquettes topographiques en strates.
/// Elles restent volontairement identiques à `geo-stl`, hormis la hauteur
/// réelle d'une couche et le nom de sortie par défaut.
#[derive(Parser, Debug)]
#[command(
    name = "geo-stl-layers",
    about = "Crée un bloc STL en strates d'altitude à partir de la surface LiDAR locale"
)]
pub struct LayeredStlArguments {
    /// Fichier TOML définissant le centre, la zone, le cache et la résolution LiDAR.
    #[arg(long, default_value = "export.toml")]
    pub config: PathBuf,

    /// Fichier STL binaire à produire, relatif au dossier du fichier TOML.
    #[arg(long, default_value = "exports/modele-couches.stl")]
    pub output: PathBuf,

    /// Largeur physique du carré imprimé, en millimètres.
    #[arg(long)]
    pub width_mm: f64,

    /// Hauteur réelle séparant deux strates topographiques, en mètres.
    #[arg(long, default_value_t = 1.0)]
    pub layer_height_m: f64,

    /// Épaisseur du socle sous le point LiDAR le plus bas, en millimètres.
    #[arg(long, default_value_t = 3.0)]
    pub base_mm: f64,

    /// Niveau de détail LiDAR : fast, balanced ou full.
    #[arg(long, value_enum, default_value_t = StlQuality::Balanced)]
    pub quality: StlQuality,

    /// Ajoute les volumes de bâtiments BD TOPO sur la surface LiDAR.
    #[arg(long)]
    pub buildings: bool,

    /// Ajoute les routes BD TOPO sous forme de rubans imprimables en relief.
    #[arg(long)]
    pub roads: bool,

    /// Largeur physique minimale des routes, en millimètres.
    #[arg(long, default_value_t = 1.0)]
    pub road_min_width_mm: f64,

    /// Relief physique des routes au-dessus du terrain, en millimètres.
    #[arg(long, default_value_t = 0.6)]
    pub road_height_mm: f64,

    /// Ajoute des arbres simplifiés aux sommets de végétation LiDAR.
    #[arg(long)]
    pub trees: bool,

    /// Hauteur physique minimale d'un arbre imprimé, en millimètres.
    #[arg(long, default_value_t = 5.0)]
    pub tree_min_height_mm: f64,

    /// Multiplie la hauteur mesurée des arbres pour améliorer leur lisibilité.
    #[arg(long, default_value_t = 1.3)]
    pub tree_height_scale: f64,

    /// Forme du feuillage : rounded (feuillu arrondi) ou conifer (sapin).
    #[arg(long, value_enum, default_value_t = TreeShape::Rounded)]
    pub tree_shape: TreeShape,

    /// Diamètre physique minimal des troncs, en millimètres.
    #[arg(long, default_value_t = 1.2)]
    pub tree_trunk_diameter_mm: f64,

    /// Diamètre physique minimal des houppiers, en millimètres.
    #[arg(long, default_value_t = 3.0)]
    pub tree_crown_diameter_mm: f64,

    /// Lisse le relevé avant de calculer les courbes de niveau. Sans valeur : deux passes.
    #[arg(long, num_args = 0..=1, default_missing_value = "2", value_name = "PASSES")]
    pub smooth: Option<usize>,

    /// Intensité de chaque passe de lissage, entre 0 (aucun) et 1 (maximal).
    #[arg(long, default_value_t = 0.35)]
    pub smooth_strength: f64,

    /// Ignore le cache de surface existant et le reconstruit.
    #[arg(long)]
    pub rebuild_cache: bool,

    /// Autorise le remplacement d'un fichier STL existant.
    #[arg(long)]
    pub overwrite: bool,
}

impl From<LayeredStlArguments> for StlArguments {
    fn from(arguments: LayeredStlArguments) -> Self {
        Self {
            config: arguments.config,
            output: arguments.output,
            width_mm: arguments.width_mm,
            base_mm: arguments.base_mm,
            quality: arguments.quality,
            buildings: arguments.buildings,
            roads: arguments.roads,
            road_min_width_mm: arguments.road_min_width_mm,
            road_height_mm: arguments.road_height_mm,
            trees: arguments.trees,
            tree_min_height_mm: arguments.tree_min_height_mm,
            tree_height_scale: arguments.tree_height_scale,
            tree_shape: arguments.tree_shape,
            tree_trunk_diameter_mm: arguments.tree_trunk_diameter_mm,
            tree_crown_diameter_mm: arguments.tree_crown_diameter_mm,
            smooth: arguments.smooth,
            smooth_strength: arguments.smooth_strength,
            rebuild_cache: arguments.rebuild_cache,
            overwrite: arguments.overwrite,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum TerrainStyle {
    Continuous,
    Layered { layer_height_m: f64 },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum StlQuality {
    /// Environ un point COPC par cellule du maillage final.
    Fast,
    /// Environ seize points COPC par cellule ; bon compromis pour maisons et arbres.
    Balanced,
    /// Tous les points LiDAR, sans réduction.
    Full,
}

impl StlQuality {
    fn target_spacing(self, grid_step: f64) -> Option<f64> {
        match self {
            Self::Fast => Some(grid_step),
            Self::Balanced => Some(grid_step / 4.),
            Self::Full => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum TreeShape {
    /// Houppier ovoïde et arrondi, descendant bas autour du tronc.
    Rounded,
    /// Trois étages coniques imbriqués pour représenter un sapin.
    Conifer,
}

#[derive(Debug)]
struct Surface {
    count: usize,
    heights_m: Vec<f64>,
    heights_without_buildings_m: Vec<f64>,
    ground_m: Vec<f64>,
    canopy_tops: Vec<[f64; 3]>,
    minimum_m: f64,
    points_read: u64,
    points_used: u64,
    points_available: u64,
    files_used: usize,
    interpolated: usize,
    cached: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct SurfaceCacheKey {
    schema_version: u32,
    xmin: f64,
    ymin: f64,
    side_m: f64,
    count: usize,
    grid_step_m: f64,
    valid_altitude_min_m: f64,
    valid_altitude_max_m: f64,
    lidar_ground_class: u8,
    lidar_fill_radius_cells: usize,
    vegetation_classes: Vec<u8>,
    vegetation_cell_size_m: f64,
    quality: StlQuality,
    files: Vec<LidarFileStamp>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct LidarFileStamp {
    path: String,
    size: u64,
    modified_ns: u64,
}

#[derive(Debug, Deserialize, Serialize)]
struct SurfaceCacheReport {
    points_read: u64,
    points_used: u64,
    points_available: u64,
    files_used: usize,
    interpolated: usize,
}

#[derive(Debug, Deserialize, Serialize)]
struct SurfaceCacheHeader {
    key: SurfaceCacheKey,
    report: SurfaceCacheReport,
}

struct SurfaceBuilder<'a> {
    settings: &'a ExportConfig,
    xmin: f64,
    ymin: f64,
    count: usize,
    step: f64,
    heights: Vec<Option<f64>>,
    heights_without_buildings: Vec<Option<f64>>,
    ground: Vec<Option<f64>>,
    canopy: BTreeMap<(i64, i64), [f64; 3]>,
    points_read: u64,
    points_used: u64,
    points_available: u64,
    files_used: usize,
}

#[derive(Debug)]
struct PrintableTree {
    x_m: f64,
    y_m: f64,
    height_m: f64,
    crown_radius_m: f64,
    trunk_radius_m: f64,
}

#[derive(Debug)]
struct RoadTopology {
    vertices: Vec<[f64; 2]>,
    triangles: Vec<[usize; 3]>,
    boundary: Vec<(usize, usize)>,
}

#[derive(Default)]
struct VectorFeatures {
    buildings: Vec<ObjectPlan>,
    roads: Vec<ObjectPlan>,
}

pub fn run(arguments: StlArguments) -> Result<()> {
    run_with_style(arguments, TerrainStyle::Continuous).map(|_| ())
}

/// Exécute un STL continu et renvoie le cache LiDAR utilisé pour le produire.
pub fn run_with_cache(arguments: StlArguments) -> Result<PathBuf> {
    run_with_style(arguments, TerrainStyle::Continuous)
}

/// Construit uniquement la surface cache, sans écrire ni trianguler un STL final.
pub fn warm_surface_cache(config_path: &Path, quality: StlQuality) -> Result<PathBuf> {
    let config_path = std::fs::canonicalize(config_path)
        .with_context(|| format!("configuration inaccessible : {}", config_path.display()))?;
    let base = config_path
        .parent()
        .context("le fichier de configuration n'a pas de dossier parent")?;
    let settings = config::read(&config_path)?;
    let requested_root = resolve(base, Path::new(&settings.paths.cache_root));
    let cache_root = std::fs::canonicalize(&requested_root).with_context(|| {
        format!(
            "racine du cache inaccessible : {}",
            requested_root.display()
        )
    })?;
    let (center_x, center_y) = crate::coordinates::to_lambert93(
        settings.area.center[0],
        settings.area.center[1],
        &settings.area.crs,
    )?;
    let xmin = center_x - settings.area.side_m / 2.;
    let ymin = center_y - settings.area.side_m / 2.;
    read_surface_with_cache(&cache_root, &settings, xmin, ymin, quality, false)
        .map(|(_, path)| path)
}

pub fn run_layered(arguments: LayeredStlArguments) -> Result<()> {
    positive("layer-height-m", arguments.layer_height_m)?;
    let layer_height_m = arguments.layer_height_m;
    run_with_style(arguments.into(), TerrainStyle::Layered { layer_height_m }).map(|_| ())
}

fn run_with_style(arguments: StlArguments, terrain_style: TerrainStyle) -> Result<PathBuf> {
    validate_arguments(&arguments)?;
    let config_path = std::fs::canonicalize(&arguments.config).with_context(|| {
        format!(
            "configuration inaccessible : {}",
            arguments.config.display()
        )
    })?;
    let base = config_path
        .parent()
        .context("le fichier de configuration n'a pas de dossier parent")?;
    let settings = config::read(&config_path)?;
    let requested_root = resolve(base, Path::new(&settings.paths.cache_root));
    let cache_root = std::fs::canonicalize(&requested_root).with_context(|| {
        format!(
            "racine du cache inaccessible : {}",
            requested_root.display()
        )
    })?;
    let output = resolve(base, &arguments.output);
    validate_output(&output, arguments.overwrite)?;

    let (cx, cy) = crate::coordinates::to_lambert93(
        settings.area.center[0],
        settings.area.center[1],
        &settings.area.crs,
    )?;
    let xmin = cx - settings.area.side_m / 2.;
    let ymin = cy - settings.area.side_m / 2.;
    let (mut surface, cache_path) = read_surface_with_cache(
        &cache_root,
        &settings,
        xmin,
        ymin,
        arguments.quality,
        arguments.rebuild_cache,
    )?;
    let scale_mm_per_m = arguments.width_mm / settings.area.side_m;
    let trees = if arguments.trees {
        prepare_trees(&settings, &surface, xmin, ymin)
    } else {
        Vec::new()
    };
    if arguments.trees {
        // Les volumes d'arbres remplacent la canopée de l'enveloppe LiDAR. Le
        // terrain visible devient donc le sol, sans doubler les mêmes arbres.
        surface.heights_m.clone_from(&surface.ground_m);
        update_surface_minimum(&mut surface);
    } else if arguments.buildings {
        surface
            .heights_m
            .clone_from(&surface.heights_without_buildings_m);
        update_surface_minimum(&mut surface);
    }
    if let Some(passes) = arguments.smooth {
        smooth_surface(&mut surface, passes, arguments.smooth_strength);
        println!(
            "[STL] Relief lissé : {passes} passe(s), intensité {:.2}",
            arguments.smooth_strength
        );
    }
    let features = if arguments.buildings || arguments.roads {
        prepare_vector_features(
            &cache_root,
            &settings,
            &surface,
            xmin,
            ymin,
            arguments.buildings,
            arguments.roads,
            arguments.road_min_width_mm / scale_mm_per_m,
        )?
    } else {
        VectorFeatures::default()
    };
    let roads = prepare_road_topologies(&features.roads)?;
    let vertical_scale = scale_mm_per_m * settings.terrain.vertical_exaggeration;
    let terrain_triangles = terrain_triangle_count(
        &surface,
        arguments.width_mm,
        arguments.base_mm,
        vertical_scale,
        terrain_style,
    )?;
    let building_triangles = building_triangle_count(&features.buildings)?;
    let road_triangles = road_triangle_count(&roads)?;
    let tree_triangles = tree_triangle_count(trees.len(), arguments.tree_shape)?;
    let triangle_count = terrain_triangles
        .checked_add(building_triangles)
        .and_then(|value| value.checked_add(road_triangles))
        .and_then(|value| value.checked_add(tree_triangles))
        .context("trop de triangles pour un STL binaire")?;

    println!("[STL] Cache : {}", cache_root.display());
    println!("[STL] Sortie : {}", output.display());
    println!(
        "[STL] {} fichier(s), {} points lus sur {} disponibles, {} points retenus{}",
        surface.files_used,
        surface.points_read,
        surface.points_available,
        surface.points_used,
        if surface.cached { " (cache)" } else { "" }
    );
    if surface.interpolated > 0 {
        println!(
            "[STL] {} sommet(s) sans mesure ont été interpolés",
            surface.interpolated
        );
    }
    println!(
        "[STL] {} x {} sommets, {} triangles, échelle {:.6} mm/m",
        surface.count, surface.count, triangle_count, scale_mm_per_m
    );
    if let TerrainStyle::Layered { layer_height_m } = terrain_style {
        println!(
            "[STL couches] {} strate(s), pas réel {:.3} m, hauteur imprimée {:.3} mm par strate",
            layered_level_count(&surface, layer_height_m)?,
            layer_height_m,
            layer_height_m * vertical_scale
        );
    }
    if arguments.buildings {
        println!(
            "[STL] {} bâtiment(s) BD TOPO, {} triangles de bâtiment",
            features.buildings.len(),
            building_triangles
        );
    }
    if arguments.roads {
        println!(
            "[STL] {} maillage(s) de routes BD TOPO, {} triangles de route, largeur minimale {:.2} mm",
            roads.len(),
            road_triangles,
            arguments.road_min_width_mm
        );
    }
    if arguments.trees {
        println!(
            "[STL] {} arbre(s) LiDAR, {} triangles d'arbre, troncs d'au moins {:.2} mm",
            trees.len(),
            tree_triangles,
            arguments.tree_trunk_diameter_mm
        );
    }

    write_atomic(
        &output,
        &surface,
        arguments.width_mm,
        arguments.base_mm,
        vertical_scale,
        scale_mm_per_m,
        terrain_style,
        &features.buildings,
        &roads,
        arguments.road_height_mm,
        &trees,
        arguments.tree_min_height_mm,
        arguments.tree_height_scale,
        arguments.tree_shape,
        arguments.tree_trunk_diameter_mm,
        arguments.tree_crown_diameter_mm,
        triangle_count,
        arguments.overwrite,
    )?;
    println!(
        "{} : {}",
        if matches!(terrain_style, TerrainStyle::Continuous) {
            "Bloc STL imprimable"
        } else {
            "Bloc STL en couches imprimable"
        },
        output.display()
    );
    Ok(cache_path)
}

fn validate_arguments(arguments: &StlArguments) -> Result<()> {
    positive("width-mm", arguments.width_mm)?;
    positive("base-mm", arguments.base_mm)?;
    positive("road-min-width-mm", arguments.road_min_width_mm)?;
    positive("road-height-mm", arguments.road_height_mm)?;
    positive("tree-min-height-mm", arguments.tree_min_height_mm)?;
    positive("tree-height-scale", arguments.tree_height_scale)?;
    positive("tree-trunk-diameter-mm", arguments.tree_trunk_diameter_mm)?;
    positive("tree-crown-diameter-mm", arguments.tree_crown_diameter_mm)?;
    if let Some(passes) = arguments.smooth
        && !(1..=50).contains(&passes)
    {
        bail!("--smooth attend un nombre de passes entre 1 et 50");
    }
    if !arguments.smooth_strength.is_finite()
        || arguments.smooth_strength <= 0.
        || arguments.smooth_strength > 1.
    {
        bail!("--smooth-strength doit être dans ]0, 1]");
    }
    Ok(())
}

fn positive(name: &str, value: f64) -> Result<()> {
    if !value.is_finite() || value <= 0. {
        bail!("--{name} doit être fini et strictement positif");
    }
    Ok(())
}

/// Lissage laplacien modéré de la grille d'altitudes. Les bords restent fixes
/// pour conserver exactement les quatre parois du bloc.
fn smooth_surface(surface: &mut Surface, passes: usize, strength: f64) {
    let count = surface.count;
    if count < 3 || passes == 0 {
        return;
    }
    let mut next = surface.heights_m.clone();
    for _ in 0..passes {
        for row in 1..count - 1 {
            for col in 1..count - 1 {
                let index = row * count + col;
                let mut sum = 0.;
                for y in row - 1..=row + 1 {
                    for x in col - 1..=col + 1 {
                        if x != col || y != row {
                            sum += surface.heights_m[y * count + x];
                        }
                    }
                }
                let average = sum / 8.;
                next[index] = surface.heights_m[index] * (1. - strength) + average * strength;
            }
        }
        std::mem::swap(&mut surface.heights_m, &mut next);
        next.copy_from_slice(&surface.heights_m);
    }
    update_surface_minimum(surface);
}

fn update_surface_minimum(surface: &mut Surface) {
    surface.minimum_m = surface
        .heights_m
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);
}

/// Transforme uniquement les retours des classes de végétation configurées
/// (3, 4 et 5 par défaut) en arbres. Une bosse de terrain ou un toit ne peut
/// ainsi plus être interprété comme un arbre.
fn prepare_trees(
    settings: &ExportConfig,
    surface: &Surface,
    xmin: f64,
    ymin: f64,
) -> Vec<PrintableTree> {
    let mut eligible = surface
        .canopy_tops
        .iter()
        .filter_map(|top| {
            let x_m = top[0] - xmin;
            let y_m = top[1] - ymin;
            let ground_m = grid_mesh_height(
                &surface.ground_m,
                surface.count,
                settings.area.side_m,
                x_m,
                y_m,
            );
            let height_m = top[2] - ground_m;
            (height_m.is_finite()
                && height_m >= settings.vegetation.min_height_m
                && height_m <= settings.vegetation.max_height_m)
                .then_some((x_m, y_m, height_m))
        })
        .collect::<Vec<_>>();
    eligible.sort_by(|left, right| {
        left.1
            .total_cmp(&right.1)
            .then_with(|| left.0.total_cmp(&right.0))
    });
    let stride = eligible
        .len()
        .div_ceil(settings.vegetation.max_trees.max(1))
        .max(1);
    eligible
        .into_iter()
        .step_by(stride)
        .take(settings.vegetation.max_trees)
        .map(|(x_m, y_m, height_m)| PrintableTree {
            x_m,
            y_m,
            height_m,
            crown_radius_m: height_m * settings.vegetation.crown_radius_ratio,
            trunk_radius_m: height_m * settings.vegetation.trunk_radius_ratio,
        })
        .collect()
}

/// Les empreintes vectorielles gardent les petits bâtiments que la grille
/// régulière du relief ne peut pas représenter à l'échelle d'une grande zone.
#[allow(clippy::too_many_arguments)]
fn prepare_vector_features(
    root: &Path,
    settings: &ExportConfig,
    surface: &Surface,
    xmin: f64,
    ymin: f64,
    buildings: bool,
    roads: bool,
    minimum_road_width_m: f64,
) -> Result<VectorFeatures> {
    let mut feature_settings = settings.clone();
    for (name, layer) in &mut feature_settings.layers {
        layer.enabled = (buildings && name == "bd_topo.batiment")
            || (roads && name == "bd_topo.troncon_de_route");
    }
    feature_settings.buildings.source = "bd_topo".into();
    feature_settings.buildings.texture_roofs = false;
    feature_settings.vectors.missing_layers = "error".into();
    if roads {
        let style = feature_settings
            .layers
            .get_mut("bd_topo.troncon_de_route")
            .context("configuration de la couche bd_topo.troncon_de_route absente")?;
        style.line_width_m = style.line_width_m.max(minimum_road_width_m);
    }
    let grid = TerrainGrid {
        xmin,
        ymin,
        side: settings.area.side_m,
        count: surface.count,
        heights: surface.ground_m.clone(),
        origin: [xmin, ymin, surface.minimum_m],
        exaggeration: settings.terrain.vertical_exaggeration,
        source: "LiDAR STL".into(),
    };
    let mut warnings = Vec::new();
    let layers =
        crate::export::vectors::prepare(root, &feature_settings, &grid, None, &mut warnings)?;
    for warning in warnings {
        eprintln!("[STL objets] {warning}");
    }
    let mut features = VectorFeatures::default();
    for layer in layers {
        match layer.name.as_str() {
            "bd_topo.batiment" => features.buildings = layer.objects,
            "bd_topo.troncon_de_route" => features.roads = layer.objects,
            _ => {}
        }
    }
    Ok(features)
}

fn resolve(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn validate_output(output: &Path, overwrite: bool) -> Result<()> {
    if output
        .extension()
        .and_then(|value| value.to_str())
        .is_none_or(|value| !value.eq_ignore_ascii_case("stl"))
    {
        bail!("--output doit se terminer par .stl");
    }
    if output.exists() && !overwrite {
        bail!(
            "le fichier existe déjà : {} ; ajouter --overwrite pour le remplacer",
            output.display()
        );
    }
    Ok(())
}

fn grid_count(settings: &ExportConfig) -> Result<usize> {
    let count = (settings.area.side_m / settings.terrain.grid_step_m).ceil() + 1.;
    if !count.is_finite() || count < 2. || count * count > settings.terrain.max_vertices as f64 {
        bail!(
            "surface STL trop dense : {:.0} x {:.0} = {:.0} sommets pour une limite de {}. Augmenter terrain.grid_step_m ou terrain.max_vertices",
            count,
            count,
            count * count,
            settings.terrain.max_vertices
        );
    }
    Ok(count as usize)
}

#[cfg(test)]
fn read_surface(
    root: &Path,
    settings: &ExportConfig,
    xmin: f64,
    ymin: f64,
    quality: StlQuality,
    rebuild_cache: bool,
) -> Result<Surface> {
    read_surface_with_cache(root, settings, xmin, ymin, quality, rebuild_cache)
        .map(|(surface, _)| surface)
}

fn read_surface_with_cache(
    root: &Path,
    settings: &ExportConfig,
    xmin: f64,
    ymin: f64,
    quality: StlQuality,
    rebuild_cache: bool,
) -> Result<(Surface, PathBuf)> {
    let count = grid_count(settings)?;
    let step = settings.area.side_m / (count - 1) as f64;
    let lidar_root = root.join("LIDAR_HD");
    let candidates = lidar_files(&lidar_root)?;
    if candidates.is_empty() {
        bail!(
            "aucun fichier LiDAR complet trouvé dans {}",
            lidar_root.display()
        );
    }

    let key = surface_cache_key(
        &lidar_root,
        &candidates,
        settings,
        xmin,
        ymin,
        count,
        quality,
    )?;
    let cache_path = surface_cache_path(root, &key)?;
    if !rebuild_cache {
        match load_surface_cache(&cache_path, &key) {
            Ok(Some(surface)) => {
                println!("[STL cache] surface réutilisée : {}", cache_path.display());
                return Ok((surface, cache_path));
            }
            Ok(None) => {}
            Err(error) => eprintln!(
                "[STL cache] cache ignoré car invalide ({}): {error}",
                cache_path.display()
            ),
        }
    }

    let mut builder = SurfaceBuilder {
        settings,
        xmin,
        ymin,
        count,
        step,
        heights: vec![None; count * count],
        heights_without_buildings: vec![None; count * count],
        ground: vec![None; count * count],
        canopy: BTreeMap::new(),
        points_read: 0,
        points_used: 0,
        points_available: 0,
        files_used: 0,
    };
    for path in &candidates {
        if quality != StlQuality::Full && try_read_copc(path, &mut builder, quality)? {
            continue;
        }
        read_full_lidar(path, &mut builder)?;
    }
    if builder.files_used == 0 {
        bail!("aucune dalle LiDAR complète ne recouvre la zone demandée");
    }
    let missing_before = complete_lidar_grid(
        &mut builder.heights,
        count,
        settings.terrain.lidar_fill_radius_cells,
        "surface LiDAR",
    )?;
    if missing_before > 0 {
        eprintln!(
            "[STL LiDAR] {missing_before} sommet(s) sans mesure interpolés ({:.3} %)",
            missing_before as f64 / builder.heights.len() as f64 * 100.
        );
    }
    if builder
        .heights_without_buildings
        .iter()
        .all(Option::is_none)
    {
        builder
            .heights_without_buildings
            .clone_from(&builder.heights);
    } else {
        complete_lidar_grid(
            &mut builder.heights_without_buildings,
            count,
            settings.terrain.lidar_fill_radius_cells,
            "surface LiDAR sans bâtiments",
        )?;
    }
    if builder.ground.iter().all(Option::is_none) {
        builder
            .ground
            .clone_from(&builder.heights_without_buildings);
    } else {
        complete_lidar_grid(
            &mut builder.ground,
            count,
            settings.terrain.lidar_fill_radius_cells,
            "sol LiDAR",
        )?;
    }
    let heights_m: Vec<f64> = builder.heights.into_iter().map(Option::unwrap).collect();
    let heights_without_buildings_m = builder
        .heights_without_buildings
        .into_iter()
        .map(Option::unwrap)
        .collect();
    let ground_m = builder.ground.into_iter().map(Option::unwrap).collect();
    let canopy_tops = builder.canopy.into_values().collect();
    let minimum_m = heights_m.iter().copied().fold(f64::INFINITY, f64::min);
    let surface = Surface {
        count,
        heights_m,
        heights_without_buildings_m,
        ground_m,
        canopy_tops,
        minimum_m,
        points_read: builder.points_read,
        points_used: builder.points_used,
        points_available: builder.points_available,
        files_used: builder.files_used,
        interpolated: missing_before,
        cached: false,
    };
    if let Err(error) = save_surface_cache(&cache_path, &key, &surface) {
        eprintln!(
            "[STL cache] impossible d'enregistrer {} : {error}",
            cache_path.display()
        );
    } else {
        println!("[STL cache] surface enregistrée : {}", cache_path.display());
    }
    Ok((surface, cache_path))
}

impl SurfaceBuilder<'_> {
    fn overlaps(&self, bounds: las::Bounds) -> bool {
        bounds.max.x >= self.xmin
            && bounds.min.x <= self.xmin + self.settings.area.side_m
            && bounds.max.y >= self.ymin
            && bounds.min.y <= self.ymin + self.settings.area.side_m
    }

    fn add(&mut self, point: Point) {
        let class: u8 = point.classification.into();
        self.add_values(point.x, point.y, point.z, class, point.is_withheld);
    }

    fn add_values(&mut self, x: f64, y: f64, z: f64, class: u8, is_withheld: bool) {
        self.points_read += 1;
        if self.points_read.is_multiple_of(1_000_000) {
            println!(
                "[STL LiDAR] {} millions de points lus, {} points retenus",
                self.points_read / 1_000_000,
                self.points_used
            );
        }
        if !x.is_finite()
            || !y.is_finite()
            || !z.is_finite()
            || is_withheld
            || z < self.settings.terrain.valid_altitude_min_m
            || z > self.settings.terrain.valid_altitude_max_m
            || x < self.xmin
            || x > self.xmin + self.settings.area.side_m
            || y < self.ymin
            || y > self.ymin + self.settings.area.side_m
        {
            return;
        }
        if self.settings.vegetation.classes.contains(&class) {
            let key = (
                ((x - self.xmin) / self.settings.vegetation.cell_size_m).floor() as i64,
                ((y - self.ymin) / self.settings.vegetation.cell_size_m).floor() as i64,
            );
            let top = self.canopy.entry(key).or_insert([x, y, z]);
            if z > top[2] {
                *top = [x, y, z];
            }
        }
        if !surface_class(class, self.settings.terrain.lidar_ground_class) {
            return;
        }
        let col = (((x - self.xmin) / self.step).round() as usize).min(self.count - 1);
        let row = (((y - self.ymin) / self.step).round() as usize).min(self.count - 1);
        let value = &mut self.heights[row * self.count + col];
        *value = Some(value.map_or(z, |current| current.max(z)));
        if class != 6 {
            let value = &mut self.heights_without_buildings[row * self.count + col];
            *value = Some(value.map_or(z, |current| current.max(z)));
        }
        if class == self.settings.terrain.lidar_ground_class {
            let value = &mut self.ground[row * self.count + col];
            *value = Some(value.map_or(z, |current| current.min(z)));
        }
        self.points_used += 1;
    }
}

fn read_full_lidar(path: &Path, builder: &mut SurfaceBuilder<'_>) -> Result<()> {
    let mut reader =
        Reader::from_path(path).with_context(|| format!("lecture LAZ {}", path.display()))?;
    if !builder.overlaps(reader.header().bounds()) {
        return Ok(());
    }
    println!("[STL LiDAR complet] {}", path.display());
    builder.files_used += 1;
    builder.points_available += reader.header().number_of_points();
    for point in reader.points() {
        builder.add(point?);
    }
    Ok(())
}

/// Lit seulement les niveaux COPC nécessaires à la qualité choisie. `false`
/// demande au code appelant de revenir au lecteur LAZ complet (fichier non COPC).
fn try_read_copc(
    path: &Path,
    builder: &mut SurfaceBuilder<'_>,
    quality: StlQuality,
) -> Result<bool> {
    let mut reader = match CopcReader::from_path(path) {
        Ok(reader) => reader,
        Err(error) => {
            if has_copc_info_vlr(path) {
                return Err(error).with_context(|| {
                    format!("ouverture de la hiérarchie COPC : {}", path.display())
                });
            }
            return Ok(false);
        }
    };
    if reader.header().max_x < builder.xmin
        || reader.header().min_x > builder.xmin + builder.settings.area.side_m
        || reader.header().max_y < builder.ymin
        || reader.header().min_y > builder.ymin + builder.settings.area.side_m
    {
        return Ok(true);
    }
    let target_spacing = quality
        .target_spacing(builder.step)
        .context("qualité COPC sans espacement cible")?;
    println!(
        "[STL COPC {:?}] espacement cible {:.2} m : {}",
        quality,
        target_spacing,
        path.display()
    );
    builder.files_used += 1;
    builder.points_available += reader.header().number_of_points;
    let bounds = CopcBounds::new(
        (
            builder.xmin,
            builder.ymin,
            builder.settings.terrain.valid_altitude_min_m,
        ),
        (
            builder.xmin + builder.settings.area.side_m,
            builder.ymin + builder.settings.area.side_m,
            builder.settings.terrain.valid_altitude_max_m,
        ),
    );
    let points = reader
        .points(
            LodSelection::Resolution(target_spacing),
            BoundsSelection::Within(bounds),
        )
        .with_context(|| format!("requête spatiale COPC : {}", path.display()))?;
    for point in points {
        let point = point.with_context(|| format!("lecture COPC : {}", path.display()))?;
        let class: u8 = point.classification.into();
        builder.add_values(point.x, point.y, point.z, class, point.is_withheld);
    }
    Ok(true)
}

fn has_copc_info_vlr(path: &Path) -> bool {
    Reader::from_path(path).is_ok_and(|reader| {
        reader.header().vlrs().iter().any(|vlr| {
            vlr.record_id == 1
                && vlr
                    .user_id
                    .trim_end_matches('\0')
                    .eq_ignore_ascii_case("copc")
        })
    })
}

/// Classes décrivant une surface physique. Les fils et les classes de bruit sont exclus
/// pour éviter des aiguilles fragiles dans le modèle imprimé.
fn surface_class(class: u8, ground_class: u8) -> bool {
    class == ground_class || matches!(class, 0 | 1 | 3 | 4 | 5 | 6 | 9 | 10 | 11 | 15 | 17)
}

fn complete_lidar_grid(
    heights: &mut [Option<f64>],
    count: usize,
    radius: usize,
    label: &str,
) -> Result<usize> {
    let measured = heights.iter().filter(|height| height.is_some()).count();
    if measured == 0 {
        bail!("aucune mesure exploitable pour {label}");
    }
    let missing_before = heights.len() - measured;
    fill_holes(heights, count, radius);
    fill_remaining_holes(heights, count);
    let missing_after = heights.iter().filter(|height| height.is_none()).count();
    if missing_after > 0 {
        bail!("impossible d'interpoler {missing_after} sommet(s) de {label}");
    }
    Ok(missing_before)
}

fn fill_holes(heights: &mut [Option<f64>], count: usize, radius: usize) {
    let original = heights.to_vec();
    for row in 0..count {
        for col in 0..count {
            let index = row * count + col;
            if original[index].is_some() {
                continue;
            }
            for distance in 1..=radius {
                let mut sum = 0.;
                let mut samples = 0usize;
                for y in row.saturating_sub(distance)..=(row + distance).min(count - 1) {
                    for x in col.saturating_sub(distance)..=(col + distance).min(count - 1) {
                        if let Some(height) = original[y * count + x] {
                            sum += height;
                            samples += 1;
                        }
                    }
                }
                if samples > 0 {
                    heights[index] = Some(sum / samples as f64);
                    break;
                }
            }
        }
    }
}

/// Propage une interpolation locale jusqu'aux derniers trous. Une file de front
/// évite les balayages répétés de toute la grille, même pour une lacune étendue.
fn fill_remaining_holes(heights: &mut [Option<f64>], count: usize) {
    let mut queued = vec![false; heights.len()];
    let mut queue = VecDeque::new();
    for row in 0..count {
        for col in 0..count {
            let index = row * count + col;
            if heights[index].is_none() && neighbor_average(heights, count, row, col).is_some() {
                queue.push_back((row, col));
                queued[index] = true;
            }
        }
    }
    while let Some((row, col)) = queue.pop_front() {
        let index = row * count + col;
        if heights[index].is_some() {
            continue;
        }
        let Some(value) = neighbor_average(heights, count, row, col) else {
            continue;
        };
        heights[index] = Some(value);
        for y in row.saturating_sub(1)..=(row + 1).min(count - 1) {
            for x in col.saturating_sub(1)..=(col + 1).min(count - 1) {
                let neighbor = y * count + x;
                if heights[neighbor].is_none() && !queued[neighbor] {
                    queued[neighbor] = true;
                    queue.push_back((y, x));
                }
            }
        }
    }
}

fn neighbor_average(heights: &[Option<f64>], count: usize, row: usize, col: usize) -> Option<f64> {
    let mut sum = 0.;
    let mut samples = 0usize;
    for y in row.saturating_sub(1)..=(row + 1).min(count - 1) {
        for x in col.saturating_sub(1)..=(col + 1).min(count - 1) {
            if (x != col || y != row)
                && let Some(value) = heights[y * count + x]
            {
                sum += value;
                samples += 1;
            }
        }
    }
    (samples > 0).then_some(sum / samples as f64)
}

fn lidar_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut folders = vec![root.to_path_buf()];
    let mut candidates = Vec::new();
    while let Some(folder) = folders.pop() {
        if !folder.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(&folder)
            .with_context(|| format!("lecture du dossier LiDAR : {}", folder.display()))?
        {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                folders.push(entry.path());
            } else if kind.is_file()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".laz.download.json")
            {
                let receipt: Value = serde_json::from_slice(&std::fs::read(entry.path())?)
                    .with_context(|| format!("reçu LiDAR invalide : {}", entry.path().display()))?;
                let path = entry.path().with_file_name(
                    entry
                        .file_name()
                        .to_string_lossy()
                        .trim_end_matches(".download.json"),
                );
                if std::fs::metadata(&path).is_ok_and(|metadata| {
                    Some(metadata.len()) == receipt.get("size").and_then(Value::as_u64)
                }) {
                    candidates.push(path);
                }
            }
        }
    }

    // Une même dalle peut exister dans plusieurs éditions. La plus récente gagne.
    candidates.sort_by(|a, b| {
        edition(b)
            .cmp(&edition(a))
            .then_with(|| b.as_os_str().cmp(a.as_os_str()))
    });
    let mut names = BTreeSet::new();
    candidates.retain(|path| {
        path.file_name()
            .is_some_and(|name| names.insert(name.to_os_string()))
    });
    Ok(candidates)
}

fn edition(path: &Path) -> String {
    path.parent()
        .and_then(Path::file_name)
        .map(|name| {
            name.to_string_lossy()
                .rsplit('_')
                .next()
                .unwrap_or("")
                .to_owned()
        })
        .unwrap_or_default()
}

fn surface_cache_key(
    lidar_root: &Path,
    candidates: &[PathBuf],
    settings: &ExportConfig,
    xmin: f64,
    ymin: f64,
    count: usize,
    quality: StlQuality,
) -> Result<SurfaceCacheKey> {
    let files = candidates
        .iter()
        .map(|path| {
            let metadata = std::fs::metadata(path)
                .with_context(|| format!("métadonnées LiDAR : {}", path.display()))?;
            let modified_ns = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .and_then(|duration| u64::try_from(duration.as_nanos()).ok())
                .unwrap_or(0);
            Ok(LidarFileStamp {
                path: path
                    .strip_prefix(lidar_root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .into_owned(),
                size: metadata.len(),
                modified_ns,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(SurfaceCacheKey {
        schema_version: 4,
        xmin,
        ymin,
        side_m: settings.area.side_m,
        count,
        grid_step_m: settings.terrain.grid_step_m,
        valid_altitude_min_m: settings.terrain.valid_altitude_min_m,
        valid_altitude_max_m: settings.terrain.valid_altitude_max_m,
        lidar_ground_class: settings.terrain.lidar_ground_class,
        lidar_fill_radius_cells: settings.terrain.lidar_fill_radius_cells,
        vegetation_classes: settings.vegetation.classes.clone(),
        vegetation_cell_size_m: settings.vegetation.cell_size_m,
        quality,
        files,
    })
}

fn surface_cache_path(root: &Path, key: &SurfaceCacheKey) -> Result<PathBuf> {
    let encoded = serde_json::to_vec(key)?;
    let hash = encoded.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    Ok(root
        .join("STL_CACHE")
        .join(format!("surface-{hash:016x}.gstl")))
}

fn load_surface_cache(path: &Path, key: &SurfaceCacheKey) -> Result<Option<Surface>> {
    if !path.is_file() {
        return Ok(None);
    }
    let mut reader = BufReader::new(
        File::open(path).with_context(|| format!("lecture du cache : {}", path.display()))?,
    );
    let mut magic = [0u8; 8];
    reader.read_exact(&mut magic)?;
    if &magic != b"GSTLC04\0" {
        bail!("signature de cache inconnue");
    }
    let header_size = read_u32(&mut reader)? as usize;
    if header_size > 16 * 1024 * 1024 {
        bail!("en-tête de cache excessif");
    }
    let mut encoded = vec![0u8; header_size];
    reader.read_exact(&mut encoded)?;
    let header: SurfaceCacheHeader = serde_json::from_slice(&encoded)?;
    if header.key != *key {
        return Ok(None);
    }
    let length = read_u64(&mut reader)? as usize;
    let expected = key
        .count
        .checked_mul(key.count)
        .context("dimensions de cache excessives")?;
    if length != expected {
        bail!("nombre d'altitudes incohérent dans le cache");
    }
    let heights_m = read_height_grid(&mut reader, length)?;
    let heights_without_buildings_m = read_height_grid(&mut reader, length)?;
    let ground_m = read_height_grid(&mut reader, length)?;
    let canopy_length = usize::try_from(read_u64(&mut reader)?)
        .context("nombre de points de végétation excessif dans le cache")?;
    if canopy_length > 10_000_000 {
        bail!("nombre de points de végétation excessif dans le cache");
    }
    let mut canopy_tops = Vec::with_capacity(canopy_length);
    for _ in 0..canopy_length {
        let point = [
            read_f64(&mut reader)?,
            read_f64(&mut reader)?,
            read_f64(&mut reader)?,
        ];
        if point.iter().any(|value| !value.is_finite()) {
            bail!("point de végétation invalide dans le cache");
        }
        canopy_tops.push(point);
    }
    let minimum_m = heights_m.iter().copied().fold(f64::INFINITY, f64::min);
    Ok(Some(Surface {
        count: key.count,
        heights_m,
        heights_without_buildings_m,
        ground_m,
        canopy_tops,
        minimum_m,
        points_read: header.report.points_read,
        points_used: header.report.points_used,
        points_available: header.report.points_available,
        files_used: header.report.files_used,
        interpolated: header.report.interpolated,
        cached: true,
    }))
}

fn save_surface_cache(path: &Path, key: &SurfaceCacheKey, surface: &Surface) -> Result<()> {
    let parent = path.parent().context("cache de surface sans dossier")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("création du cache : {}", parent.display()))?;
    let header = SurfaceCacheHeader {
        key: key.clone(),
        report: SurfaceCacheReport {
            points_read: surface.points_read,
            points_used: surface.points_used,
            points_available: surface.points_available,
            files_used: surface.files_used,
            interpolated: surface.interpolated,
        },
    };
    let encoded = serde_json::to_vec(&header)?;
    let partial = path.with_extension("gstl.part");
    let file = File::create(&partial)
        .with_context(|| format!("création du cache : {}", partial.display()))?;
    let mut writer = BufWriter::new(file);
    writer.write_all(b"GSTLC04\0")?;
    writer.write_all(&u32::try_from(encoded.len())?.to_le_bytes())?;
    writer.write_all(&encoded)?;
    writer.write_all(&u64::try_from(surface.heights_m.len())?.to_le_bytes())?;
    for heights in [
        &surface.heights_m,
        &surface.heights_without_buildings_m,
        &surface.ground_m,
    ] {
        for height in heights {
            writer.write_all(&height.to_le_bytes())?;
        }
    }
    writer.write_all(&u64::try_from(surface.canopy_tops.len())?.to_le_bytes())?;
    for point in &surface.canopy_tops {
        for value in point {
            writer.write_all(&value.to_le_bytes())?;
        }
    }
    writer.flush()?;
    writer.get_ref().sync_all()?;
    drop(writer);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    std::fs::rename(&partial, path)
        .with_context(|| format!("publication du cache : {}", path.display()))?;
    Ok(())
}

fn read_u32<R: Read>(reader: &mut R) -> Result<u32> {
    let mut bytes = [0u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_height_grid<R: Read>(reader: &mut R, length: usize) -> Result<Vec<f64>> {
    let mut heights = Vec::with_capacity(length);
    for _ in 0..length {
        let mut bytes = [0u8; 8];
        reader.read_exact(&mut bytes)?;
        let value = f64::from_le_bytes(bytes);
        if !value.is_finite() {
            bail!("altitude invalide dans le cache");
        }
        heights.push(value);
    }
    Ok(heights)
}

fn read_u64<R: Read>(reader: &mut R) -> Result<u64> {
    let mut bytes = [0u8; 8];
    reader.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_f64<R: Read>(reader: &mut R) -> Result<f64> {
    let mut bytes = [0u8; 8];
    reader.read_exact(&mut bytes)?;
    Ok(f64::from_le_bytes(bytes))
}

fn triangle_count(count: usize) -> Result<u32> {
    let intervals = count
        .checked_sub(1)
        .context("la grille STL doit contenir au moins deux sommets")?;
    // Deux triangles par cellule au-dessus, huit par intervalle sur les parois,
    // puis un éventail de quatre triangles par intervalle pour le fond.
    let total = 2usize
        .checked_mul(intervals)
        .and_then(|value| value.checked_mul(intervals))
        .and_then(|value| value.checked_add(12usize.checked_mul(intervals)?))
        .context("trop de triangles pour un STL binaire")?;
    u32::try_from(total).context("trop de triangles pour un STL binaire")
}

#[allow(clippy::type_complexity)]
fn building_topology(object: &ObjectPlan) -> Result<(Vec<[usize; 3]>, Vec<(usize, usize)>)> {
    let mut roofs = Vec::new();
    for &face_index in &object.roof_faces {
        let face = object
            .faces
            .get(face_index)
            .context("indice de face de toit BD TOPO invalide")?;
        if face.len() < 3 {
            continue;
        }
        for index in 1..face.len() - 1 {
            let mut triangle = [face[0], face[index], face[index + 1]];
            let [a, b, c] = triangle.map(|vertex| {
                object
                    .vertices
                    .get(vertex)
                    .copied()
                    .context("indice de sommet de bâtiment invalide")
            });
            let (a, b, c) = (a?, b?, c?);
            let signed = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            if signed.abs() <= f64::EPSILON {
                continue;
            }
            if signed < 0. {
                triangle.swap(1, 2);
            }
            roofs.push(triangle);
        }
    }

    let mut edges = std::collections::BTreeMap::<(usize, usize), (usize, usize, usize)>::new();
    for triangle in &roofs {
        for (first, second) in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            let key = if first < second {
                (first, second)
            } else {
                (second, first)
            };
            let entry = edges.entry(key).or_insert((first, second, 0));
            entry.2 += 1;
            if entry.2 > 2 {
                bail!("toit BD TOPO non manifold : {}", object.name);
            }
        }
    }
    let boundary = edges
        .into_values()
        .filter_map(|(first, second, count)| (count == 1).then_some((first, second)))
        .collect();
    Ok((roofs, boundary))
}

fn building_triangle_count(buildings: &[ObjectPlan]) -> Result<u32> {
    let mut total = 0usize;
    for building in buildings {
        let (roofs, boundary) = building_topology(building)?;
        total = total
            .checked_add(2usize.checked_mul(roofs.len()).context("trop de toits")?)
            .and_then(|value| value.checked_add(2usize.checked_mul(boundary.len())?))
            .context("trop de triangles de bâtiments")?;
    }
    u32::try_from(total).context("trop de triangles de bâtiments pour un STL binaire")
}

fn prepare_road_topologies(roads: &[ObjectPlan]) -> Result<Vec<RoadTopology>> {
    let mut result = Vec::new();
    for object in roads {
        let road = road_topology(object)?;
        if !road.triangles.is_empty() {
            result.push(road);
        }
    }
    Ok(result)
}

fn road_topology(object: &ObjectPlan) -> Result<RoadTopology> {
    let mut vertices = Vec::<[f64; 2]>::new();
    let mut canonical = BTreeMap::<(i64, i64), usize>::new();
    let mut triangles = Vec::new();
    for face in &object.faces {
        if face.len() < 3 {
            continue;
        }
        for offset in 1..face.len() - 1 {
            let source = [face[0], face[offset], face[offset + 1]];
            let mut triangle = [0usize; 3];
            for (destination, source) in triangle.iter_mut().zip(source) {
                let point = object
                    .vertices
                    .get(source)
                    .context("indice de sommet de route invalide")?;
                if !point[0].is_finite() || !point[1].is_finite() {
                    bail!("coordonnée de route invalide");
                }
                let key = (
                    (point[0] * 1_000_000.).round() as i64,
                    (point[1] * 1_000_000.).round() as i64,
                );
                *destination = *canonical.entry(key).or_insert_with(|| {
                    let index = vertices.len();
                    vertices.push([point[0], point[1]]);
                    index
                });
            }
            if triangle[0] == triangle[1]
                || triangle[1] == triangle[2]
                || triangle[2] == triangle[0]
            {
                continue;
            }
            let [a, b, c] = triangle.map(|index| vertices[index]);
            let signed = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            if signed.abs() <= f64::EPSILON {
                continue;
            }
            if signed < 0. {
                triangle.swap(1, 2);
            }
            triangles.push(triangle);
        }
    }
    let mut edges = BTreeMap::<(usize, usize), (usize, usize, usize)>::new();
    for triangle in &triangles {
        for (first, second) in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            let key = if first < second {
                (first, second)
            } else {
                (second, first)
            };
            let entry = edges.entry(key).or_insert((first, second, 0));
            entry.2 += 1;
            if entry.2 > 2 {
                bail!("maillage de route non manifold : {}", object.name);
            }
        }
    }
    let boundary = edges
        .into_values()
        .filter_map(|(first, second, count)| (count == 1).then_some((first, second)))
        .collect();
    Ok(RoadTopology {
        vertices,
        triangles,
        boundary,
    })
}

fn road_triangle_count(roads: &[RoadTopology]) -> Result<u32> {
    let total = roads.iter().try_fold(0usize, |total, road| {
        total
            .checked_add(2usize.checked_mul(road.triangles.len())?)
            .and_then(|value| value.checked_add(2usize.checked_mul(road.boundary.len())?))
    });
    u32::try_from(total.context("trop de triangles de routes")?)
        .context("trop de triangles de routes pour un STL binaire")
}

const TREE_SEGMENTS: usize = 8;

fn tree_triangle_count(tree_count: usize, shape: TreeShape) -> Result<u32> {
    // Tronc fermé : 4N - 4 triangles. Le feuillu utilise trois anneaux
    // (6N triangles) ; le conifère trois cônes fermés (3 * (2N - 2)).
    let trunk = 4usize
        .checked_mul(TREE_SEGMENTS)
        .and_then(|value| value.checked_sub(4))
        .context("nombre de triangles de tronc invalide")?;
    let foliage = match shape {
        TreeShape::Rounded => 6usize.checked_mul(TREE_SEGMENTS),
        TreeShape::Conifer => 3usize.checked_mul(2 * TREE_SEGMENTS - 2),
    }
    .context("nombre de triangles de feuillage invalide")?;
    let per_tree = trunk
        .checked_add(foliage)
        .context("nombre de triangles d'arbre invalide")?;
    let total = tree_count
        .checked_mul(per_tree)
        .context("trop de triangles d'arbres")?;
    u32::try_from(total).context("trop de triangles d'arbres pour un STL binaire")
}

fn surface_mesh_height(surface: &Surface, side_m: f64, x_m: f64, y_m: f64) -> f64 {
    grid_mesh_height(&surface.heights_m, surface.count, side_m, x_m, y_m)
}

fn grid_mesh_height(heights: &[f64], count: usize, side_m: f64, x_m: f64, y_m: f64) -> f64 {
    let step = side_m / (count - 1) as f64;
    let u = (x_m / step).clamp(0., (count - 1) as f64);
    let v = (y_m / step).clamp(0., (count - 1) as f64);
    let col = (u.floor() as usize).min(count - 2);
    let row = (v.floor() as usize).min(count - 2);
    let a = u - col as f64;
    let b = v - row as f64;
    let sw = heights[row * count + col];
    let se = heights[row * count + col + 1];
    let nw = heights[(row + 1) * count + col];
    let ne = heights[(row + 1) * count + col + 1];
    if b <= a {
        sw * (1. - a) + se * (a - b) + ne * b
    } else {
        sw * (1. - b) + ne * a + nw * (b - a)
    }
}

fn point(
    surface: &Surface,
    row: usize,
    col: usize,
    width_mm: f64,
    base_mm: f64,
    vertical_scale: f64,
) -> [f32; 3] {
    let intervals = (surface.count - 1) as f64;
    [
        (col as f64 / intervals * width_mm) as f32,
        (row as f64 / intervals * width_mm) as f32,
        (base_mm
            + (surface.heights_m[row * surface.count + col] - surface.minimum_m) * vertical_scale)
            as f32,
    ]
}

fn terrain_triangle_count(
    surface: &Surface,
    width_mm: f64,
    base_mm: f64,
    vertical_scale: f64,
    terrain_style: TerrainStyle,
) -> Result<u32> {
    match terrain_style {
        TerrainStyle::Continuous => triangle_count(surface.count),
        TerrainStyle::Layered { layer_height_m } => {
            let mut count = 0u32;
            emit_layered_terrain(
                surface,
                width_mm,
                base_mm,
                vertical_scale,
                layer_height_m,
                |_| {
                    count = count
                        .checked_add(1)
                        .context("trop de triangles de terrain en couches")?;
                    Ok(())
                },
            )?;
            Ok(count)
        }
    }
}

fn layered_level_count(surface: &Surface, layer_height_m: f64) -> Result<usize> {
    let maximum = surface
        .heights_m
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let relief = maximum - surface.minimum_m;
    if !relief.is_finite() || relief < 0. {
        bail!("plage d'altitudes LiDAR invalide");
    }
    let count = (relief / layer_height_m).floor() + 1.;
    if !count.is_finite() || count > 100_000. {
        bail!(
            "le relief produirait {:.0} strates ; augmenter --layer-height-m",
            count
        );
    }
    Ok(count as usize)
}

fn printable_terrain_height(
    surface: &Surface,
    side_m: f64,
    x_m: f64,
    y_m: f64,
    terrain_style: TerrainStyle,
) -> f64 {
    let height = surface_mesh_height(surface, side_m, x_m, y_m);
    match terrain_style {
        TerrainStyle::Continuous => height,
        TerrainStyle::Layered { layer_height_m } => {
            surface.minimum_m
                + ((height - surface.minimum_m) / layer_height_m)
                    .max(0.)
                    .floor()
                    * layer_height_m
        }
    }
}

fn write_continuous_terrain<W: Write>(
    writer: &mut W,
    surface: &Surface,
    width_mm: f64,
    base_mm: f64,
    vertical_scale: f64,
) -> Result<()> {
    let n = surface.count;
    for row in 0..n - 1 {
        for col in 0..n - 1 {
            let a = point(surface, row, col, width_mm, base_mm, vertical_scale);
            let b = point(surface, row, col + 1, width_mm, base_mm, vertical_scale);
            let c = point(surface, row + 1, col, width_mm, base_mm, vertical_scale);
            let d = point(surface, row + 1, col + 1, width_mm, base_mm, vertical_scale);
            write_triangle(writer, [a, b, c])?;
            write_triangle(writer, [b, d, c])?;
        }
    }

    // Sud et nord.
    for col in 0..n - 1 {
        let south_0 = point(surface, 0, col, width_mm, base_mm, vertical_scale);
        let south_1 = point(surface, 0, col + 1, width_mm, base_mm, vertical_scale);
        write_wall(writer, south_0, south_1, false)?;

        let north_0 = point(surface, n - 1, col, width_mm, base_mm, vertical_scale);
        let north_1 = point(surface, n - 1, col + 1, width_mm, base_mm, vertical_scale);
        write_wall(writer, north_0, north_1, true)?;
    }
    // Ouest et est.
    for row in 0..n - 1 {
        let west_0 = point(surface, row, 0, width_mm, base_mm, vertical_scale);
        let west_1 = point(surface, row + 1, 0, width_mm, base_mm, vertical_scale);
        write_wall(writer, west_0, west_1, true)?;

        let east_0 = point(surface, row, n - 1, width_mm, base_mm, vertical_scale);
        let east_1 = point(surface, row + 1, n - 1, width_mm, base_mm, vertical_scale);
        write_wall(writer, east_0, east_1, false)?;
    }

    // Le fond en éventail réutilise chaque segment inférieur des parois.
    let boundary = bottom_boundary(n, width_mm);
    let center = [width_mm as f32 / 2., width_mm as f32 / 2., 0.];
    for index in 0..boundary.len() {
        let current = boundary[index];
        let next = boundary[(index + 1) % boundary.len()];
        write_triangle(writer, [center, next, current])?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct LayerVertex {
    x_m: f64,
    y_m: f64,
    height_m: f64,
}

#[derive(Clone, Copy, Debug)]
struct BoundarySegment {
    start: [f32; 2],
    end: [f32; 2],
    top_mm: f32,
}

fn layer_index(height_m: f64, minimum_m: f64, layer_height_m: f64) -> usize {
    (((height_m - minimum_m) / layer_height_m).max(0.).floor()) as usize
}

fn layer_model_point(vertex: LayerVertex, z_mm: f64, side_m: f64, width_mm: f64) -> [f32; 3] {
    [
        (vertex.x_m / side_m * width_mm) as f32,
        (vertex.y_m / side_m * width_mm) as f32,
        z_mm as f32,
    ]
}

fn interpolate_layer_vertex(first: LayerVertex, second: LayerVertex, height_m: f64) -> LayerVertex {
    let ratio = (height_m - first.height_m) / (second.height_m - first.height_m);
    LayerVertex {
        x_m: first.x_m + (second.x_m - first.x_m) * ratio,
        y_m: first.y_m + (second.y_m - first.y_m) * ratio,
        height_m,
    }
}

fn clip_layer_polygon(
    polygon: &[LayerVertex],
    height_m: f64,
    keep_above: bool,
) -> Vec<LayerVertex> {
    let mut output = Vec::new();
    if polygon.is_empty() {
        return output;
    }
    for index in 0..polygon.len() {
        let first = polygon[index];
        let second = polygon[(index + 1) % polygon.len()];
        let first_inside = if keep_above {
            first.height_m >= height_m
        } else {
            first.height_m <= height_m
        };
        let second_inside = if keep_above {
            second.height_m >= height_m
        } else {
            second.height_m <= height_m
        };
        if first_inside {
            output.push(first);
        }
        if first_inside != second_inside {
            output.push(interpolate_layer_vertex(first, second, height_m));
        }
    }
    clean_layer_polygon(output)
}

fn clean_layer_polygon(mut polygon: Vec<LayerVertex>) -> Vec<LayerVertex> {
    const EPSILON: f64 = 1e-10;
    let same = |a: LayerVertex, b: LayerVertex| {
        (a.x_m - b.x_m).abs() <= EPSILON && (a.y_m - b.y_m).abs() <= EPSILON
    };
    polygon.dedup_by(|left, right| same(*left, *right));
    if polygon.len() > 1 && same(polygon[0], *polygon.last().unwrap()) {
        polygon.pop();
    }
    let mut changed = true;
    while changed && polygon.len() >= 3 {
        changed = false;
        for index in 0..polygon.len() {
            let previous = polygon[(index + polygon.len() - 1) % polygon.len()];
            let current = polygon[index];
            let next = polygon[(index + 1) % polygon.len()];
            let cross = (current.x_m - previous.x_m) * (next.y_m - current.y_m)
                - (current.y_m - previous.y_m) * (next.x_m - current.x_m);
            if cross.abs() <= EPSILON {
                polygon.remove(index);
                changed = true;
                break;
            }
        }
    }
    polygon
}

fn contour_segment(triangle: [LayerVertex; 3], height_m: f64) -> Option<[LayerVertex; 2]> {
    let minimum = triangle
        .iter()
        .map(|point| point.height_m)
        .fold(f64::INFINITY, f64::min);
    let maximum = triangle
        .iter()
        .map(|point| point.height_m)
        .fold(f64::NEG_INFINITY, f64::max);
    // La règle stricte du côté bas attribue une arête exactement sur le seuil
    // à un seul des deux triangles voisins.
    if minimum >= height_m || maximum < height_m {
        return None;
    }
    let mut intersections = Vec::with_capacity(2);
    for (first, second) in [(0, 1), (1, 2), (2, 0)] {
        let a = triangle[first];
        let b = triangle[second];
        if (a.height_m < height_m && b.height_m >= height_m)
            || (b.height_m < height_m && a.height_m >= height_m)
        {
            let point = interpolate_layer_vertex(a, b, height_m);
            if intersections.iter().all(|existing: &LayerVertex| {
                (existing.x_m - point.x_m).abs() > 1e-10 || (existing.y_m - point.y_m).abs() > 1e-10
            }) {
                intersections.push(point);
            }
        }
    }
    (intersections.len() == 2).then(|| [intersections[0], intersections[1]])
}

#[allow(clippy::too_many_arguments)]
fn emit_layered_source_triangle<F>(
    triangle: [LayerVertex; 3],
    surface: &Surface,
    side_m: f64,
    width_mm: f64,
    base_mm: f64,
    vertical_scale: f64,
    layer_height_m: f64,
    emit: &mut F,
) -> Result<()>
where
    F: FnMut([[f32; 3]; 3]) -> Result<()>,
{
    let minimum = triangle
        .iter()
        .map(|point| point.height_m)
        .fold(f64::INFINITY, f64::min);
    let maximum = triangle
        .iter()
        .map(|point| point.height_m)
        .fold(f64::NEG_INFINITY, f64::max);
    let first_band = layer_index(minimum, surface.minimum_m, layer_height_m);
    let last_band = layer_index(maximum, surface.minimum_m, layer_height_m);

    for band in first_band..=last_band {
        let lower = surface.minimum_m + band as f64 * layer_height_m;
        let upper = lower + layer_height_m;
        let clipped = clip_layer_polygon(&clip_layer_polygon(&triangle, lower, true), upper, false);
        if clipped.len() < 3 {
            continue;
        }
        let z_mm = base_mm + band as f64 * layer_height_m * vertical_scale;
        let origin = layer_model_point(clipped[0], z_mm, side_m, width_mm);
        for index in 1..clipped.len() - 1 {
            let second = layer_model_point(clipped[index], z_mm, side_m, width_mm);
            let third = layer_model_point(clipped[index + 1], z_mm, side_m, width_mm);
            let area = (second[0] - origin[0]) * (third[1] - origin[1])
                - (second[1] - origin[1]) * (third[0] - origin[0]);
            if area.abs() > f32::EPSILON {
                emit([origin, second, third])?;
            }
        }
    }

    for level in first_band.saturating_add(1)..=last_band {
        let height_m = surface.minimum_m + level as f64 * layer_height_m;
        let Some([mut first, mut second]) = contour_segment(triangle, height_m) else {
            continue;
        };
        let low = triangle
            .iter()
            .copied()
            .find(|point| point.height_m < height_m)
            .context("côté inférieur d'une strate introuvable")?;
        let cross = (second.x_m - first.x_m) * (low.y_m - first.y_m)
            - (second.y_m - first.y_m) * (low.x_m - first.x_m);
        if cross > 0. {
            std::mem::swap(&mut first, &mut second);
        }
        let low_z = base_mm + (level - 1) as f64 * layer_height_m * vertical_scale;
        let high_z = base_mm + level as f64 * layer_height_m * vertical_scale;
        let first_low = layer_model_point(first, low_z, side_m, width_mm);
        let second_low = layer_model_point(second, low_z, side_m, width_mm);
        let first_high = layer_model_point(first, high_z, side_m, width_mm);
        let second_high = layer_model_point(second, high_z, side_m, width_mm);
        emit([first_low, second_low, second_high])?;
        emit([first_low, second_high, first_high])?;
    }
    Ok(())
}

fn grid_layer_vertex(surface: &Surface, row: usize, col: usize, side_m: f64) -> LayerVertex {
    let interval = side_m / (surface.count - 1) as f64;
    LayerVertex {
        x_m: col as f64 * interval,
        y_m: row as f64 * interval,
        height_m: surface.heights_m[row * surface.count + col],
    }
}

#[allow(clippy::too_many_arguments)]
fn split_boundary_edge(
    first: LayerVertex,
    second: LayerVertex,
    surface: &Surface,
    side_m: f64,
    width_mm: f64,
    base_mm: f64,
    vertical_scale: f64,
    layer_height_m: f64,
    output: &mut Vec<BoundarySegment>,
) {
    let first_band = layer_index(first.height_m, surface.minimum_m, layer_height_m);
    let second_band = layer_index(second.height_m, surface.minimum_m, layer_height_m);
    let mut positions = vec![0.];
    let low_band = first_band.min(second_band);
    let high_band = first_band.max(second_band);
    for level in low_band.saturating_add(1)..=high_band {
        let height_m = surface.minimum_m + level as f64 * layer_height_m;
        let ratio = (height_m - first.height_m) / (second.height_m - first.height_m);
        if ratio > 0. && ratio < 1. {
            positions.push(ratio);
        }
    }
    positions.push(1.);
    positions.sort_by(f64::total_cmp);
    positions.dedup_by(|left, right| (*left - *right).abs() <= 1e-12);
    let at = |ratio: f64| LayerVertex {
        x_m: first.x_m + (second.x_m - first.x_m) * ratio,
        y_m: first.y_m + (second.y_m - first.y_m) * ratio,
        height_m: first.height_m + (second.height_m - first.height_m) * ratio,
    };
    for pair in positions.windows(2) {
        let start = at(pair[0]);
        let end = at(pair[1]);
        let middle_height = (start.height_m + end.height_m) / 2.;
        let band = layer_index(middle_height, surface.minimum_m, layer_height_m);
        let start_model = layer_model_point(start, 0., side_m, width_mm);
        let end_model = layer_model_point(end, 0., side_m, width_mm);
        output.push(BoundarySegment {
            start: [start_model[0], start_model[1]],
            end: [end_model[0], end_model[1]],
            top_mm: (base_mm + band as f64 * layer_height_m * vertical_scale) as f32,
        });
    }
}

fn layered_boundary_segments(
    surface: &Surface,
    side_m: f64,
    width_mm: f64,
    base_mm: f64,
    vertical_scale: f64,
    layer_height_m: f64,
) -> Vec<BoundarySegment> {
    let mut result = Vec::new();
    let n = surface.count;
    for col in 0..n - 1 {
        split_boundary_edge(
            grid_layer_vertex(surface, 0, col, side_m),
            grid_layer_vertex(surface, 0, col + 1, side_m),
            surface,
            side_m,
            width_mm,
            base_mm,
            vertical_scale,
            layer_height_m,
            &mut result,
        );
    }
    for row in 0..n - 1 {
        split_boundary_edge(
            grid_layer_vertex(surface, row, n - 1, side_m),
            grid_layer_vertex(surface, row + 1, n - 1, side_m),
            surface,
            side_m,
            width_mm,
            base_mm,
            vertical_scale,
            layer_height_m,
            &mut result,
        );
    }
    for col in (0..n - 1).rev() {
        split_boundary_edge(
            grid_layer_vertex(surface, n - 1, col + 1, side_m),
            grid_layer_vertex(surface, n - 1, col, side_m),
            surface,
            side_m,
            width_mm,
            base_mm,
            vertical_scale,
            layer_height_m,
            &mut result,
        );
    }
    for row in (0..n - 1).rev() {
        split_boundary_edge(
            grid_layer_vertex(surface, row + 1, 0, side_m),
            grid_layer_vertex(surface, row, 0, side_m),
            surface,
            side_m,
            width_mm,
            base_mm,
            vertical_scale,
            layer_height_m,
            &mut result,
        );
    }
    result
}

fn emit_layered_boundary<F>(segments: &[BoundarySegment], emit: &mut F) -> Result<()>
where
    F: FnMut([[f32; 3]; 3]) -> Result<()>,
{
    for index in 0..segments.len() {
        let segment = segments[index];
        let previous = segments[(index + segments.len() - 1) % segments.len()];
        let next = segments[(index + 1) % segments.len()];
        let mut polygon = vec![
            [segment.start[0], segment.start[1], 0.],
            [segment.end[0], segment.end[1], 0.],
        ];
        if next.top_mm < segment.top_mm {
            polygon.push([segment.end[0], segment.end[1], next.top_mm]);
        }
        polygon.push([segment.end[0], segment.end[1], segment.top_mm]);
        polygon.push([segment.start[0], segment.start[1], segment.top_mm]);
        if previous.top_mm < segment.top_mm {
            polygon.push([segment.start[0], segment.start[1], previous.top_mm]);
        }
        let center = [
            (segment.start[0] + segment.end[0]) / 2.,
            (segment.start[1] + segment.end[1]) / 2.,
            segment.top_mm / 2.,
        ];
        for pair in polygon
            .iter()
            .copied()
            .zip(polygon.iter().copied().cycle().skip(1))
            .take(polygon.len())
        {
            emit([center, pair.0, pair.1])?;
        }
    }

    let xmin = segments
        .iter()
        .map(|segment| segment.start[0])
        .fold(f32::INFINITY, f32::min);
    let xmax = segments
        .iter()
        .map(|segment| segment.start[0])
        .fold(f32::NEG_INFINITY, f32::max);
    let ymin = segments
        .iter()
        .map(|segment| segment.start[1])
        .fold(f32::INFINITY, f32::min);
    let ymax = segments
        .iter()
        .map(|segment| segment.start[1])
        .fold(f32::NEG_INFINITY, f32::max);
    let center = [(xmin + xmax) / 2., (ymin + ymax) / 2., 0.];
    for index in 0..segments.len() {
        let current = [segments[index].start[0], segments[index].start[1], 0.];
        let next = [
            segments[(index + 1) % segments.len()].start[0],
            segments[(index + 1) % segments.len()].start[1],
            0.,
        ];
        emit([center, next, current])?;
    }
    Ok(())
}

fn emit_layered_terrain<F>(
    surface: &Surface,
    width_mm: f64,
    base_mm: f64,
    vertical_scale: f64,
    layer_height_m: f64,
    mut emit: F,
) -> Result<()>
where
    F: FnMut([[f32; 3]; 3]) -> Result<()>,
{
    positive("layer-height-m", layer_height_m)?;
    layered_level_count(surface, layer_height_m)?;
    // Les coordonnées planimétriques sont normalisées sur `count - 1` : les
    // interpolations restent identiques et le carré imprimé garde sa largeur exacte.
    let side_m = (surface.count - 1) as f64;
    let n = surface.count;
    for row in 0..n - 1 {
        for col in 0..n - 1 {
            let sw = LayerVertex {
                x_m: col as f64,
                y_m: row as f64,
                height_m: surface.heights_m[row * n + col],
            };
            let se = LayerVertex {
                x_m: (col + 1) as f64,
                y_m: row as f64,
                height_m: surface.heights_m[row * n + col + 1],
            };
            let nw = LayerVertex {
                x_m: col as f64,
                y_m: (row + 1) as f64,
                height_m: surface.heights_m[(row + 1) * n + col],
            };
            let ne = LayerVertex {
                x_m: (col + 1) as f64,
                y_m: (row + 1) as f64,
                height_m: surface.heights_m[(row + 1) * n + col + 1],
            };
            emit_layered_source_triangle(
                [sw, se, nw],
                surface,
                side_m,
                width_mm,
                base_mm,
                vertical_scale,
                layer_height_m,
                &mut emit,
            )?;
            emit_layered_source_triangle(
                [se, ne, nw],
                surface,
                side_m,
                width_mm,
                base_mm,
                vertical_scale,
                layer_height_m,
                &mut emit,
            )?;
        }
    }
    let segments = layered_boundary_segments(
        surface,
        side_m,
        width_mm,
        base_mm,
        vertical_scale,
        layer_height_m,
    );
    emit_layered_boundary(&segments, &mut emit)
}

#[allow(clippy::too_many_arguments)]
fn write_atomic(
    output: &Path,
    surface: &Surface,
    width_mm: f64,
    base_mm: f64,
    vertical_scale: f64,
    scale_mm_per_m: f64,
    terrain_style: TerrainStyle,
    buildings: &[ObjectPlan],
    roads: &[RoadTopology],
    road_height_mm: f64,
    trees: &[PrintableTree],
    tree_min_height_mm: f64,
    tree_height_scale: f64,
    tree_shape: TreeShape,
    tree_trunk_diameter_mm: f64,
    tree_crown_diameter_mm: f64,
    triangle_count: u32,
    overwrite: bool,
) -> Result<()> {
    if let Some(parent) = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("création du dossier : {}", parent.display()))?;
    }
    let extension = output
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("stl");
    let partial = output.with_extension(format!("{extension}.part"));
    if partial.exists() {
        std::fs::remove_file(&partial).with_context(|| {
            format!(
                "ancien fichier temporaire inaccessible : {}",
                partial.display()
            )
        })?;
    }
    let result = write_binary_with_style(
        &partial,
        surface,
        width_mm,
        base_mm,
        vertical_scale,
        scale_mm_per_m,
        terrain_style,
        buildings,
        roads,
        road_height_mm,
        trees,
        tree_min_height_mm,
        tree_height_scale,
        tree_shape,
        tree_trunk_diameter_mm,
        tree_crown_diameter_mm,
        triangle_count,
    );
    if let Err(error) = result {
        let _ = std::fs::remove_file(&partial);
        return Err(error);
    }
    if output.exists() {
        if !overwrite {
            let _ = std::fs::remove_file(&partial);
            bail!("le fichier existe déjà : {}", output.display());
        }
        std::fs::remove_file(output)
            .with_context(|| format!("remplacement du fichier : {}", output.display()))?;
    }
    std::fs::rename(&partial, output)
        .with_context(|| format!("publication du STL : {}", output.display()))?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn write_binary(
    path: &Path,
    surface: &Surface,
    width_mm: f64,
    base_mm: f64,
    vertical_scale: f64,
    scale_mm_per_m: f64,
    buildings: &[ObjectPlan],
    roads: &[RoadTopology],
    road_height_mm: f64,
    trees: &[PrintableTree],
    tree_min_height_mm: f64,
    tree_height_scale: f64,
    tree_shape: TreeShape,
    tree_trunk_diameter_mm: f64,
    tree_crown_diameter_mm: f64,
    triangle_count: u32,
) -> Result<()> {
    write_binary_with_style(
        path,
        surface,
        width_mm,
        base_mm,
        vertical_scale,
        scale_mm_per_m,
        TerrainStyle::Continuous,
        buildings,
        roads,
        road_height_mm,
        trees,
        tree_min_height_mm,
        tree_height_scale,
        tree_shape,
        tree_trunk_diameter_mm,
        tree_crown_diameter_mm,
        triangle_count,
    )
}

#[allow(clippy::too_many_arguments)]
fn write_binary_with_style(
    path: &Path,
    surface: &Surface,
    width_mm: f64,
    base_mm: f64,
    vertical_scale: f64,
    scale_mm_per_m: f64,
    terrain_style: TerrainStyle,
    buildings: &[ObjectPlan],
    roads: &[RoadTopology],
    road_height_mm: f64,
    trees: &[PrintableTree],
    tree_min_height_mm: f64,
    tree_height_scale: f64,
    tree_shape: TreeShape,
    tree_trunk_diameter_mm: f64,
    tree_crown_diameter_mm: f64,
    triangle_count: u32,
) -> Result<()> {
    let file = File::create(path)
        .with_context(|| format!("création du fichier STL : {}", path.display()))?;
    let mut writer = BufWriter::new(file);
    let mut header = [0u8; 80];
    let description: &[u8] = match terrain_style {
        TerrainStyle::Continuous => {
            b"geo-data : relief LiDAR, batiments, routes et arbres imprimables"
        }
        TerrainStyle::Layered { .. } => {
            b"geo-data : relief LiDAR en couches topographiques imprimables"
        }
    };
    header[..description.len()].copy_from_slice(description);
    writer.write_all(&header)?;
    writer.write_all(&triangle_count.to_le_bytes())?;

    match terrain_style {
        TerrainStyle::Continuous => {
            write_continuous_terrain(&mut writer, surface, width_mm, base_mm, vertical_scale)?;
        }
        TerrainStyle::Layered { layer_height_m } => {
            emit_layered_terrain(
                surface,
                width_mm,
                base_mm,
                vertical_scale,
                layer_height_m,
                |triangle| write_triangle(&mut writer, triangle),
            )?;
        }
    }
    write_buildings(&mut writer, buildings, scale_mm_per_m, base_mm)?;
    write_roads(
        &mut writer,
        surface,
        width_mm,
        base_mm,
        vertical_scale,
        scale_mm_per_m,
        roads,
        road_height_mm,
        terrain_style,
    )?;
    write_trees(
        &mut writer,
        surface,
        width_mm,
        base_mm,
        vertical_scale,
        scale_mm_per_m,
        trees,
        tree_min_height_mm,
        tree_height_scale,
        tree_shape,
        tree_trunk_diameter_mm,
        tree_crown_diameter_mm,
        terrain_style,
    )?;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok(())
}

fn write_buildings<W: Write>(
    writer: &mut W,
    buildings: &[ObjectPlan],
    scale_mm_per_m: f64,
    base_mm: f64,
) -> Result<()> {
    let bottom_z = (base_mm * 0.5) as f32;
    for building in buildings {
        let (roofs, boundary) = building_topology(building)?;
        let vertex = |index: usize| -> Result<[f32; 3]> {
            let point = building
                .vertices
                .get(index)
                .context("indice de sommet de bâtiment invalide")?;
            Ok([
                (point[0] * scale_mm_per_m) as f32,
                (point[1] * scale_mm_per_m) as f32,
                (base_mm + point[2] * scale_mm_per_m) as f32,
            ])
        };
        for triangle in roofs {
            let top = [
                vertex(triangle[0])?,
                vertex(triangle[1])?,
                vertex(triangle[2])?,
            ];
            write_triangle(writer, top)?;
            let bottom = [
                [top[0][0], top[0][1], bottom_z],
                [top[2][0], top[2][1], bottom_z],
                [top[1][0], top[1][1], bottom_z],
            ];
            write_triangle(writer, bottom)?;
        }
        for (first, second) in boundary {
            let top_first = vertex(first)?;
            let top_second = vertex(second)?;
            let bottom_first = [top_first[0], top_first[1], bottom_z];
            let bottom_second = [top_second[0], top_second[1], bottom_z];
            write_triangle(writer, [top_first, bottom_first, top_second])?;
            write_triangle(writer, [bottom_first, bottom_second, top_second])?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_roads<W: Write>(
    writer: &mut W,
    surface: &Surface,
    width_mm: f64,
    base_mm: f64,
    vertical_scale: f64,
    scale_mm_per_m: f64,
    roads: &[RoadTopology],
    road_height_mm: f64,
    terrain_style: TerrainStyle,
) -> Result<()> {
    let side_m = width_mm / scale_mm_per_m;
    let overlap_mm = road_height_mm * 0.5;
    for road in roads {
        let vertex = |index: usize| -> Result<([f32; 3], [f32; 3])> {
            let [x_m, y_m] = *road
                .vertices
                .get(index)
                .context("indice de sommet de route invalide")?;
            let terrain_mm = base_mm
                + (printable_terrain_height(surface, side_m, x_m, y_m, terrain_style)
                    - surface.minimum_m)
                    * vertical_scale;
            let x = (x_m * scale_mm_per_m) as f32;
            let y = (y_m * scale_mm_per_m) as f32;
            Ok((
                [x, y, (terrain_mm + road_height_mm) as f32],
                [x, y, (terrain_mm - overlap_mm) as f32],
            ))
        };
        for triangle in &road.triangles {
            let a = vertex(triangle[0])?;
            let b = vertex(triangle[1])?;
            let c = vertex(triangle[2])?;
            write_triangle(writer, [a.0, b.0, c.0])?;
            write_triangle(writer, [a.1, c.1, b.1])?;
        }
        for &(first, second) in &road.boundary {
            let first = vertex(first)?;
            let second = vertex(second)?;
            write_triangle(writer, [first.0, first.1, second.0])?;
            write_triangle(writer, [first.1, second.1, second.0])?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_trees<W: Write>(
    writer: &mut W,
    surface: &Surface,
    width_mm: f64,
    base_mm: f64,
    vertical_scale: f64,
    scale_mm_per_m: f64,
    trees: &[PrintableTree],
    minimum_height_mm: f64,
    height_scale: f64,
    shape: TreeShape,
    minimum_trunk_diameter_mm: f64,
    minimum_crown_diameter_mm: f64,
    terrain_style: TerrainStyle,
) -> Result<()> {
    let side_m = width_mm / scale_mm_per_m;
    for tree in trees {
        let height = (tree.height_m * vertical_scale * height_scale).max(minimum_height_mm);
        let trunk_radius =
            (tree.trunk_radius_m * scale_mm_per_m).max(minimum_trunk_diameter_mm / 2.);
        let crown_radius = (tree.crown_radius_m * scale_mm_per_m)
            .max(minimum_crown_diameter_mm / 2.)
            .max(trunk_radius * 1.5);
        let mut x = tree.x_m * scale_mm_per_m;
        let mut y = tree.y_m * scale_mm_per_m;
        x = x.clamp(crown_radius, (width_mm - crown_radius).max(crown_radius));
        y = y.clamp(crown_radius, (width_mm - crown_radius).max(crown_radius));
        let terrain_mm = base_mm
            + (printable_terrain_height(surface, side_m, tree.x_m, tree.y_m, terrain_style)
                - surface.minimum_m)
                * vertical_scale;
        let bottom_z = terrain_mm - minimum_trunk_diameter_mm.min(0.4) / 2.;
        let trunk_top_z = terrain_mm + height * 0.62;
        write_tree_trunk(writer, x, y, trunk_radius, bottom_z, trunk_top_z)?;
        match shape {
            TreeShape::Rounded => {
                write_rounded_foliage(writer, x, y, terrain_mm, height, crown_radius)?
            }
            TreeShape::Conifer => {
                write_conifer_foliage(writer, x, y, terrain_mm, height, crown_radius)?
            }
        }
    }
    Ok(())
}

fn tree_ring(x: f64, y: f64, z: f64, radius: f64) -> Vec<[f32; 3]> {
    (0..TREE_SEGMENTS)
        .map(|index| {
            let angle = std::f64::consts::TAU * index as f64 / TREE_SEGMENTS as f64;
            let (sin, cos) = angle.sin_cos();
            [
                (x + cos * radius) as f32,
                (y + sin * radius) as f32,
                z as f32,
            ]
        })
        .collect()
}

fn write_tree_trunk<W: Write>(
    writer: &mut W,
    x: f64,
    y: f64,
    radius: f64,
    bottom_z: f64,
    top_z: f64,
) -> Result<()> {
    let bottom = tree_ring(x, y, bottom_z, radius);
    let top = tree_ring(x, y, top_z, radius);
    for index in 1..TREE_SEGMENTS - 1 {
        write_triangle(writer, [top[0], top[index], top[index + 1]])?;
        write_triangle(writer, [bottom[0], bottom[index + 1], bottom[index]])?;
    }
    for index in 0..TREE_SEGMENTS {
        let next = (index + 1) % TREE_SEGMENTS;
        write_triangle(writer, [bottom[index], bottom[next], top[next]])?;
        write_triangle(writer, [bottom[index], top[next], top[index]])?;
    }
    Ok(())
}

/// Houppier ovoïde fermé : le pôle inférieur est à 12 % de la hauteur,
/// nettement plus bas que l'ancienne bipyramide pointue.
fn write_rounded_foliage<W: Write>(
    writer: &mut W,
    x: f64,
    y: f64,
    terrain_z: f64,
    height: f64,
    radius: f64,
) -> Result<()> {
    let rings = [
        tree_ring(x, y, terrain_z + height * 0.28, radius * 0.72),
        tree_ring(x, y, terrain_z + height * 0.55, radius),
        tree_ring(x, y, terrain_z + height * 0.80, radius * 0.72),
    ];
    let bottom = [x as f32, y as f32, (terrain_z + height * 0.12) as f32];
    let top = [x as f32, y as f32, (terrain_z + height) as f32];
    for index in 0..TREE_SEGMENTS {
        let next = (index + 1) % TREE_SEGMENTS;
        write_triangle(writer, [bottom, rings[0][next], rings[0][index]])?;
        for pair in rings.windows(2) {
            write_triangle(writer, [pair[0][index], pair[0][next], pair[1][next]])?;
            write_triangle(writer, [pair[0][index], pair[1][next], pair[1][index]])?;
        }
        write_triangle(writer, [top, rings[2][index], rings[2][next]])?;
    }
    Ok(())
}

fn write_conifer_foliage<W: Write>(
    writer: &mut W,
    x: f64,
    y: f64,
    terrain_z: f64,
    height: f64,
    radius: f64,
) -> Result<()> {
    for (base, apex, tier_radius) in [(0.18, 0.72, 1.0), (0.42, 0.86, 0.75), (0.62, 1.0, 0.50)] {
        write_closed_cone(
            writer,
            x,
            y,
            terrain_z + height * base,
            terrain_z + height * apex,
            radius * tier_radius,
        )?;
    }
    Ok(())
}

fn write_closed_cone<W: Write>(
    writer: &mut W,
    x: f64,
    y: f64,
    base_z: f64,
    apex_z: f64,
    radius: f64,
) -> Result<()> {
    let ring = tree_ring(x, y, base_z, radius);
    let apex = [x as f32, y as f32, apex_z as f32];
    for index in 1..TREE_SEGMENTS - 1 {
        write_triangle(writer, [ring[0], ring[index + 1], ring[index]])?;
    }
    for index in 0..TREE_SEGMENTS {
        let next = (index + 1) % TREE_SEGMENTS;
        write_triangle(writer, [apex, ring[index], ring[next]])?;
    }
    Ok(())
}

fn bottom_boundary(count: usize, width_mm: f64) -> Vec<[f32; 3]> {
    let mut points = Vec::with_capacity(4 * (count - 1));
    // Garder exactement le même ordre d'opérations que `point` afin que les
    // coordonnées f32 des arêtes du fond et des parois soient identiques bit à bit.
    let coordinate = |index: usize| (index as f64 / (count - 1) as f64 * width_mm) as f32;
    let width = width_mm as f32;
    for col in 0..count {
        points.push([coordinate(col), 0., 0.]);
    }
    for row in 1..count {
        points.push([width, coordinate(row), 0.]);
    }
    for col in (0..count - 1).rev() {
        points.push([coordinate(col), width, 0.]);
    }
    for row in (1..count - 1).rev() {
        points.push([0., coordinate(row), 0.]);
    }
    points
}

/// Écrit une paroi entre deux sommets consécutifs. `reverse` inverse sa normale.
fn write_wall<W: Write>(
    writer: &mut W,
    first: [f32; 3],
    second: [f32; 3],
    reverse: bool,
) -> Result<()> {
    let bottom_first = [first[0], first[1], 0.];
    let bottom_second = [second[0], second[1], 0.];
    if reverse {
        write_triangle(writer, [bottom_first, first, second])?;
        write_triangle(writer, [bottom_first, second, bottom_second])?;
    } else {
        write_triangle(writer, [bottom_first, second, first])?;
        write_triangle(writer, [bottom_first, bottom_second, second])?;
    }
    Ok(())
}

fn write_triangle<W: Write>(writer: &mut W, vertices: [[f32; 3]; 3]) -> Result<()> {
    let normal = normal(vertices);
    for value in normal.into_iter().chain(vertices.into_iter().flatten()) {
        writer.write_all(&value.to_le_bytes())?;
    }
    writer.write_all(&0u16.to_le_bytes())?;
    Ok(())
}

fn normal(vertices: [[f32; 3]; 3]) -> [f32; 3] {
    let u = [
        vertices[1][0] - vertices[0][0],
        vertices[1][1] - vertices[0][1],
        vertices[1][2] - vertices[0][2],
    ];
    let v = [
        vertices[2][0] - vertices[0][0],
        vertices[2][1] - vertices[0][1],
        vertices[2][2] - vertices[0][2],
    ];
    let cross = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let length = (cross[0].powi(2) + cross[1].powi(2) + cross[2].powi(2)).sqrt();
    if length == 0. {
        [0., 0., 0.]
    } else {
        [cross[0] / length, cross[1] / length, cross[2] / length]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::models::TerrainConfig;
    use crate::test_support as support;
    use std::collections::BTreeMap;

    fn surface() -> Surface {
        let heights_m = vec![
            100., 100., 100., 100., 100., 110., 100., 100., 100., 100., 120., 100., 100., 100.,
            100., 100.,
        ];
        Surface {
            count: 4,
            heights_without_buildings_m: heights_m.clone(),
            ground_m: vec![100.; 16],
            canopy_tops: Vec::new(),
            heights_m,
            minimum_m: 100.,
            points_read: 9,
            points_used: 9,
            points_available: 9,
            files_used: 1,
            interpolated: 0,
            cached: false,
        }
    }

    fn temp_file(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "geo-data-stl-{}-{}-{name}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn block_dimensions_and_triangle_count_are_exact() {
        let path = temp_file("block.stl");
        let surface = surface();
        let triangles = triangle_count(surface.count).unwrap();
        write_binary(
            &path,
            &surface,
            137.,
            3.,
            0.05,
            0.05,
            &[],
            &[],
            0.6,
            &[],
            5.,
            1.3,
            TreeShape::Rounded,
            1.2,
            3.,
            triangles,
        )
        .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(triangles, 54);
        assert_eq!(bytes.len(), 84 + triangles as usize * 50);
        assert_eq!(u32::from_le_bytes(bytes[80..84].try_into().unwrap()), 54);
        let values: Vec<f32> = bytes[84..]
            .chunks_exact(50)
            .flat_map(|triangle| {
                (12..48).step_by(4).map(move |offset| {
                    f32::from_le_bytes(triangle[offset..offset + 4].try_into().unwrap())
                })
            })
            .collect();
        let xs = values.iter().step_by(3).copied().collect::<Vec<_>>();
        let ys = values
            .iter()
            .skip(1)
            .step_by(3)
            .copied()
            .collect::<Vec<_>>();
        let zs = values
            .iter()
            .skip(2)
            .step_by(3)
            .copied()
            .collect::<Vec<_>>();
        assert_eq!(xs.iter().copied().fold(f32::INFINITY, f32::min), 0.);
        assert_eq!(xs.iter().copied().fold(f32::NEG_INFINITY, f32::max), 137.);
        assert_eq!(ys.iter().copied().fold(f32::INFINITY, f32::min), 0.);
        assert_eq!(ys.iter().copied().fold(f32::NEG_INFINITY, f32::max), 137.);
        assert_eq!(zs.iter().copied().fold(f32::INFINITY, f32::min), 0.);
        assert_eq!(zs.iter().copied().fold(f32::NEG_INFINITY, f32::max), 4.);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn every_mesh_edge_is_shared_by_two_triangles() {
        let path = temp_file("manifold.stl");
        let surface = surface();
        write_binary(
            &path,
            &surface,
            137.,
            3.,
            0.05,
            0.05,
            &[],
            &[],
            0.6,
            &[],
            5.,
            1.3,
            TreeShape::Rounded,
            1.2,
            3.,
            triangle_count(surface.count).unwrap(),
        )
        .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mut edges = BTreeMap::<([u32; 3], [u32; 3]), usize>::new();
        for triangle in bytes[84..].chunks_exact(50) {
            let mut vertices = [[0u32; 3]; 3];
            for (vertex, values) in vertices.iter_mut().enumerate() {
                for (axis, value) in values.iter_mut().enumerate() {
                    let offset = 12 + (vertex * 3 + axis) * 4;
                    *value = u32::from_le_bytes(triangle[offset..offset + 4].try_into().unwrap());
                }
            }
            for (first, second) in [(0, 1), (1, 2), (2, 0)] {
                let edge = if vertices[first] < vertices[second] {
                    (vertices[first], vertices[second])
                } else {
                    (vertices[second], vertices[first])
                };
                *edges.entry(edge).or_default() += 1;
            }
        }
        assert!(edges.values().all(|count| *count == 2));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn layered_export_uses_real_steps_and_remains_manifold() {
        let path = temp_file("layered.stl");
        let heights_m = vec![
            100.1, 100.2, 100.3, 100.2, 102.6, 100.4, 100.3, 100.4, 100.5,
        ];
        let surface = Surface {
            count: 3,
            heights_without_buildings_m: heights_m.clone(),
            ground_m: heights_m.clone(),
            canopy_tops: Vec::new(),
            heights_m,
            minimum_m: 100.1,
            points_read: 9,
            points_used: 9,
            points_available: 9,
            files_used: 1,
            interpolated: 0,
            cached: false,
        };
        let style = TerrainStyle::Layered { layer_height_m: 1. };
        let triangles = terrain_triangle_count(&surface, 40., 3., 0.05, style).unwrap();
        write_binary_with_style(
            &path,
            &surface,
            40.,
            3.,
            0.05,
            0.04,
            style,
            &[],
            &[],
            0.6,
            &[],
            5.,
            1.3,
            TreeShape::Rounded,
            1.2,
            3.,
            triangles,
        )
        .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.len(), 84 + triangles as usize * 50);

        let mut edges = BTreeMap::<([u32; 3], [u32; 3]), usize>::new();
        let mut z_values = BTreeSet::new();
        for triangle in bytes[84..].chunks_exact(50) {
            let mut vertices = [[0u32; 3]; 3];
            for (vertex, values) in vertices.iter_mut().enumerate() {
                for (axis, value) in values.iter_mut().enumerate() {
                    let offset = 12 + (vertex * 3 + axis) * 4;
                    *value = u32::from_le_bytes(triangle[offset..offset + 4].try_into().unwrap());
                }
                z_values.insert(values[2]);
            }
            for (first, second) in [(0, 1), (1, 2), (2, 0)] {
                let edge = if vertices[first] < vertices[second] {
                    (vertices[first], vertices[second])
                } else {
                    (vertices[second], vertices[first])
                };
                *edges.entry(edge).or_default() += 1;
            }
        }
        assert!(edges.values().all(|count| *count == 2));
        let actual = z_values.into_iter().map(f32::from_bits).collect::<Vec<_>>();
        // 1,5 mm est uniquement le sommet de triangulation coplanaire des
        // parois extérieures ; les surfaces de terrain sont bien à 3, 3,05 et 3,10 mm.
        assert_eq!(actual, vec![0., 1.5, 3., 3.05, 3.1]);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn layered_binary_exposes_a_real_layer_height_option() {
        let arguments = LayeredStlArguments::try_parse_from([
            "geo-stl-layers",
            "--width-mm",
            "150",
            "--layer-height-m",
            "1.25",
        ])
        .unwrap();
        assert_eq!(arguments.layer_height_m, 1.25);
        assert_eq!(
            arguments.output,
            PathBuf::from("exports/modele-couches.stl")
        );
    }

    #[test]
    fn layered_contours_join_the_outer_walls_without_open_edges() {
        let heights_m = vec![100.1, 102.6, 100.2, 102.7];
        let surface = Surface {
            count: 2,
            heights_without_buildings_m: heights_m.clone(),
            ground_m: heights_m.clone(),
            canopy_tops: Vec::new(),
            heights_m,
            minimum_m: 100.1,
            points_read: 4,
            points_used: 4,
            points_available: 4,
            files_used: 1,
            interpolated: 0,
            cached: false,
        };
        let mut triangles = Vec::new();
        emit_layered_terrain(&surface, 40., 3., 0.05, 1., |triangle| {
            triangles.push(triangle);
            Ok(())
        })
        .unwrap();
        let mut edges = BTreeMap::<([u32; 3], [u32; 3]), usize>::new();
        for triangle in triangles {
            let vertices = triangle.map(|point| point.map(f32::to_bits));
            for (first, second) in [(0, 1), (1, 2), (2, 0)] {
                let edge = if vertices[first] < vertices[second] {
                    (vertices[first], vertices[second])
                } else {
                    (vertices[second], vertices[first])
                };
                *edges.entry(edge).or_default() += 1;
            }
        }
        assert!(edges.values().all(|count| *count == 2));
    }

    #[test]
    fn classified_lidar_builds_the_upper_surface() {
        let root = support::directory();
        let folder = root.join("LIDAR_HD/edition_2026-01-01");
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("LHD_FXX_test_LAMB93.copc.laz");
        let mut builder = las::Builder::from((1, 2));
        builder.point_format = las::point::Format::new(0).unwrap();
        builder.point_format.is_compressed = true;
        let mut writer = las::Writer::from_path(&path, builder.into_header().unwrap()).unwrap();
        for row in 0..3 {
            for col in 0..3 {
                writer
                    .write_point(las::Point {
                        x: col as f64 * 10.,
                        y: row as f64 * 10.,
                        z: 100.,
                        classification: las::point::Classification::new(2).unwrap(),
                        ..Default::default()
                    })
                    .unwrap();
            }
        }
        writer
            .write_point(las::Point {
                x: 10.,
                y: 10.,
                z: 112.,
                classification: las::point::Classification::new(6).unwrap(),
                ..Default::default()
            })
            .unwrap();
        writer
            .write_point(las::Point {
                x: 18.,
                y: 18.,
                z: 111.,
                classification: las::point::Classification::new(5).unwrap(),
                ..Default::default()
            })
            .unwrap();
        writer.close().unwrap();
        drop(writer);
        let size = std::fs::metadata(&path).unwrap().len();
        std::fs::write(
            path.with_extension("laz.download.json"),
            serde_json::json!({"size": size}).to_string(),
        )
        .unwrap();

        let mut settings = config::parse("").unwrap();
        settings.area.center = [10., 10.];
        settings.area.crs = "EPSG:2154".into();
        settings.area.side_m = 20.;
        settings.terrain.grid_step_m = 10.;
        settings.terrain.max_vertices = 100;
        let (result, cache_path) =
            read_surface_with_cache(&root, &settings, 0., 0., StlQuality::Balanced, false).unwrap();
        assert_eq!(result.count, 3);
        assert_eq!(result.minimum_m, 100.);
        assert_eq!(result.heights_m[4], 112.);
        assert_eq!(result.heights_without_buildings_m[4], 100.);
        assert_eq!(result.ground_m[4], 100.);
        assert_eq!(result.points_used, 11);
        assert_eq!(result.canopy_tops, vec![[18., 18., 111.]]);
        assert!(!result.cached);
        std::fs::remove_file(&cache_path).unwrap();
        let config_path = root.join("export.toml");
        std::fs::write(&config_path, toml::to_string_pretty(&settings).unwrap()).unwrap();
        let warmed_cache = warm_surface_cache(&config_path, StlQuality::Balanced).unwrap();
        assert!(warmed_cache.is_file());
        let cached = read_surface(&root, &settings, 0., 0., StlQuality::Balanced, false).unwrap();
        assert!(cached.cached);
        assert_eq!(cached.heights_m, result.heights_m);
        assert_eq!(
            cached.heights_without_buildings_m,
            result.heights_without_buildings_m
        );
        assert_eq!(cached.ground_m, result.ground_m);
        assert_eq!(cached.canopy_tops, result.canopy_tops);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn holes_are_filled_only_within_the_configured_radius() {
        let mut heights = vec![None; 25];
        heights[0] = Some(100.);
        fill_holes(&mut heights, 5, 1);
        assert_eq!(heights[1], Some(100.));
        assert_eq!(heights[5], Some(100.));
        assert!(heights[12].is_none());
    }

    #[test]
    fn remaining_holes_are_interpolated_instead_of_rejecting_the_stl() {
        let mut heights = vec![Some(100.); 25];
        heights[12] = None;
        heights[13] = None;
        fill_remaining_holes(&mut heights, 5);
        assert!(heights.iter().all(Option::is_some));
        assert_eq!(heights[12], Some(100.));
        assert_eq!(heights[13], Some(100.));
    }

    #[test]
    fn smoothing_reduces_small_spikes_without_moving_the_boundary() {
        let mut surface = Surface {
            count: 3,
            heights_m: vec![100., 100., 100., 100., 108., 100., 100., 100., 100.],
            heights_without_buildings_m: vec![100.; 9],
            ground_m: vec![100.; 9],
            canopy_tops: Vec::new(),
            minimum_m: 100.,
            points_read: 9,
            points_used: 9,
            points_available: 9,
            files_used: 1,
            interpolated: 0,
            cached: false,
        };
        smooth_surface(&mut surface, 2, 0.5);
        assert_eq!(surface.heights_m[0], 100.);
        assert!(surface.heights_m[4] > 100.);
        assert!(surface.heights_m[4] < 104.1);
        assert_eq!(surface.minimum_m, 100.);
    }

    #[test]
    fn building_roof_becomes_a_closed_printable_shell() {
        let building = ObjectPlan {
            name: "test".into(),
            kind: "mesh".into(),
            vertices: vec![[1., 1., 10.], [3., 1., 10.], [3., 3., 10.], [1., 3., 10.]],
            faces: vec![vec![0, 1, 2], vec![0, 2, 3]],
            roof_faces: vec![0, 1],
            uv: Vec::new(),
            lines: Vec::new(),
            properties: Value::Null,
        };
        assert_eq!(
            building_triangle_count(std::slice::from_ref(&building)).unwrap(),
            12
        );
        let mut bytes = Vec::new();
        write_buildings(&mut bytes, &[building], 1., 3.).unwrap();
        assert_eq!(bytes.len(), 12 * 50);
    }

    #[test]
    fn road_surface_becomes_a_closed_raised_ribbon() {
        // Les deux triangles utilisent volontairement des indices distincts sur
        // leur diagonale commune : la topologie doit les ressouder par position.
        let road = ObjectPlan {
            name: "route test".into(),
            kind: "mesh".into(),
            vertices: vec![
                [1., 1., 100.],
                [3., 1., 100.],
                [3., 3., 100.],
                [1., 1., 100.],
                [3., 3., 100.],
                [1., 3., 100.],
            ],
            faces: vec![vec![0, 1, 2], vec![3, 4, 5]],
            roof_faces: Vec::new(),
            uv: Vec::new(),
            lines: Vec::new(),
            properties: Value::Null,
        };
        let topology = road_topology(&road).unwrap();
        assert_eq!(topology.vertices.len(), 4);
        assert_eq!(topology.triangles.len(), 2);
        assert_eq!(topology.boundary.len(), 4);
        assert_eq!(
            road_triangle_count(std::slice::from_ref(&topology)).unwrap(),
            12
        );
        let mut flat = surface();
        flat.heights_m.fill(100.);
        flat.minimum_m = 100.;
        let mut bytes = Vec::new();
        write_roads(
            &mut bytes,
            &flat,
            4.,
            3.,
            1.,
            1.,
            &[topology],
            0.6,
            TerrainStyle::Continuous,
        )
        .unwrap();
        assert_eq!(bytes.len(), 12 * 50);
    }

    #[test]
    fn lidar_canopy_becomes_a_tree_with_a_thick_closed_trunk() {
        let mut settings = config::parse("").unwrap();
        settings.area.side_m = 40.;
        settings.vegetation.min_height_m = 3.;
        settings.vegetation.max_height_m = 30.;
        let terrain = Surface {
            count: 5,
            heights_m: vec![100.; 25],
            heights_without_buildings_m: vec![100.; 25],
            ground_m: vec![100.; 25],
            canopy_tops: vec![[20., 20., 112.]],
            minimum_m: 100.,
            points_read: 25,
            points_used: 25,
            points_available: 25,
            files_used: 1,
            interpolated: 0,
            cached: false,
        };
        let trees = prepare_trees(&settings, &terrain, 0., 0.);
        assert_eq!(trees.len(), 1);
        assert_eq!(trees[0].height_m, 12.);
        assert_eq!(
            tree_triangle_count(trees.len(), TreeShape::Rounded).unwrap(),
            76
        );
        let mut bytes = Vec::new();
        write_trees(
            &mut bytes,
            &terrain,
            40.,
            3.,
            1.,
            1.,
            &trees,
            5.,
            1.3,
            TreeShape::Rounded,
            1.2,
            3.,
            TerrainStyle::Continuous,
        )
        .unwrap();
        assert_eq!(bytes.len(), 76 * 50);

        let mut conifer = Vec::new();
        write_trees(
            &mut conifer,
            &terrain,
            40.,
            3.,
            1.,
            1.,
            &trees,
            5.,
            1.3,
            TreeShape::Conifer,
            1.2,
            3.,
            TerrainStyle::Continuous,
        )
        .unwrap();
        assert_eq!(
            tree_triangle_count(trees.len(), TreeShape::Conifer).unwrap(),
            70
        );
        assert_eq!(conifer.len(), 70 * 50);
    }

    #[test]
    fn relief_bumps_without_vegetation_class_do_not_create_trees() {
        let mut settings = config::parse("").unwrap();
        settings.area.side_m = 40.;
        let mut terrain = surface();
        terrain.count = 4;
        terrain.heights_without_buildings_m[5] = 130.;
        terrain.canopy_tops.clear();
        assert!(prepare_trees(&settings, &terrain, 0., 0.).is_empty());
    }

    #[test]
    fn lidar_noise_and_wires_are_not_printed_as_spikes() {
        let terrain = TerrainConfig {
            source: "lidar".into(),
            grid_step_m: 10.,
            max_vertices: 100,
            vertical_exaggeration: 1.,
            altitude_origin: "min".into(),
            valid_altitude_min_m: -500.,
            valid_altitude_max_m: 9000.,
            lidar_ground_class: 2,
            lidar_fill_radius_cells: 1,
        };
        assert!(surface_class(
            terrain.lidar_ground_class,
            terrain.lidar_ground_class
        ));
        assert!(surface_class(5, terrain.lidar_ground_class));
        assert!(surface_class(6, terrain.lidar_ground_class));
        assert!(!surface_class(7, terrain.lidar_ground_class));
        assert!(!surface_class(13, terrain.lidar_ground_class));
        assert!(!surface_class(14, terrain.lidar_ground_class));
        assert!(!surface_class(18, terrain.lidar_ground_class));
    }
}
