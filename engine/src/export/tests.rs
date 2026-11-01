//! Contexte : protéger la géoréférence, les surfaces, les options et l'intégrité du cache.
//! Usage : cargo test --lib export ; les fichiers sont synthétiques, sans réseau ni Blender.
//! Une vérification Blender séparée rouvre aussi le vrai .blend après intégration des images.

use super::models::*;
use super::*;
use crate::test_support as support;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn config() -> ExportConfig {
    super::config::parse("").unwrap()
}

/// Écrit un véritable LAZ compressé minuscule et, si demandé, son reçu de transfert.
pub(super) fn write_laz(
    root: &Path,
    edition: &str,
    points: &[(f64, f64, f64, u8, bool)],
    receipt: bool,
) -> PathBuf {
    let folder = root.join("LIDAR_HD").join(edition);
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("LHD_FXX_test_LAMB93.copc.laz");
    let mut builder = las::Builder::from((1, 2));
    builder.point_format = las::point::Format::new(0).unwrap();
    builder.point_format.is_compressed = true;
    let mut writer = las::Writer::from_path(&path, builder.into_header().unwrap()).unwrap();
    for &(x, y, z, class, withheld) in points {
        writer
            .write_point(las::Point {
                x,
                y,
                z,
                classification: las::point::Classification::new(class).unwrap(),
                is_withheld: withheld,
                ..Default::default()
            })
            .unwrap();
    }
    writer.close().unwrap();
    drop(writer);
    if receipt {
        let size = std::fs::metadata(&path).unwrap().len();
        std::fs::write(
            path.with_extension("laz.download.json"),
            json!({"url":"https://data.geopf.fr/test.laz","size":size}).to_string(),
        )
        .unwrap();
    }
    path
}

#[test]
fn compressed_lidar_ground_and_vegetation_classes_are_kept_separately() {
    let root = support::directory();
    write_laz(
        &root,
        "edition_2026-01-01",
        &[(10., 10., 100., 2, false), (11., 11., 115., 5, false)],
        true,
    );
    let mut c = config();
    c.area.side_m = 20.;
    c.lidar.display_stride = 1;
    let samples = lidar::read(&root, &c, 0., 0., 3).unwrap();
    assert_eq!(samples.ground[4], Some(100.));
    assert_eq!(samples.canopy.len(), 1);
    assert_eq!(samples.canopy.values().next().unwrap()[2], 115.);
    assert_eq!(samples.display.len(), 2);
    assert_eq!(samples.report.points_read, 2);
    assert_eq!(samples.report.classes.get(&5), Some(&1));
    support::cleanup(&root);
}

#[test]
fn tree_limit_is_applied_after_height_filtering_so_real_trees_are_not_lost() {
    let mut c = config();
    c.vegetation.max_trees = 1;
    let mut samples = roof_samples();
    samples.canopy.clear();
    samples.canopy.insert((0, 0), [2., 2., 101.]); // végétation trop basse
    samples.canopy.insert((1, 0), [12., 2., 115.]); // arbre admissible
    let trees = vegetation::prepare(&grid(), &mut samples, &c).unwrap();
    assert_eq!(trees.len(), 1);
    assert_eq!(trees[0].height, 15.);
    assert_eq!(samples.report.trees_filtered_height, 1);
    assert_eq!(samples.report.trees_exported, 1);
}

#[test]
fn partial_lidar_is_used_for_trees_even_when_alti_supplies_the_ground() {
    let mut c = config();
    let mut samples = roof_samples();
    samples.canopy.clear();
    samples.canopy.insert((1, 1), [12., 12., 115.]);
    samples.report.coverage_fraction = 0.1;
    c.lidar.max_display_points = 0;
    c.layers.get_mut("lidar_points").unwrap().enabled = false;
    let t = grid();
    assert_eq!(t.source, "test");
    let trees = vegetation::prepare(&t, &mut samples, &c).unwrap();
    assert_eq!(trees.len(), 1);
    assert_eq!(trees[0].position, [2., 2., 0.]);
    assert_eq!(trees[0].height, 15.);
}

#[test]
fn no_tree_after_filtering_is_reported_and_a_disabled_layer_stays_empty() {
    let mut c = config();
    let mut samples = roof_samples();
    samples.canopy.clear();
    samples.canopy.insert((0, 0), [2., 2., 101.]);
    assert!(
        vegetation::prepare(&grid(), &mut samples, &c)
            .unwrap()
            .is_empty()
    );
    assert_eq!(samples.report.trees_filtered_height, 1);
    c.layers.get_mut("vegetation").unwrap().enabled = false;
    samples.canopy.insert((1, 0), [12., 2., 115.]);
    assert!(
        vegetation::prepare(&grid(), &mut samples, &c)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn lidar_mode_builds_terrain_from_ground_and_limits_interpolation() {
    let root = support::directory();
    write_laz(
        &root,
        "edition_2026-01-01",
        &[(10., 10., 100., 2, false)],
        true,
    );
    let mut c = config();
    c.area.center = [10., 10.];
    c.area.crs = "EPSG:2154".into();
    c.area.side_m = 20.;
    c.terrain.grid_step_m = 10.;
    c.terrain.source = "lidar".into();
    c.terrain.lidar_fill_radius_cells = 1;
    let (terrain, _, _) = terrain::prepare(&root, &c).unwrap();
    assert!(terrain.heights.iter().all(|z| *z == 100.));
    assert_eq!(terrain.source, "LiDAR sol");
    c.area.side_m = 100.;
    assert!(terrain::prepare(&root, &c).is_err());
    support::cleanup(&root);
}

#[test]
fn lidar_files_without_receipt_are_ignored() {
    let root = support::directory();
    write_laz(
        &root,
        "edition_2026-01-01",
        &[(10., 10., 100., 2, false)],
        false,
    );
    let mut c = config();
    c.area.side_m = 20.;
    let samples = lidar::read(&root, &c, 0., 0., 3).unwrap();
    assert!(samples.files.is_empty());
    assert!(samples.ground.iter().all(Option::is_none));
    support::cleanup(&root);
}

#[test]
fn withheld_lidar_points_do_not_create_terrain() {
    let root = support::directory();
    write_laz(
        &root,
        "edition_2026-01-01",
        &[(10., 10., 100., 2, true)],
        true,
    );
    let mut c = config();
    c.area.side_m = 20.;
    let samples = lidar::read(&root, &c, 0., 0., 3).unwrap();
    assert!(samples.ground.iter().all(Option::is_none));
    support::cleanup(&root);
}

#[test]
fn newest_lidar_edition_wins_even_when_product_prefix_differs() {
    let root = support::directory();
    write_laz(
        &root,
        "NUALHD_NG_2025-03-12",
        &[(10., 10., 100., 2, false)],
        true,
    );
    write_laz(
        &root,
        "NUALHD_NF_2026-02-03",
        &[(10., 10., 200., 2, false)],
        true,
    );
    let mut c = config();
    c.area.side_m = 20.;
    let samples = lidar::read(&root, &c, 0., 0., 3).unwrap();
    assert_eq!(samples.files.len(), 1);
    assert_eq!(samples.ground[4], Some(200.));
    support::cleanup(&root);
}

#[test]
fn lidar_outside_the_area_is_not_used() {
    let root = support::directory();
    write_laz(
        &root,
        "edition_2026-01-01",
        &[(100., 100., 100., 2, false)],
        true,
    );
    let mut c = config();
    c.area.side_m = 20.;
    let samples = lidar::read(&root, &c, 0., 0., 3).unwrap();
    assert!(samples.files.is_empty());
    support::cleanup(&root);
}

#[test]
fn lidar_display_is_bounded_and_deterministic() {
    let root = support::directory();
    let points: Vec<_> = (0..100)
        .map(|i| (i as f64 / 10., 10., 100., 2, false))
        .collect();
    write_laz(&root, "edition_2026-01-01", &points, true);
    let mut c = config();
    c.area.side_m = 20.;
    c.lidar.display_stride = 1;
    c.lidar.max_display_points = 3;
    let first = lidar::read(&root, &c, 0., 0., 3).unwrap();
    let second = lidar::read(&root, &c, 0., 0., 3).unwrap();
    assert_eq!(first.display.len(), 3);
    assert_eq!(first.display, second.display);
    support::cleanup(&root);
}
fn grid() -> TerrainGrid {
    TerrainGrid {
        xmin: 0.,
        ymin: 0.,
        side: 20.,
        count: 3,
        heights: vec![100.; 9],
        origin: [10., 10., 100.],
        exaggeration: 1.,
        source: "test".into(),
    }
}
fn raster(root: &Path) -> PathBuf {
    let path = root.join("RGE_ALTI/grid/0");
    std::fs::create_dir_all(path.join("0")).unwrap();
    let metadata = json!({"schema_version":2,"crs":"EPSG:2154","format":"image/x-bil;bits=32","coordinate_order":"x_y",
        "matrix":{"matrix_set":"grid","crs":"EPSG:2154","identifier":"0","scale_denominator":1.,
        "resolution_units_per_px":1.,"nominal_resolution_m_per_px":1.,"top_left_x":700000.,"top_left_y":6600004.,
        "tile_width":2,"tile_height":2,"matrix_width":2,"matrix_height":1,
        "limits":{"col_min":0,"col_max":1,"row_min":0,"row_max":0}}});
    std::fs::write(path.join("matrix.json"), metadata.to_string()).unwrap();
    for (col, values) in [(0, [1f32, 2., 3., 4.]), (1, [5f32, 6., 7., 8.])] {
        std::fs::write(
            path.join("0").join(format!("{col}.bil")),
            values
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    }
    path.join("matrix.json")
}
fn feature(geometry: Value) -> Value {
    json!({"type":"Feature","id":"test.1","geometry":geometry,"properties":{"hauteur":8.0}})
}
fn square() -> Value {
    json!({"type":"Polygon","coordinates":[[[2,2],[18,2],[18,18],[2,18],[2,2]],[[8,8],[8,12],[12,12],[12,8],[8,8]]]})
}

#[test]
fn complete_defaults_are_valid_and_every_layer_is_enabled() {
    let c = config();
    assert_eq!(c.layers.len(), 25);
    assert_eq!(c.camera.clip_end_m, 10_000.0);
    assert_eq!(c.view.clip_end_m, 10_000.0);
    assert_eq!(c.water.thickness_m, 1.0);
    assert!(c.layers.values().all(|l| l.enabled));
    assert!(!c.layers["cadastre.parcelle"].visible);
    assert!(
        c.layers
            .iter()
            .all(|(key, l)| key == "cadastre.parcelle" || l.visible)
    );
}
#[test]
fn partial_configuration_preserves_unspecified_defaults() {
    let c = super::config::parse(
        "[area]\nside_m = 500\n[layers.\"cadastre.parcelle\"]\nenabled = false",
    )
    .unwrap();
    assert_eq!(c.area.side_m, 500.);
    assert!(!c.layers["cadastre.parcelle"].enabled);
    assert!(c.layers["terrain"].enabled);
    assert_eq!(c.ortho.interpolation, "nearest");
}
#[test]
fn misspelled_parameter_is_rejected() {
    assert!(super::config::parse("[terrain]\nstep = 5").is_err());
}
#[test]
fn unknown_layer_is_rejected() {
    assert!(super::config::parse("[layers.invented]\nenabled = true").is_err());
}
#[test]
fn invalid_parameter_type_is_rejected() {
    assert!(super::config::parse("[terrain]\ngrid_step_m = \"five\"").is_err());
}
#[test]
fn invalid_sources_and_negative_step_are_rejected() {
    for text in [
        "[terrain]\nsource = \"flat\"",
        "[terrain]\ngrid_step_m = -1",
        "[area]\nside_m = 0",
    ] {
        assert!(super::config::parse(text).is_err());
    }
}
#[test]
fn camera_cannot_be_below_the_terrain() {
    assert!(super::config::parse("[camera]\nelevation_deg = -20").is_err());
}
#[test]
fn output_must_be_a_blend_file() {
    assert!(super::config::parse("[paths]\noutput_file = \"scene.obj\"").is_err());
}
#[test]
fn relative_paths_are_resolved_from_configuration_directory() {
    assert_eq!(
        super::config::resolve(Path::new("projet/config"), "cache"),
        PathBuf::from("projet/config/cache")
    );
}
#[test]
fn local_origin_keeps_geographic_precision_and_vertical_exaggeration() {
    let mut t = grid();
    t.exaggeration = 2.;
    assert_eq!(t.local(11., 12., 103.), [1., 2., 6.]);
}
#[test]
fn terrain_height_interpolates_and_includes_the_northern_edge() {
    let mut t = grid();
    t.heights = vec![0., 10., 20., 20., 30., 40., 40., 50., 60.];
    assert_eq!(t.height(5., 5.).unwrap(), 15.);
    assert_eq!(t.height(20., 20.).unwrap(), 60.);
}
#[test]
fn projection_keeps_xy_order_for_geographic_rasters() {
    let p = Projection::new("EPSG:2154", "EPSG:4326").unwrap();
    let (x, y) = p.point(700000., 6600000.).unwrap();
    assert!((x - 3.).abs() < 1e-8);
    assert!((y - 46.5).abs() < 1e-8);
}
#[test]
fn bil_pixel_center_and_north_south_order_are_correct() {
    let root = support::directory();
    let path = raster(&root);
    let mut c = RasterCache::open(&path, 4).unwrap();
    assert_eq!(c.sample(700000.5, 6600003.5, false).unwrap()[0], 1.);
    assert_eq!(c.sample(700000.5, 6600002.5, false).unwrap()[0], 3.);
    support::cleanup(&root);
}
#[test]
fn bilinear_sampling_crosses_tile_boundaries() {
    let root = support::directory();
    let path = raster(&root);
    let mut c = RasterCache::open(&path, 4).unwrap();
    assert_eq!(c.sample(700002., 6600003.5, true).unwrap()[0], 3.5);
    support::cleanup(&root);
}
#[test]
fn missing_and_truncated_bil_are_not_replaced_by_zero() {
    let root = support::directory();
    let path = raster(&root);
    std::fs::write(path.parent().unwrap().join("0/0.bil"), [1, 2]).unwrap();
    let mut c = RasterCache::open(&path, 4).unwrap();
    assert!(c.sample(700000.5, 6600003.5, false).is_err());
    std::fs::remove_file(path.parent().unwrap().join("0/1.bil")).unwrap();
    assert!(c.sample(700002.5, 6600003.5, false).is_err());
    support::cleanup(&root);
}
#[test]
fn explicit_no_data_bil_is_flat_at_sea_level() {
    let root = support::directory();
    let path = raster(&root);
    let tile = path.parent().unwrap().join("0/0.bil");
    std::fs::remove_file(&tile).unwrap();
    std::fs::write(
        crate::cache::no_data_marker_path(&tile),
        b"wmts-no-data-v1\n",
    )
    .unwrap();
    let mut c = RasterCache::open(&path, 4).unwrap();
    assert_eq!(c.sample(700000.5, 6600003.5, false).unwrap()[0], 0.);
    support::cleanup(&root);
}
#[test]
fn old_raster_schema_is_rejected() {
    let root = support::directory();
    let path = raster(&root);
    let mut value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["schema_version"] = json!(1);
    std::fs::write(&path, value.to_string()).unwrap();
    assert!(RasterCache::open(&path, 4).is_err());
    support::cleanup(&root);
}
#[test]
fn incomplete_terrain_cache_is_an_error() {
    let root = support::directory();
    let mut c = config();
    c.terrain.source = "alti".into();
    assert!(terrain::prepare(&root, &c).is_err());
    support::cleanup(&root);
}
#[test]
fn excessive_terrain_density_is_rejected_before_allocation() {
    let root = support::directory();
    let mut c = config();
    c.terrain.grid_step_m = 0.001;
    assert!(
        terrain::prepare(&root, &c)
            .unwrap_err()
            .to_string()
            .contains("dense")
    );
    support::cleanup(&root);
}
#[test]
fn triangulation_preserves_polygon_holes() {
    let c = config();
    let out = geometry::prepare(
        &feature(square()),
        "bd_topage.PlanEau",
        &c.layers["bd_topage.PlanEau"],
        &grid(),
        &c,
    )
    .unwrap()
    .unwrap();
    let mut area = 0.;
    for face in out.faces {
        let a = out.vertices[face[0]];
        let b = out.vertices[face[1]];
        let d = out.vertices[face[2]];
        area += ((b[0] - a[0]) * (d[1] - a[1]) - (b[1] - a[1]) * (d[0] - a[0])).abs() / 2.;
    }
    assert!((area - 240.).abs() < 1e-6);
}
#[test]
fn cadastral_polygons_are_outlines_including_inner_rings() {
    let c = config();
    let out = geometry::prepare(
        &feature(square()),
        "cadastre.parcelle",
        &c.layers["cadastre.parcelle"],
        &grid(),
        &c,
    )
    .unwrap()
    .unwrap();
    assert!(out.faces.is_empty());
    assert_eq!(out.lines.len(), 2);
}
#[test]
fn polygons_outside_the_export_area_are_omitted() {
    let c = config();
    let f = feature(
        json!({"type":"Polygon","coordinates":[[[100,100],[110,100],[110,110],[100,100]]]}),
    );
    assert!(
        geometry::prepare(
            &f,
            "bd_topage.PlanEau",
            &c.layers["bd_topage.PlanEau"],
            &grid(),
            &c
        )
        .unwrap()
        .is_none()
    );
}
#[test]
fn crossing_lines_are_clipped_and_draped_on_terrain() {
    let c = config();
    let f = feature(json!({"type":"LineString","coordinates":[[-10,10],[30,10]]}));
    let out = geometry::prepare(
        &f,
        "bd_topo.troncon_de_route",
        &c.layers["bd_topo.troncon_de_route"],
        &grid(),
        &c,
    )
    .unwrap()
    .unwrap();
    assert_eq!(out.lines[0].first().unwrap()[0], -10.);
    assert_eq!(out.lines[0].last().unwrap()[0], 10.);
    assert!(out.lines[0].iter().all(|p| (p[2] - 0.2).abs() < 1e-8));
}
#[test]
fn buildings_use_the_source_height_and_keep_attributes() {
    let c = config();
    let out = geometry::prepare(
        &feature(square()),
        "bd_topo.batiment",
        &c.layers["bd_topo.batiment"],
        &grid(),
        &c,
    )
    .unwrap()
    .unwrap();
    assert!(out.vertices.iter().any(|p| (p[2] - 8.2).abs() < 1e-8));
    assert_eq!(out.properties["hauteur"], 8.);
}
#[test]
fn missing_building_height_uses_the_configured_fallback() {
    let c = config();
    let mut f = feature(square());
    f["properties"] = json!({});
    let out = geometry::prepare(
        &f,
        "bd_topo.batiment",
        &c.layers["bd_topo.batiment"],
        &grid(),
        &c,
    )
    .unwrap()
    .unwrap();
    assert!(out.vertices.iter().any(|p| (p[2] - 7.2).abs() < 1e-8));
}
#[test]
fn missing_vector_layers_are_reported_or_rejected_according_to_configuration() {
    let root = support::directory();
    let mut c = config();
    for (name, style) in &mut c.layers {
        style.enabled = name == "cadastre.parcelle";
    }
    let mut warnings = Vec::new();
    let layers = vectors::prepare(&root, &c, &grid(), None, &mut warnings).unwrap();
    assert!(layers.is_empty());
    assert_eq!(warnings.len(), 1);
    c.vectors.missing_layers = "error".into();
    assert!(vectors::prepare(&root, &c, &grid(), None, &mut warnings).is_err());
    support::cleanup(&root);
}

/// Mesures synthétiques d'un toit incliné : quatre points par mètre carré.
fn roof_samples() -> LidarSamples {
    let mut samples = LidarSamples {
        report: Default::default(),
        ground: vec![None; 9],
        canopy: Default::default(),
        roofs: Default::default(),
        display: Vec::new(),
        files: Vec::new(),
    };
    for x in 2..18 {
        for y in 2..18 {
            for (dx, dy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                let px = x as f64 + dx;
                let py = y as f64 + dy;
                samples
                    .roofs
                    .entry((x, y))
                    .or_default()
                    .add([px, py, 108. + 0.1 * px + 0.05 * py]);
            }
        }
    }
    samples
}

fn building(c: &ExportConfig) -> ObjectPlan {
    geometry::prepare(
        &feature(square()),
        "bd_topo.batiment",
        &c.layers["bd_topo.batiment"],
        &grid(),
        c,
    )
    .unwrap()
    .unwrap()
}

#[test]
fn dense_lidar_replaces_flat_roof_but_preserves_courtyard() {
    let c = config();
    let mut out = building(&c);
    let original = out.roof_faces.len();
    buildings::improve(&feature(square()), &mut out, &grid(), &c, &roof_samples()).unwrap();
    assert!(out.roof_faces.len() > original);
    assert_eq!(
        out.properties["export_toit"][0]["source"],
        "LiDAR HD classe 6"
    );
    let mut area = 0.;
    let mut zmin = f64::INFINITY;
    let mut zmax = f64::NEG_INFINITY;
    for i in &out.roof_faces {
        let face = &out.faces[*i];
        let a = out.vertices[face[0]];
        let b = out.vertices[face[1]];
        let d = out.vertices[face[2]];
        area += ((b[0] - a[0]) * (d[1] - a[1]) - (b[1] - a[1]) * (d[0] - a[0])).abs() / 2.;
        for index in face {
            zmin = zmin.min(out.vertices[*index][2]);
            zmax = zmax.max(out.vertices[*index][2]);
        }
    }
    assert!((area - 240.).abs() < 1e-6);
    assert!(zmax - zmin > 1.);
}

#[test]
fn sparse_lidar_keeps_the_complete_original_building() {
    let c = config();
    let mut out = building(&c);
    let original = out.vertices.clone();
    let mut samples = roof_samples();
    samples.roofs.retain(|key, _| *key == (2, 2));
    buildings::improve(&feature(square()), &mut out, &grid(), &c, &samples).unwrap();
    assert_eq!(out.vertices, original);
    assert_eq!(
        out.properties["export_toit"]["source"],
        "BD TOPO / hauteur de secours"
    );
}

#[test]
fn lidar_is_not_accepted_only_because_coverage_is_good() {
    let mut c = config();
    c.buildings.min_density_per_m2 = 20.;
    let mut out = building(&c);
    let original = out.vertices.clone();
    buildings::improve(&feature(square()), &mut out, &grid(), &c, &roof_samples()).unwrap();
    assert_eq!(out.vertices, original);
}

#[test]
fn incoherent_lidar_cells_do_not_raise_the_roof() {
    let c = config();
    let mut samples = roof_samples();
    for cell in samples.roofs.values_mut() {
        cell.add([cell.mean()[0], cell.mean()[1], 500.]);
    }
    let mut out = building(&c);
    let original = out.vertices.clone();
    buildings::improve(&feature(square()), &mut out, &grid(), &c, &samples).unwrap();
    assert_eq!(out.vertices, original);
}

#[test]
fn explicitly_required_lidar_reports_insufficient_building_data() {
    let mut c = config();
    c.buildings.source = "lidar".into();
    let mut samples = roof_samples();
    samples.roofs.clear();
    let mut out = building(&c);
    assert!(buildings::improve(&feature(square()), &mut out, &grid(), &c, &samples).is_err());
}

#[test]
fn forcing_bd_topo_keeps_original_geometry_even_with_dense_lidar() {
    let mut c = config();
    c.buildings.source = "bd_topo".into();
    let mut out = building(&c);
    let original = out.vertices.clone();
    buildings::improve(&feature(square()), &mut out, &grid(), &c, &roof_samples()).unwrap();
    assert_eq!(out.vertices, original);
}

#[test]
fn roof_interpolation_does_not_cross_large_unmeasured_gaps() {
    let mut cells = std::collections::BTreeMap::new();
    cells.insert((0, 0), [0.5, 0.5, 110.]);
    let roof = RoofSurface {
        cells,
        cell_size: 1.,
        radius: 2,
    };
    assert_eq!(roof.height(0.5, 0.5), Some(110.));
    assert!(roof.height(10., 10.).is_none());
}

#[test]
fn class_six_building_measurements_are_independent_of_display_stride() {
    let root = support::directory();
    write_laz(
        &root,
        "edition_2026-01-01",
        &[(10., 10., 110., 6, false), (10.1, 10.1, 112., 6, false)],
        true,
    );
    let mut c = config();
    c.area.side_m = 20.;
    c.layers.get_mut("lidar_points").unwrap().enabled = false;
    c.lidar.display_stride = 10000;
    let samples = lidar::read(&root, &c, 0., 0., 3).unwrap();
    assert!(samples.display.is_empty());
    assert_eq!(samples.roofs[&(10, 10)].count, 2);
    assert_eq!(samples.roofs[&(10, 10)].mean()[2], 111.);
    support::cleanup(&root);
}

#[test]
fn adjoining_road_segments_form_one_surface_without_a_junction_hole() {
    use geo::Contains;
    let c = config();
    let t = grid();
    let key = "bd_topo.troncon_de_route";
    let objects = [json!([[2, 10], [10, 10]]), json!([[10, 10], [10, 18]])]
        .into_iter()
        .map(|coordinates| {
            geometry::prepare(
                &feature(json!({"type":"LineString","coordinates":coordinates})),
                key,
                &c.layers[key],
                &t,
                &c,
            )
            .unwrap()
            .unwrap()
        })
        .collect();
    let mut layer = LayerPlan {
        name: key.into(),
        style: c.layers[key].clone(),
        sources: Vec::new(),
        partial: false,
        objects,
    };
    roads::join(&mut layer, &t, &c).unwrap();
    assert_eq!(layer.objects.len(), 1);
    let out = &layer.objects[0];
    assert!(out.lines.is_empty());
    for [x, y] in [[0.1, 0.1], [-0.5, 0.5], [1., 1.], [-0.8, -0.3]] {
        assert!(
            out.faces.iter().any(|f| {
                let a = out.vertices[f[0]];
                let b = out.vertices[f[1]];
                let d = out.vertices[f[2]];
                geo::Triangle::new(
                    geo::Coord { x: a[0], y: a[1] },
                    geo::Coord { x: b[0], y: b[1] },
                    geo::Coord { x: d[0], y: d[1] },
                )
                .contains(&geo::Point::new(x, y))
            }),
            "trou au raccord {x},{y}"
        );
    }
    assert!(out.vertices.iter().all(|p| (p[2] - 0.2).abs() < 1e-8));
}

#[test]
fn an_empty_but_complete_vector_cache_creates_no_layer() {
    let root = support::directory();
    let folder = root.join("CADASTRE/test/parcelle");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("complete.json"),json!({"source":"fixture","layer":"CADASTRALPARCELS.PARCELLAIRE_EXPRESS:parcelle","bbox":[0.,0.,20.,20.],"sort_by":"idu A","pages":[0],"features":0}).to_string()).unwrap();
    std::fs::write(
        folder.join("page-000000.geojson"),
        json!({"type":"FeatureCollection","features":[]}).to_string(),
    )
    .unwrap();
    let mut c = config();
    for (name, layer) in &mut c.layers {
        layer.enabled = name == "cadastre.parcelle";
    }
    let mut warnings = Vec::new();
    assert!(
        vectors::prepare(&root, &c, &grid(), None, &mut warnings)
            .unwrap()
            .is_empty()
    );
    assert!(warnings.is_empty());
    support::cleanup(&root);
}

#[test]
fn every_default_toml_parameter_has_an_explanatory_comment() {
    let mut previous = "";
    for line in super::config::DEFAULT_TOML
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if !line.starts_with('#') && line.contains('=') {
            assert!(previous.starts_with('#'), "sans commentaire : {line}");
        }
        previous = line;
    }
}

#[test]
fn effective_toml_can_be_saved_and_read_back() {
    let c = config();
    let text = toml::to_string_pretty(&c).unwrap();
    let reread = super::config::parse(&text).unwrap();
    assert_eq!(
        serde_json::to_value(c).unwrap(),
        serde_json::to_value(reread).unwrap()
    );
}

#[test]
fn invalid_building_quality_thresholds_are_rejected() {
    for text in [
        "[buildings]\nmin_coverage = 1.1",
        "[buildings]\nmin_density_per_m2 = 0",
        "[buildings]\ncell_size_m = 0",
        "[buildings]\nfill_radius_cells = 100",
    ] {
        assert!(super::config::parse(text).is_err());
    }
}

pub(super) fn native_ortho_cache(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let folder = root.join("BD_ORTHO/native/0");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("matrix.json"),json!({"schema_version":2,"crs":"EPSG:2154","format":"image/jpeg","coordinate_order":"x_y",
        "matrix":{"matrix_set":"native","crs":"EPSG:2154","identifier":"0","scale_denominator":1.,
        "resolution_units_per_px":2.5,"nominal_resolution_m_per_px":2.5,"top_left_x":0.,"top_left_y":20.,
        "tile_width":4,"tile_height":4,"matrix_width":2,"matrix_height":2,"limits":{"col_min":0,"col_max":1,"row_min":0,"row_max":1}}}).to_string()).unwrap();
    let mut originals = Vec::new();
    for row in 0..2 {
        std::fs::create_dir_all(folder.join(row.to_string())).unwrap();
        for col in 0..2 {
            let path = folder.join(row.to_string()).join(format!("{col}.jpg"));
            image::RgbImage::from_fn(4, 4, |x, y| {
                image::Rgb([(x * 50) as u8, (y * 50) as u8, 100])
            })
            .save(&path)
            .unwrap();
            originals.push((path.clone(), std::fs::read(path).unwrap()));
        }
    }
    originals
}

#[test]
fn native_ortho_preserves_jpeg_bytes_dimensions_and_geographic_uv() {
    let root = support::directory();
    let originals = native_ortho_cache(&root);
    let c = config();
    let mut layers = vec![LayerPlan {
        name: "bd_topo.batiment".into(),
        style: c.layers["bd_topo.batiment"].clone(),
        sources: Vec::new(),
        partial: false,
        objects: vec![building(&c)],
    }];
    let plan = ortho::prepare(&root, &c, &grid(), &mut layers).unwrap();
    assert_eq!(plan.tiles.len(), 4);
    assert!(
        plan.tiles
            .iter()
            .all(|tile| tile.width == 4 && tile.height == 4)
    );
    assert_eq!(plan.terrain_uv[0], [0., 2.]);
    assert_eq!(plan.terrain_uv[4], [1., 1.]);
    assert_eq!(plan.terrain_uv[8], [2., 0.]);
    let object = &layers[0].objects[0];
    assert_eq!(object.uv.len(), object.vertices.len());
    for (path, bytes) in originals {
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(plan.tiles.iter().any(|tile| Path::new(&tile.path) == path));
    }
    support::cleanup(&root);
}

#[test]
fn missing_native_jpeg_is_an_error_not_a_blank_or_resized_texture() {
    let root = support::directory();
    let originals = native_ortho_cache(&root);
    std::fs::remove_file(&originals[0].0).unwrap();
    assert!(ortho::prepare(&root, &config(), &grid(), &mut []).is_err());
    support::cleanup(&root);
}

#[test]
fn explicit_no_data_jpeg_uses_one_shared_fallback_image() {
    let root = support::directory();
    let originals = native_ortho_cache(&root);
    let missing = &originals[0].0;
    std::fs::remove_file(missing).unwrap();
    std::fs::write(
        crate::cache::no_data_marker_path(missing),
        b"wmts-no-data-v1\n",
    )
    .unwrap();
    let plan = ortho::prepare(&root, &config(), &grid(), &mut []).unwrap();
    let replacement = plan
        .tiles
        .iter()
        .find(|tile| tile.col == 0 && tile.row == 0)
        .unwrap();
    assert!(replacement.path.ends_with("no-data.jpg"));
    assert_eq!(image::image_dimensions(&replacement.path).unwrap(), (4, 4));
    support::cleanup(&root);
}

#[test]
fn native_texture_limit_does_not_silently_reduce_resolution() {
    let root = support::directory();
    native_ortho_cache(&root);
    let mut c = config();
    c.ortho.max_tiles = 3;
    assert!(ortho::prepare(&root, &c, &grid(), &mut []).is_err());
    support::cleanup(&root);
}

#[test]
fn bd_topo_three_dimensional_roof_is_preserved_when_lidar_is_missing() {
    let c = config();
    let f = feature(
        json!({"type":"Polygon","coordinates":[[[2,2,108],[18,2,110],[18,18,111],[2,18,109],[2,2,108]]]}),
    );
    let mut out = geometry::prepare(
        &f,
        "bd_topo.batiment",
        &c.layers["bd_topo.batiment"],
        &grid(),
        &c,
    )
    .unwrap()
    .unwrap();
    let mut samples = roof_samples();
    samples.roofs.clear();
    buildings::improve(&f, &mut out, &grid(), &c, &samples).unwrap();
    assert_eq!(out.properties["export_toit"]["source"], "BD TOPO 3D");
    assert!(out.vertices.iter().any(|p| (p[2] - 11.2).abs() < 1e-8));
    assert!(out.vertices.iter().any(|p| (p[2] - 8.2).abs() < 1e-8));
}

#[test]
fn invalid_bd_topo_roof_altitudes_keep_a_safe_height_extrusion() {
    let c = config();
    let f = feature(
        json!({"type":"Polygon","coordinates":[[[2,2,-9999],[18,2,-9999],[18,18,-9999],[2,18,-9999],[2,2,-9999]]]}),
    );
    let mut out = geometry::prepare(
        &f,
        "bd_topo.batiment",
        &c.layers["bd_topo.batiment"],
        &grid(),
        &c,
    )
    .unwrap()
    .unwrap();
    let original = out.vertices.clone();
    let mut samples = roof_samples();
    samples.roofs.clear();
    buildings::improve(&f, &mut out, &grid(), &c, &samples).unwrap();
    assert_eq!(out.vertices, original);
}
