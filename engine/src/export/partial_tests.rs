//! Contexte : une erreur 504 ou une zone plus grande ne doit pas faire perdre les données locales.
//! Usage : cargo test --lib export::partial_tests ; tous les caches sont synthétiques.
//! On vérifie les limites de couverture, les interruptions, les doublons et les pages invalides.

use super::{
    config,
    models::{ExportConfig, TerrainGrid},
    vectors,
};
use crate::test_support as support;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn settings() -> ExportConfig {
    let mut c = config::parse("").unwrap();
    for (key, style) in &mut c.layers {
        style.enabled = key == "cadastre.parcelle";
    }
    c
}

fn terrain() -> TerrainGrid {
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

fn parcel(id: &str, x: f64) -> Value {
    json!({"type":"Feature","id":id,"properties":{"idu":id},"geometry":{"type":"Polygon","coordinates":[[[x,2.],[x+2.,2.],[x+2.,4.],[x,4.],[x,2.]]]}})
}

/// request.json suffit pour un transfert interrompu ; complete.json certifie toutes ses pages.
fn cache(
    root: &Path,
    name: &str,
    bbox: [f64; 4],
    complete: bool,
    pages: Vec<(usize, Vec<Value>)>,
) -> PathBuf {
    let folder = root.join("CADASTRE").join(name).join("parcelle");
    std::fs::create_dir_all(&folder).unwrap();
    let mut meta = json!({"source":"fixture","layer":"CADASTRALPARCELS.PARCELLAIRE_EXPRESS:parcelle","bbox":bbox,"sort_by":"idu A","crs":"EPSG:2154"});
    if complete {
        meta["pages"] = json!(pages.iter().map(|(start, _)| *start).collect::<Vec<_>>());
        meta["features"] = json!(pages.iter().map(|(_, p)| p.len()).sum::<usize>());
    }
    std::fs::write(
        folder.join(if complete {
            "complete.json"
        } else {
            "request.json"
        }),
        meta.to_string(),
    )
    .unwrap();
    for (start, features) in pages {
        std::fs::write(
            folder.join(format!("page-{start:06}.geojson")),
            json!({"type":"FeatureCollection","features":features}).to_string(),
        )
        .unwrap();
    }
    folder
}

#[test]
fn a_complete_neighbor_cache_is_used_even_when_its_extent_is_smaller() {
    let root = support::directory();
    cache(
        &root,
        "left",
        [0., 0., 10., 20.],
        true,
        vec![(0, vec![parcel("a", 2.)])],
    );
    let mut warnings = Vec::new();
    let layers = vectors::prepare(&root, &settings(), &terrain(), None, &mut warnings).unwrap();
    assert_eq!(layers[0].objects.len(), 1);
    assert!(layers[0].partial);
    assert!(warnings.iter().any(|w| w.contains("partielles")));
    support::cleanup(&root);
}

#[test]
fn strict_mode_still_requires_a_single_complete_covering_cache() {
    let root = support::directory();
    cache(
        &root,
        "left",
        [0., 0., 10., 20.],
        true,
        vec![(0, vec![parcel("a", 2.)])],
    );
    let mut c = settings();
    c.vectors.allow_partial_cache = false;
    assert!(
        vectors::prepare(&root, &c, &terrain(), None, &mut Vec::new())
            .unwrap()
            .is_empty()
    );
    support::cleanup(&root);
}

#[test]
fn two_complete_neighbor_caches_can_cover_the_whole_requested_area() {
    let root = support::directory();
    cache(
        &root,
        "left",
        [0., 0., 10., 20.],
        true,
        vec![(0, vec![parcel("a", 2.)])],
    );
    cache(
        &root,
        "right",
        [10., 0., 20., 20.],
        true,
        vec![(0, vec![parcel("b", 12.)])],
    );
    let layers = vectors::prepare(&root, &settings(), &terrain(), None, &mut Vec::new()).unwrap();
    assert_eq!(layers[0].objects.len(), 2);
    assert_eq!(layers[0].sources.len(), 2);
    assert!(!layers[0].partial);
    support::cleanup(&root);
}

#[test]
fn valid_pages_after_a_pagination_gap_are_not_discarded() {
    let root = support::directory();
    cache(
        &root,
        "interrupted",
        [0., 0., 20., 20.],
        false,
        vec![(0, vec![parcel("a", 2.)]), (1000, vec![parcel("b", 12.)])],
    );
    let layers = vectors::prepare(&root, &settings(), &terrain(), None, &mut Vec::new()).unwrap();
    assert_eq!(layers[0].objects.len(), 2);
    assert!(layers[0].partial);
    support::cleanup(&root);
}

#[test]
fn legacy_pages_without_a_receipt_keep_their_exact_folder_identity() {
    let root = support::directory();
    let folder = cache(
        &root,
        "0_0_10_20",
        [0., 0., 10., 20.],
        false,
        vec![(0, vec![parcel("a", 2.)])],
    );
    std::fs::remove_file(folder.join("request.json")).unwrap();
    let layers = vectors::prepare(&root, &settings(), &terrain(), None, &mut Vec::new()).unwrap();
    assert_eq!(layers[0].objects.len(), 1);
    assert!(layers[0].partial);
    support::cleanup(&root);
}

#[test]
fn overlapping_pages_are_deduplicated_by_stable_cadastral_reference() {
    let root = support::directory();
    let a = parcel("000001", 2.);
    let mut b = a.clone();
    b["id"] = json!("another-geoserver-id");
    cache(&root, "left", [0., 0., 12., 20.], true, vec![(0, vec![a])]);
    cache(&root, "right", [1., 0., 20., 20.], true, vec![(0, vec![b])]);
    let layers = vectors::prepare(&root, &settings(), &terrain(), None, &mut Vec::new()).unwrap();
    assert_eq!(layers[0].objects.len(), 1);
    support::cleanup(&root);
}

#[test]
fn a_truncated_page_does_not_discard_other_valid_pages() {
    let root = support::directory();
    let folder = cache(
        &root,
        "interrupted",
        [0., 0., 20., 20.],
        false,
        vec![(0, vec![parcel("a", 2.)]), (1, vec![parcel("b", 12.)])],
    );
    std::fs::write(folder.join("page-000001.geojson"), "{").unwrap();
    let mut warnings = Vec::new();
    let layers = vectors::prepare(&root, &settings(), &terrain(), None, &mut warnings).unwrap();
    assert_eq!(layers[0].objects.len(), 1);
    assert!(layers[0].partial);
    assert!(warnings.iter().any(|w| w.contains("page ignorée")));
    support::cleanup(&root);
}

#[test]
fn partial_pages_from_another_layer_or_outside_the_area_are_ignored() {
    let root = support::directory();
    let folder = cache(
        &root,
        "foreign",
        [0., 0., 20., 20.],
        false,
        vec![(0, vec![parcel("a", 2.)])],
    );
    let mut request: Value =
        serde_json::from_slice(&std::fs::read(folder.join("request.json")).unwrap()).unwrap();
    request["layer"] = json!("BDTOPO_V3:batiment");
    std::fs::write(folder.join("request.json"), request.to_string()).unwrap();
    cache(
        &root,
        "outside",
        [100., 100., 120., 120.],
        false,
        vec![(0, vec![parcel("b", 102.)])],
    );
    assert!(
        vectors::prepare(&root, &settings(), &terrain(), None, &mut Vec::new())
            .unwrap()
            .is_empty()
    );
    support::cleanup(&root);
}

#[test]
fn a_complete_covering_cache_takes_precedence_over_interrupted_versions() {
    let root = support::directory();
    cache(
        &root,
        "complete",
        [0., 0., 20., 20.],
        true,
        vec![(0, vec![parcel("a", 2.)])],
    );
    cache(
        &root,
        "interrupted",
        [0., 0., 20., 20.],
        false,
        vec![(0, vec![parcel("b", 12.)])],
    );
    let layers = vectors::prepare(&root, &settings(), &terrain(), None, &mut Vec::new()).unwrap();
    assert_eq!(layers[0].objects.len(), 1);
    assert!(!layers[0].partial);
    support::cleanup(&root);
}

#[test]
fn a_failed_first_page_does_not_create_a_layer_or_invent_any_parcel() {
    let root = support::directory();
    cache(
        &root,
        "first-page-failed",
        [0., 0., 20., 20.],
        false,
        Vec::new(),
    );
    assert!(
        vectors::prepare(&root, &settings(), &terrain(), None, &mut Vec::new())
            .unwrap()
            .is_empty()
    );
    support::cleanup(&root);
}
