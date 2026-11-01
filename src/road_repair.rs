//! Contexte : secours local lorsque l'union historique des chaussées devient non-manifold.
//! Usage : reconstruire des rubans routiers fermés depuis les axes BD TOPO et les ajouter au STL.

use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
};

use serde_json::Value;

use crate::{
    domain::{ExportFormat, ExportQuality, ExportRequest},
    error::{AppError, AppResult},
};

type Point2 = [f64; 2];
type Point3 = [f32; 3];
type Triangle = [Point3; 3];

/// Résumé court ajouté au journal du worker après une réparation réussie.
#[derive(Clone, Copy, Debug)]
pub struct RoadRepairReport {
    pub segments: usize,
    pub triangles: usize,
    pub source_files: usize,
}

/// Résumé de la nappe forestière ajoutée au STL depuis les deux surfaces LiDAR.
#[derive(Clone, Copy, Debug)]
pub struct ForestCanopyReport {
    pub patches: usize,
    pub triangles: usize,
    pub road_cells_removed: usize,
}

/// Paramètres physiques suffisants pour reconstruire les routes sans exposer le moteur historique.
#[derive(Clone, Debug)]
struct RepairSettings {
    width_mm: f64,
    base_mm: f64,
    road_min_width_mm: f64,
    road_height_mm: f64,
    vertical_exaggeration: f64,
    layer_height_m: Option<f64>,
    include_buildings: bool,
    include_trees: bool,
}

/// Grille d'altitudes déjà calculée et mise en cache par le premier passage STL.
#[derive(Debug)]
struct SurfaceCache {
    xmin: f64,
    ymin: f64,
    side_m: f64,
    count: usize,
    heights: Vec<f64>,
    minimum_m: f64,
}

/// Surfaces LiDAR nécessaires pour distinguer le sol du sommet de la végétation.
#[derive(Debug)]
struct CanopySurface {
    xmin: f64,
    ymin: f64,
    side_m: f64,
    count: usize,
    canopy: Vec<f64>,
    ground: Vec<f64>,
    minimum_m: f64,
}

/// Axe routier et largeur réelle éventuellement fournie par BD TOPO.
#[derive(Debug)]
struct RoadLine {
    points: Vec<Point2>,
    source_width_m: Option<f64>,
}

/// Extrait le chemin du cache de surface annoncé par le moteur dans sa sortie détaillée.
pub fn surface_cache_path(output: &str) -> Option<PathBuf> {
    output.lines().find_map(|line| {
        let marker = if line.contains("[STL cache] surface réutilisée :") {
            "[STL cache] surface réutilisée :"
        } else if line.contains("[STL cache] surface enregistrée :") {
            "[STL cache] surface enregistrée :"
        } else {
            return None;
        };
        let value = line.split_once(marker)?.1.trim();
        (!value.is_empty()).then(|| PathBuf::from(value))
    })
}

/// Reconstruit les rubans hors du thread asynchrone afin de ne pas bloquer les autres workers.
pub async fn append_segmented_roads(
    output: PathBuf,
    data_root: PathBuf,
    surface_cache: PathBuf,
    request: ExportRequest,
) -> AppResult<RoadRepairReport> {
    tokio::task::spawn_blocking(move || {
        let settings = RepairSettings::from_request(&request);
        repair_sync(&output, &data_root, &surface_cache, &settings)
    })
    .await
    .map_err(|error| AppError::engine(format!("worker de réparation des routes : {error}")))?
}

/// Construit une enveloppe forestière fermée sans réintroduire les arbres ponctuels du moteur.
pub async fn append_forest_canopy(
    output: PathBuf,
    data_root: PathBuf,
    surface_cache: PathBuf,
    request: ExportRequest,
) -> AppResult<ForestCanopyReport> {
    tokio::task::spawn_blocking(move || {
        let settings = RepairSettings::from_request(&request);
        append_canopy_sync(&output, &data_root, &surface_cache, &settings, &request)
    })
    .await
    .map_err(|error| AppError::engine(format!("worker de canopée LiDAR : {error}")))?
}

impl RepairSettings {
    /// Copie uniquement les valeurs nécessaires à la géométrie de secours.
    fn from_request(request: &ExportRequest) -> Self {
        Self {
            width_mm: request.advanced.width_mm.unwrap_or(150.0),
            base_mm: request.advanced.base_mm.unwrap_or(3.0),
            road_min_width_mm: request.advanced.road_min_width_mm.unwrap_or(1.0),
            road_height_mm: request.advanced.road_height_mm.unwrap_or(0.6),
            vertical_exaggeration: request.advanced.vertical_exaggeration.unwrap_or(1.0),
            layer_height_m: (request.format == ExportFormat::StlLayers)
                .then(|| request.advanced.layer_height_m.unwrap_or(1.0)),
            include_buildings: request.advanced.include_buildings.unwrap_or(true),
            include_trees: request.advanced.include_trees.unwrap_or(true),
        }
    }
}

/// Lit les sources, construit les prismes de route et publie le STL complété en une écriture.
fn repair_sync(
    output: &Path,
    data_root: &Path,
    cache_path: &Path,
    settings: &RepairSettings,
) -> AppResult<RoadRepairReport> {
    let surface = read_surface_cache(cache_path, settings)?;
    let road_directory = find_road_directory(data_root, &surface)?;
    let (lines, source_files) = read_road_lines(&road_directory)?;
    let (triangles, segments) = build_road_triangles(&surface, &lines, settings);
    if triangles.is_empty() {
        return Err(AppError::engine(
            "aucun tronçon routier exploitable dans l'emprise".to_owned(),
        ));
    }
    append_binary_stl(output, &triangles)?;
    Ok(RoadRepairReport {
        segments,
        triangles: triangles.len(),
        source_files,
    })
}

/// Lit l'en-tête GSTL puis choisit la grille réellement utilisée par le relief imprimé.
fn read_surface_cache(path: &Path, settings: &RepairSettings) -> AppResult<SurfaceCache> {
    let bytes = fs::read(path).map_err(|error| {
        AppError::engine(format!(
            "lecture du cache de surface {} : {error}",
            path.display()
        ))
    })?;
    if bytes.get(..8) != Some(b"GSTLC04\0") {
        return Err(AppError::engine("signature du cache de surface inconnue"));
    }
    let mut cursor = 8;
    let header_size = read_u32(&bytes, &mut cursor)? as usize;
    let header_end = cursor
        .checked_add(header_size)
        .ok_or_else(|| AppError::engine("en-tête de cache excessif"))?;
    let header: Value = serde_json::from_slice(
        bytes
            .get(cursor..header_end)
            .ok_or_else(|| AppError::engine("en-tête de cache tronqué"))?,
    )
    .map_err(|error| AppError::engine(format!("en-tête de cache invalide : {error}")))?;
    cursor = header_end;
    let key = header
        .get("key")
        .ok_or_else(|| AppError::engine("clé absente du cache de surface"))?;
    let xmin = json_f64(key, "xmin")?;
    let ymin = json_f64(key, "ymin")?;
    let side_m = json_f64(key, "side_m")?;
    let count = json_u64(key, "count")? as usize;
    let length = read_u64(&bytes, &mut cursor)? as usize;
    if length != count.saturating_mul(count) || count < 2 {
        return Err(AppError::engine("dimensions de cache incohérentes"));
    }
    let grid_index = if settings.include_trees {
        2
    } else if settings.include_buildings {
        1
    } else {
        0
    };
    let grid_bytes = length
        .checked_mul(8)
        .ok_or_else(|| AppError::engine("grille de cache excessive"))?;
    cursor = cursor
        .checked_add(grid_index * grid_bytes)
        .ok_or_else(|| AppError::engine("position de grille excessive"))?;
    let mut heights = Vec::with_capacity(length);
    for _ in 0..length {
        heights.push(read_f64(&bytes, &mut cursor)?);
    }
    if heights.iter().any(|value| !value.is_finite()) {
        return Err(AppError::engine("altitude non finie dans le cache"));
    }
    let minimum_m = heights.iter().copied().fold(f64::INFINITY, f64::min);
    Ok(SurfaceCache {
        xmin,
        ymin,
        side_m,
        count,
        heights,
        minimum_m,
    })
}

/// Relit les grilles sans bâtiments et du sol pour obtenir la hauteur réelle de végétation.
fn read_canopy_surface(path: &Path) -> AppResult<CanopySurface> {
    let bytes = fs::read(path).map_err(|error| {
        AppError::engine(format!(
            "lecture du cache de canopée {} : {error}",
            path.display()
        ))
    })?;
    if bytes.get(..8) != Some(b"GSTLC04\0") {
        return Err(AppError::engine("signature du cache de surface inconnue"));
    }
    let mut cursor = 8;
    let header_size = read_u32(&bytes, &mut cursor)? as usize;
    let header_end = cursor
        .checked_add(header_size)
        .ok_or_else(|| AppError::engine("en-tête de cache excessif"))?;
    let header: Value = serde_json::from_slice(
        bytes
            .get(cursor..header_end)
            .ok_or_else(|| AppError::engine("en-tête de cache tronqué"))?,
    )
    .map_err(|error| AppError::engine(format!("en-tête de cache invalide : {error}")))?;
    cursor = header_end;
    let key = header
        .get("key")
        .ok_or_else(|| AppError::engine("clé absente du cache de surface"))?;
    let xmin = json_f64(key, "xmin")?;
    let ymin = json_f64(key, "ymin")?;
    let side_m = json_f64(key, "side_m")?;
    let count = json_u64(key, "count")? as usize;
    let length = read_u64(&bytes, &mut cursor)? as usize;
    if length != count.saturating_mul(count) || count < 2 {
        return Err(AppError::engine("dimensions de cache incohérentes"));
    }
    let grid_bytes = length
        .checked_mul(8)
        .ok_or_else(|| AppError::engine("grille de cache excessive"))?;
    let canopy_start = cursor
        .checked_add(grid_bytes)
        .ok_or_else(|| AppError::engine("position de canopée excessive"))?;
    let ground_start = canopy_start
        .checked_add(grid_bytes)
        .ok_or_else(|| AppError::engine("position de sol excessive"))?;
    let canopy = read_grid(&bytes, canopy_start, length)?;
    let ground = read_grid(&bytes, ground_start, length)?;
    if canopy.iter().chain(&ground).any(|value| !value.is_finite()) {
        return Err(AppError::engine("altitude non finie dans le cache"));
    }
    let minimum_m = ground.iter().copied().fold(f64::INFINITY, f64::min);
    Ok(CanopySurface {
        xmin,
        ymin,
        side_m,
        count,
        canopy,
        ground,
        minimum_m,
    })
}

/// Lit une grille f64 complète à une position connue du cache binaire.
fn read_grid(bytes: &[u8], start: usize, length: usize) -> AppResult<Vec<f64>> {
    let mut cursor = start;
    let mut values = Vec::with_capacity(length);
    for _ in 0..length {
        values.push(read_f64(bytes, &mut cursor)?);
    }
    Ok(values)
}

/// Produit puis ajoute la coque forestière en limitant volontairement sa résolution.
fn append_canopy_sync(
    output: &Path,
    data_root: &Path,
    cache_path: &Path,
    settings: &RepairSettings,
    request: &ExportRequest,
) -> AppResult<ForestCanopyReport> {
    let surface = read_canopy_surface(cache_path)?;
    let road_lines = if request.advanced.carve_forest_roads.unwrap_or(false) {
        let directory =
            find_road_directory_for_bounds(data_root, surface.xmin, surface.ymin, surface.side_m)?;
        read_road_lines(&directory)?.0
    } else {
        Vec::new()
    };
    let (triangles, patches, road_cells_removed) =
        build_canopy_triangles(&surface, settings, request, &road_lines);
    if triangles.is_empty() {
        return Err(AppError::engine(
            "aucune canopée LiDAR suffisamment haute dans cette emprise".to_owned(),
        ));
    }
    append_binary_stl(output, &triangles)?;
    Ok(ForestCanopyReport {
        patches,
        triangles: triangles.len(),
        road_cells_removed,
    })
}

/// Construit une coque par massif avec dessus, dessous et lisières verticales.
fn build_canopy_triangles(
    surface: &CanopySurface,
    settings: &RepairSettings,
    request: &ExportRequest,
    road_lines: &[RoadLine],
) -> (Vec<Triangle>, usize, usize) {
    const MINIMUM_VEGETATION_M: f64 = 2.0;
    let max_cells_per_axis = match request.advanced.forest_quality.unwrap_or_default() {
        ExportQuality::Draft => 140,
        ExportQuality::Standard => 240,
        ExportQuality::High => 360,
    };
    let stride = (surface.count - 1).div_ceil(max_cells_per_axis).max(1);
    let mut samples = (0..surface.count - 1).step_by(stride).collect::<Vec<_>>();
    samples.push(surface.count - 1);
    let sample_count = samples.len();
    let mut heights = vec![0.0; sample_count * sample_count];
    for (row, source_row) in samples.iter().copied().enumerate() {
        for (column, source_column) in samples.iter().copied().enumerate() {
            let source_index = source_row * surface.count + source_column;
            heights[row * sample_count + column] =
                (surface.canopy[source_index] - surface.ground[source_index]).clamp(0.0, 60.0);
        }
    }
    let smoothing = request.advanced.canopy_smoothing.unwrap_or(0.7);
    for _ in 0..(smoothing * 4.0).round() as usize {
        smooth_canopy_heights(&mut heights, sample_count);
    }
    let cell_count = sample_count - 1;
    let mut active = vec![false; cell_count * cell_count];
    for row in 0..cell_count {
        for column in 0..cell_count {
            let values = [
                heights[row * sample_count + column],
                heights[row * sample_count + column + 1],
                heights[(row + 1) * sample_count + column],
                heights[(row + 1) * sample_count + column + 1],
            ];
            let tall = values
                .iter()
                .filter(|height| **height >= MINIMUM_VEGETATION_M)
                .count();
            let average = values.iter().sum::<f64>() / 4.0;
            active[row * cell_count + column] = tall >= 2 && average >= MINIMUM_VEGETATION_M;
        }
    }
    for _ in 0..(smoothing * 3.0).round() as usize {
        fill_canopy_holes(&mut active, cell_count);
    }
    let road_cells_removed = if road_lines.is_empty() {
        0
    } else {
        carve_canopy_roads(&mut active, cell_count, surface, road_lines)
    };
    let patches = count_canopy_patches(&active, cell_count);
    let scale = settings.width_mm / surface.side_m;
    let height_scale = request.advanced.tree_height_scale.unwrap_or(1.3);
    let minimum_height_mm = request.advanced.tree_min_height_mm.unwrap_or(5.0);
    let mut triangles = Vec::new();
    for row in 0..cell_count {
        for column in 0..cell_count {
            if !active[row * cell_count + column] {
                continue;
            }
            let corners = [
                canopy_point(
                    surface,
                    settings,
                    &samples,
                    &heights,
                    sample_count,
                    column,
                    row,
                    scale,
                    height_scale,
                    minimum_height_mm,
                ),
                canopy_point(
                    surface,
                    settings,
                    &samples,
                    &heights,
                    sample_count,
                    column + 1,
                    row,
                    scale,
                    height_scale,
                    minimum_height_mm,
                ),
                canopy_point(
                    surface,
                    settings,
                    &samples,
                    &heights,
                    sample_count,
                    column,
                    row + 1,
                    scale,
                    height_scale,
                    minimum_height_mm,
                ),
                canopy_point(
                    surface,
                    settings,
                    &samples,
                    &heights,
                    sample_count,
                    column + 1,
                    row + 1,
                    scale,
                    height_scale,
                    minimum_height_mm,
                ),
            ];
            let top = [corners[0].0, corners[1].0, corners[2].0, corners[3].0];
            let bottom = [corners[0].1, corners[1].1, corners[2].1, corners[3].1];
            triangles.extend([
                [top[0], top[2], top[1]],
                [top[1], top[2], top[3]],
                [bottom[0], bottom[1], bottom[2]],
                [bottom[1], bottom[3], bottom[2]],
            ]);
            if row == 0 || !active[(row - 1) * cell_count + column] {
                append_canopy_wall(&mut triangles, top[0], top[1], bottom[0], bottom[1]);
            }
            if row + 1 == cell_count || !active[(row + 1) * cell_count + column] {
                append_canopy_wall(&mut triangles, top[3], top[2], bottom[3], bottom[2]);
            }
            if column == 0 || !active[row * cell_count + column - 1] {
                append_canopy_wall(&mut triangles, top[2], top[0], bottom[2], bottom[0]);
            }
            if column + 1 == cell_count || !active[row * cell_count + column + 1] {
                append_canopy_wall(&mut triangles, top[1], top[3], bottom[1], bottom[3]);
            }
        }
    }
    (triangles, patches, road_cells_removed)
}

/// Ferme les petits trous internes afin d'obtenir des massifs, pas une grille clairsemée.
fn fill_canopy_holes(active: &mut [bool], side: usize) {
    let original = active.to_vec();
    for row in 0..side {
        for column in 0..side {
            if original[row * side + column] {
                continue;
            }
            let mut neighbours = 0;
            for row_delta in -1isize..=1 {
                for column_delta in -1isize..=1 {
                    if row_delta == 0 && column_delta == 0 {
                        continue;
                    }
                    let candidate_row = row as isize + row_delta;
                    let candidate_column = column as isize + column_delta;
                    if candidate_row >= 0
                        && candidate_column >= 0
                        && candidate_row < side as isize
                        && candidate_column < side as isize
                        && original[candidate_row as usize * side + candidate_column as usize]
                    {
                        neighbours += 1;
                    }
                }
            }
            if neighbours >= 5 {
                active[row * side + column] = true;
            }
        }
    }
}

/// Désactive les cellules dont le centre recouvre une chaussée BD TOPO et sa marge visuelle.
fn carve_canopy_roads(
    active: &mut [bool],
    side: usize,
    surface: &CanopySurface,
    lines: &[RoadLine],
) -> usize {
    let before = active.iter().filter(|cell| **cell).count();
    let cell_size = surface.side_m / side as f64;
    let bounds = [
        surface.xmin,
        surface.ymin,
        surface.xmin + surface.side_m,
        surface.ymin + surface.side_m,
    ];
    for line in lines {
        let clearance = line.source_width_m.unwrap_or(6.0) / 2.0 + 2.0 + cell_size * 0.55;
        for segment in line.points.windows(2) {
            let Some((start, end)) = clip_segment(segment[0], segment[1], bounds) else {
                continue;
            };
            let minimum_column = (((start[0].min(end[0]) - clearance - surface.xmin) / cell_size)
                .floor() as isize)
                .clamp(0, side as isize - 1) as usize;
            let maximum_column = (((start[0].max(end[0]) + clearance - surface.xmin) / cell_size)
                .ceil() as isize)
                .clamp(0, side as isize - 1) as usize;
            let minimum_row = (((start[1].min(end[1]) - clearance - surface.ymin) / cell_size)
                .floor() as isize)
                .clamp(0, side as isize - 1) as usize;
            let maximum_row = (((start[1].max(end[1]) + clearance - surface.ymin) / cell_size)
                .ceil() as isize)
                .clamp(0, side as isize - 1) as usize;
            for row in minimum_row..=maximum_row {
                for column in minimum_column..=maximum_column {
                    let center = [
                        surface.xmin + (column as f64 + 0.5) * cell_size,
                        surface.ymin + (row as f64 + 0.5) * cell_size,
                    ];
                    if point_segment_distance(center, start, end) <= clearance {
                        active[row * side + column] = false;
                    }
                }
            }
        }
    }
    before.saturating_sub(active.iter().filter(|cell| **cell).count())
}

/// Calcule la distance métrique d'un point au segment routier le plus proche.
fn point_segment_distance(point: Point2, start: Point2, end: Point2) -> f64 {
    let delta = [end[0] - start[0], end[1] - start[1]];
    let length_squared = delta[0] * delta[0] + delta[1] * delta[1];
    if length_squared <= f64::EPSILON {
        return ((point[0] - start[0]).powi(2) + (point[1] - start[1]).powi(2)).sqrt();
    }
    let ratio = (((point[0] - start[0]) * delta[0] + (point[1] - start[1]) * delta[1])
        / length_squared)
        .clamp(0.0, 1.0);
    let nearest = [start[0] + delta[0] * ratio, start[1] + delta[1] * ratio];
    ((point[0] - nearest[0]).powi(2) + (point[1] - nearest[1]).powi(2)).sqrt()
}

/// Adoucit les écarts entre cellules LiDAR sans étendre la forêt hors des mesures présentes.
fn smooth_canopy_heights(heights: &mut [f64], side: usize) {
    let original = heights.to_vec();
    for row in 0..side {
        for column in 0..side {
            let index = row * side + column;
            if original[index] <= 0.0 {
                continue;
            }
            let mut total = original[index] * 2.0;
            let mut weight = 2.0;
            for row_delta in -1isize..=1 {
                for column_delta in -1isize..=1 {
                    if row_delta == 0 && column_delta == 0 {
                        continue;
                    }
                    let candidate_row = row as isize + row_delta;
                    let candidate_column = column as isize + column_delta;
                    if candidate_row < 0
                        || candidate_column < 0
                        || candidate_row >= side as isize
                        || candidate_column >= side as isize
                    {
                        continue;
                    }
                    let value = original[candidate_row as usize * side + candidate_column as usize];
                    if value > 0.0 {
                        total += value;
                        weight += 1.0;
                    }
                }
            }
            heights[index] = total / weight;
        }
    }
}

/// Compte les ensembles connexes pour fournir un retour lisible dans le journal serveur.
fn count_canopy_patches(active: &[bool], side: usize) -> usize {
    let mut visited = vec![false; active.len()];
    let mut patches = 0;
    for start in 0..active.len() {
        if !active[start] || visited[start] {
            continue;
        }
        patches += 1;
        visited[start] = true;
        let mut queue = VecDeque::from([start]);
        while let Some(index) = queue.pop_front() {
            let row = index / side;
            let column = index % side;
            let neighbours = [
                row.checked_sub(1).map(|value| value * side + column),
                (row + 1 < side).then_some((row + 1) * side + column),
                column.checked_sub(1).map(|value| row * side + value),
                (column + 1 < side).then_some(row * side + column + 1),
            ];
            for neighbour in neighbours.into_iter().flatten() {
                if active[neighbour] && !visited[neighbour] {
                    visited[neighbour] = true;
                    queue.push_back(neighbour);
                }
            }
        }
    }
    patches
}

/// Convertit un échantillon LiDAR en sommet haut et sommet posé sur le terrain imprimé.
#[allow(clippy::too_many_arguments)]
fn canopy_point(
    surface: &CanopySurface,
    settings: &RepairSettings,
    samples: &[usize],
    heights: &[f64],
    sample_count: usize,
    column: usize,
    row: usize,
    scale: f64,
    height_scale: f64,
    minimum_height_mm: f64,
) -> (Point3, Point3) {
    let source_column = samples[column];
    let source_row = samples[row];
    let source_index = source_row * surface.count + source_column;
    let raw_ground = surface.ground[source_index];
    let ground = settings.layer_height_m.map_or(raw_ground, |layer| {
        surface.minimum_m + ((raw_ground - surface.minimum_m) / layer).max(0.0).floor() * layer
    });
    let x = source_column as f64 / (surface.count - 1) as f64 * surface.side_m * scale;
    let y = source_row as f64 / (surface.count - 1) as f64 * surface.side_m * scale;
    let terrain_z =
        settings.base_mm + (ground - surface.minimum_m) * scale * settings.vertical_exaggeration;
    let lidar_height = heights[row * sample_count + column]
        * scale
        * settings.vertical_exaggeration
        * height_scale;
    let top_z = terrain_z + lidar_height.max(minimum_height_mm);
    // Une légère pénétration évite une simple surface tangente que certains trancheurs
    // interprètent comme deux volumes distincts.
    let bottom_z = (terrain_z - 0.35).max(0.25);
    (
        [x as f32, y as f32, top_z as f32],
        [x as f32, y as f32, bottom_z as f32],
    )
}

/// Ajoute les deux triangles d'une lisière entre sommet de canopée et terrain.
fn append_canopy_wall(
    triangles: &mut Vec<Triangle>,
    top_start: Point3,
    top_end: Point3,
    bottom_start: Point3,
    bottom_end: Point3,
) {
    triangles.extend([
        [top_start, bottom_start, top_end],
        [top_end, bottom_start, bottom_end],
    ]);
}

/// Trouve la plus petite extraction BD TOPO qui contient exactement la surface calculée.
fn find_road_directory(data_root: &Path, surface: &SurfaceCache) -> AppResult<PathBuf> {
    find_road_directory_for_bounds(data_root, surface.xmin, surface.ymin, surface.side_m)
}

/// Trouve l'extraction routière couvrant des bornes indépendamment du type de surface.
fn find_road_directory_for_bounds(
    data_root: &Path,
    xmin: f64,
    ymin: f64,
    side_m: f64,
) -> AppResult<PathBuf> {
    let root = data_root.join("BD_TOPO");
    let mut candidates = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let Some(bbox) = parse_bbox(&entry.file_name().to_string_lossy()) else {
            continue;
        };
        let xmax = xmin + side_m;
        let ymax = ymin + side_m;
        let tolerance = 0.05;
        if bbox[0] <= xmin + tolerance
            && bbox[1] <= ymin + tolerance
            && bbox[2] >= xmax - tolerance
            && bbox[3] >= ymax - tolerance
        {
            let directory = entry.path().join("troncon_de_route");
            if directory.is_dir() {
                candidates.push(((bbox[2] - bbox[0]) * (bbox[3] - bbox[1]), directory));
            }
        }
    }
    candidates.sort_by(|left, right| left.0.total_cmp(&right.0));
    candidates
        .into_iter()
        .next()
        .map(|(_, path)| path)
        .ok_or_else(|| AppError::engine("extraction BD TOPO des routes introuvable"))
}

/// Convertit le nom xmin_ymin_xmax_ymax d'une extraction en quatre coordonnées.
fn parse_bbox(name: &str) -> Option<[f64; 4]> {
    let values = name
        .split('_')
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    (values.len() == 4).then(|| [values[0], values[1], values[2], values[3]])
}

/// Charge les LineString et MultiLineString de toutes les pages routières du cache.
fn read_road_lines(directory: &Path) -> AppResult<(Vec<RoadLine>, usize)> {
    let mut pages = fs::read_dir(directory)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("page-") && name.ends_with(".geojson"))
        })
        .collect::<Vec<_>>();
    pages.sort();
    let mut lines = Vec::new();
    for page in &pages {
        let value: Value = serde_json::from_slice(&fs::read(page)?).map_err(|error| {
            AppError::engine(format!(
                "GeoJSON routier invalide {} : {error}",
                page.display()
            ))
        })?;
        for feature in value
            .get("features")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let width = feature
                .pointer("/properties/largeur_de_chaussee")
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite() && *value > 0.0);
            let Some(geometry) = feature.get("geometry") else {
                continue;
            };
            match geometry.get("type").and_then(Value::as_str) {
                Some("LineString") => push_line(&mut lines, geometry.get("coordinates"), width),
                Some("MultiLineString") => {
                    if let Some(parts) = geometry.get("coordinates").and_then(Value::as_array) {
                        for part in parts {
                            push_line(&mut lines, Some(part), width);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok((lines, pages.len()))
}

/// Convertit un tableau GeoJSON en axe 2D lorsque deux points valides sont présents.
fn push_line(lines: &mut Vec<RoadLine>, coordinates: Option<&Value>, width: Option<f64>) {
    let Some(points) = coordinates.and_then(Value::as_array) else {
        return;
    };
    let points = points
        .iter()
        .filter_map(|point| {
            let values = point.as_array()?;
            let x = values.first()?.as_f64()?;
            let y = values.get(1)?.as_f64()?;
            (x.is_finite() && y.is_finite()).then_some([x, y])
        })
        .collect::<Vec<_>>();
    if points.len() >= 2 {
        lines.push(RoadLine {
            points,
            source_width_m: width,
        });
    }
}

/// Transforme chaque segment découpé en petit prisme fermé qui peut chevaucher ses voisins.
fn build_road_triangles(
    surface: &SurfaceCache,
    lines: &[RoadLine],
    settings: &RepairSettings,
) -> (Vec<Triangle>, usize) {
    let mut triangles = Vec::new();
    let mut segment_count = 0;
    let scale = settings.width_mm / surface.side_m;
    let maximum_step = surface.side_m / (surface.count - 1) as f64 / 2.0;
    let bounds = [
        surface.xmin,
        surface.ymin,
        surface.xmin + surface.side_m,
        surface.ymin + surface.side_m,
    ];
    for line in lines {
        let width_mm = settings
            .road_min_width_mm
            .max(line.source_width_m.unwrap_or(0.0) * scale);
        for pair in line.points.windows(2) {
            let Some((start, end)) = clip_segment(pair[0], pair[1], bounds) else {
                continue;
            };
            let length = ((end[0] - start[0]).powi(2) + (end[1] - start[1]).powi(2)).sqrt();
            let divisions = (length / maximum_step.max(0.5)).ceil().max(1.0) as usize;
            for index in 0..divisions {
                let a = interpolate(start, end, index as f64 / divisions as f64);
                let b = interpolate(start, end, (index + 1) as f64 / divisions as f64);
                append_segment_prism(&mut triangles, surface, a, b, width_mm, settings);
                segment_count += 1;
            }
        }
    }
    (triangles, segment_count)
}

/// Ajoute les douze triangles d'un tronçon fermé, légèrement prolongé pour couvrir les joints.
fn append_segment_prism(
    triangles: &mut Vec<Triangle>,
    surface: &SurfaceCache,
    start: Point2,
    end: Point2,
    width_mm: f64,
    settings: &RepairSettings,
) {
    let scale = settings.width_mm / surface.side_m;
    let mut a = [
        (start[0] - surface.xmin) * scale,
        (start[1] - surface.ymin) * scale,
    ];
    let mut b = [
        (end[0] - surface.xmin) * scale,
        (end[1] - surface.ymin) * scale,
    ];
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let length = (dx * dx + dy * dy).sqrt();
    if length <= f64::EPSILON {
        return;
    }
    let direction = [dx / length, dy / length];
    let half = width_mm / 2.0;
    a[0] = (a[0] - direction[0] * half).clamp(0.0, settings.width_mm);
    a[1] = (a[1] - direction[1] * half).clamp(0.0, settings.width_mm);
    b[0] = (b[0] + direction[0] * half).clamp(0.0, settings.width_mm);
    b[1] = (b[1] + direction[1] * half).clamp(0.0, settings.width_mm);
    let normal = [-direction[1] * half, direction[0] * half];
    let xy = [
        [a[0] + normal[0], a[1] + normal[1]],
        [a[0] - normal[0], a[1] - normal[1]],
        [b[0] + normal[0], b[1] + normal[1]],
        [b[0] - normal[0], b[1] - normal[1]],
    ];
    let mut top = [[0.0; 3]; 4];
    let mut bottom = [[0.0; 3]; 4];
    for (index, point) in xy.into_iter().enumerate() {
        let local = [point[0] / scale, point[1] / scale];
        let height = printable_height(surface, local, settings.layer_height_m);
        let terrain_mm = settings.base_mm
            + (height - surface.minimum_m) * scale * settings.vertical_exaggeration;
        top[index] = [
            point[0] as f32,
            point[1] as f32,
            (terrain_mm + settings.road_height_mm) as f32,
        ];
        bottom[index] = [
            point[0] as f32,
            point[1] as f32,
            (terrain_mm - settings.road_height_mm * 0.5) as f32,
        ];
    }
    triangles.extend([
        [top[0], top[1], top[3]],
        [top[0], top[3], top[2]],
        [bottom[0], bottom[3], bottom[1]],
        [bottom[0], bottom[2], bottom[3]],
        [top[0], bottom[0], top[2]],
        [bottom[0], bottom[2], top[2]],
        [top[1], top[3], bottom[1]],
        [bottom[1], top[3], bottom[3]],
        [top[0], top[1], bottom[0]],
        [bottom[0], top[1], bottom[1]],
        [top[2], bottom[2], top[3]],
        [bottom[2], bottom[3], top[3]],
    ]);
}

/// Reproduit l'interpolation triangulée de la grille historique puis les paliers éventuels.
fn printable_height(surface: &SurfaceCache, point: Point2, layer_height_m: Option<f64>) -> f64 {
    let step = surface.side_m / (surface.count - 1) as f64;
    let u = (point[0] / step).clamp(0.0, (surface.count - 1) as f64);
    let v = (point[1] / step).clamp(0.0, (surface.count - 1) as f64);
    let col = (u.floor() as usize).min(surface.count - 2);
    let row = (v.floor() as usize).min(surface.count - 2);
    let a = u - col as f64;
    let b = v - row as f64;
    let sw = surface.heights[row * surface.count + col];
    let se = surface.heights[row * surface.count + col + 1];
    let nw = surface.heights[(row + 1) * surface.count + col];
    let ne = surface.heights[(row + 1) * surface.count + col + 1];
    let height = if b <= a {
        sw * (1.0 - a) + se * (a - b) + ne * b
    } else {
        sw * (1.0 - b) + ne * a + nw * (b - a)
    };
    layer_height_m.map_or(height, |layer| {
        surface.minimum_m + ((height - surface.minimum_m) / layer).max(0.0).floor() * layer
    })
}

/// Découpe un segment au carré d'export avec l'algorithme de Liang-Barsky.
fn clip_segment(start: Point2, end: Point2, bounds: [f64; 4]) -> Option<(Point2, Point2)> {
    let delta = [end[0] - start[0], end[1] - start[1]];
    let p = [-delta[0], delta[0], -delta[1], delta[1]];
    let q = [
        start[0] - bounds[0],
        bounds[2] - start[0],
        start[1] - bounds[1],
        bounds[3] - start[1],
    ];
    let mut lower: f64 = 0.0;
    let mut upper: f64 = 1.0;
    for index in 0..4 {
        if p[index].abs() <= f64::EPSILON {
            if q[index] < 0.0 {
                return None;
            }
            continue;
        }
        let ratio = q[index] / p[index];
        if p[index] < 0.0 {
            lower = lower.max(ratio);
        } else {
            upper = upper.min(ratio);
        }
        if lower > upper {
            return None;
        }
    }
    Some((
        interpolate(start, end, lower),
        interpolate(start, end, upper),
    ))
}

/// Interpole un point 2D entre deux extrémités.
fn interpolate(start: Point2, end: Point2, ratio: f64) -> Point2 {
    [
        start[0] + (end[0] - start[0]) * ratio,
        start[1] + (end[1] - start[1]) * ratio,
    ]
}

/// Ajoute des triangles à un STL binaire et met son compteur à jour.
fn append_binary_stl(path: &Path, triangles: &[Triangle]) -> AppResult<()> {
    let mut bytes = fs::read(path)?;
    if bytes.len() < 84 {
        return Err(AppError::engine("STL de base trop court"));
    }
    let old_count = u32::from_le_bytes(bytes[80..84].try_into().expect("quatre octets"));
    let expected = 84usize
        .checked_add(old_count as usize * 50)
        .ok_or_else(|| AppError::engine("taille STL excessive"))?;
    if bytes.len() != expected {
        return Err(AppError::engine("taille du STL binaire incohérente"));
    }
    let added = u32::try_from(triangles.len())
        .map_err(|_| AppError::engine("trop de triangles routiers"))?;
    let total = old_count
        .checked_add(added)
        .ok_or_else(|| AppError::engine("trop de triangles dans le STL final"))?;
    bytes[80..84].copy_from_slice(&total.to_le_bytes());
    for triangle in triangles {
        write_triangle_bytes(&mut bytes, *triangle);
    }
    fs::write(path, bytes)?;
    Ok(())
}

/// Sérialise un triangle STL avec sa normale calculée et un attribut nul.
fn write_triangle_bytes(bytes: &mut Vec<u8>, triangle: Triangle) {
    let normal = triangle_normal(triangle);
    for value in normal.into_iter().chain(triangle.into_iter().flatten()) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&0u16.to_le_bytes());
}

/// Calcule une normale unitaire, ou nulle pour un triangle dégénéré ignoré par les slicers.
fn triangle_normal(triangle: Triangle) -> Point3 {
    let u = [
        triangle[1][0] - triangle[0][0],
        triangle[1][1] - triangle[0][1],
        triangle[1][2] - triangle[0][2],
    ];
    let v = [
        triangle[2][0] - triangle[0][0],
        triangle[2][1] - triangle[0][1],
        triangle[2][2] - triangle[0][2],
    ];
    let normal = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
    if length <= f32::EPSILON {
        [0.0, 0.0, 0.0]
    } else {
        [normal[0] / length, normal[1] / length, normal[2] / length]
    }
}

/// Lit un entier u32 little-endian à la position courante.
fn read_u32(bytes: &[u8], cursor: &mut usize) -> AppResult<u32> {
    let end = cursor
        .checked_add(4)
        .ok_or_else(|| AppError::engine("position de lecture excessive"))?;
    let value = bytes
        .get(*cursor..end)
        .ok_or_else(|| AppError::engine("cache de surface tronqué"))?;
    *cursor = end;
    Ok(u32::from_le_bytes(value.try_into().expect("quatre octets")))
}

/// Lit un entier u64 little-endian à la position courante.
fn read_u64(bytes: &[u8], cursor: &mut usize) -> AppResult<u64> {
    let end = cursor
        .checked_add(8)
        .ok_or_else(|| AppError::engine("position de lecture excessive"))?;
    let value = bytes
        .get(*cursor..end)
        .ok_or_else(|| AppError::engine("cache de surface tronqué"))?;
    *cursor = end;
    Ok(u64::from_le_bytes(value.try_into().expect("huit octets")))
}

/// Lit un flottant f64 little-endian à la position courante.
fn read_f64(bytes: &[u8], cursor: &mut usize) -> AppResult<f64> {
    let end = cursor
        .checked_add(8)
        .ok_or_else(|| AppError::engine("position de lecture excessive"))?;
    let value = bytes
        .get(*cursor..end)
        .ok_or_else(|| AppError::engine("cache de surface tronqué"))?;
    *cursor = end;
    Ok(f64::from_le_bytes(value.try_into().expect("huit octets")))
}

/// Extrait un nombre décimal obligatoire d'un objet JSON.
fn json_f64(value: &Value, name: &str) -> AppResult<f64> {
    value
        .get(name)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .ok_or_else(|| AppError::engine(format!("champ {name} invalide dans le cache")))
}

/// Extrait un entier positif obligatoire d'un objet JSON.
fn json_u64(value: &Value, name: &str) -> AppResult<u64> {
    value
        .get(name)
        .and_then(Value::as_u64)
        .ok_or_else(|| AppError::engine(format!("champ {name} invalide dans le cache")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vérifie le découpage d'un axe qui traverse entièrement le carré.
    #[test]
    fn clipping_keeps_only_the_part_inside_the_area() {
        let (start, end) = clip_segment([-5.0, 5.0], [15.0, 5.0], [0.0, 0.0, 10.0, 10.0])
            .expect("segment visible");
        assert_eq!(start, [0.0, 5.0]);
        assert_eq!(end, [10.0, 5.0]);
    }

    /// Vérifie l'interpolation triangulée et la quantification des strates.
    #[test]
    fn layered_height_uses_the_cached_grid() {
        let surface = SurfaceCache {
            xmin: 0.0,
            ymin: 0.0,
            side_m: 10.0,
            count: 2,
            heights: vec![100.0, 102.0, 104.0, 106.0],
            minimum_m: 100.0,
        };
        assert_eq!(printable_height(&surface, [5.0, 5.0], Some(2.0)), 102.0);
    }

    /// Une route traversante doit ouvrir un corridor continu dans la canopée échantillonnée.
    #[test]
    fn road_axis_removes_canopy_cells() {
        let surface = CanopySurface {
            xmin: 0.0,
            ymin: 0.0,
            side_m: 100.0,
            count: 2,
            canopy: vec![0.0; 4],
            ground: vec![0.0; 4],
            minimum_m: 0.0,
        };
        let roads = vec![RoadLine {
            points: vec![[0.0, 50.0], [100.0, 50.0]],
            source_width_m: Some(6.0),
        }];
        let mut active = vec![true; 100];

        let removed = carve_canopy_roads(&mut active, 10, &surface, &roads);

        assert!(removed >= 10);
        assert!(!active[5 * 10 + 5]);
    }
}
