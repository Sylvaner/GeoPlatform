//! Contexte : l'eau ne doit pas disparaître dans les triangles du relief orthophotographique.
//! Usage : cargo test --lib export::water_tests ; toutes les surfaces sont synthétiques.
//! Vérifier aussi l'intérieur des faces détecte le défaut invisible dans un test des seuls sommets.

use super::{config, geometry, models::TerrainGrid};
use serde_json::json;

#[test]
fn water_sources_do_not_create_coincident_surfaces() {
    let t = terrain();
    let mut covered = geo::MultiPolygon(Vec::new());
    assert!(
        super::water::uncovered(&lake(), &mut covered, &t)
            .unwrap()
            .is_some()
    );
    assert!(
        super::water::uncovered(&lake(), &mut covered, &t)
            .unwrap()
            .is_none()
    );
    let mut neighbour = lake();
    neighbour["geometry"]["coordinates"] = json!([[[10, 1], [20, 1], [20, 19], [10, 19], [10, 1]]]);
    let remaining = super::water::uncovered(&neighbour, &mut covered, &t)
        .unwrap()
        .unwrap();
    let c = config::parse("").unwrap();
    let object = geometry::prepare(
        &remaining,
        "bd_topage.SurfaceElementaire",
        &c.layers["bd_topage.SurfaceElementaire"],
        &t,
        &c,
    )
    .unwrap()
    .unwrap();
    assert!(
        object
            .vertices
            .iter()
            .all(|p| p[0] + t.origin[0] >= 19. - 1e-6)
    );
    assert!(
        super::water::priority("bd_topage.PlanEau")
            < super::water::priority("bd_topage.SurfaceElementaire")
    );
}

fn terrain() -> TerrainGrid {
    TerrainGrid {
        xmin: 0.,
        ymin: 0.,
        side: 20.,
        count: 3,
        heights: vec![100., 100., 100., 100., 120., 100., 100., 100., 100.],
        origin: [10., 10., 100.],
        exaggeration: 1.,
        source: "test".into(),
    }
}

fn lake() -> serde_json::Value {
    json!({"type":"Feature", "id":"lake", "properties":{}, "geometry":{
        "type":"Polygon", "coordinates":[[[1,1],[19,1],[19,19],[1,19],[1,1]]]}})
}

#[test]
fn water_follows_the_photographic_surface_even_inside_large_polygons() {
    let mut c = config::parse("").unwrap();
    c.layers.get_mut("orthophoto").unwrap().z_offset_m = 2.;
    let mut t = terrain();
    t.exaggeration = 3.;
    let out = geometry::prepare(
        &lake(),
        "bd_topage.PlanEau",
        &c.layers["bd_topage.PlanEau"],
        &t,
        &c,
    )
    .unwrap()
    .unwrap();
    assert!(
        out.faces.len() > 2,
        "un grand lac ne peut pas rester deux triangles plats"
    );
    for face in &out.faces {
        let points: Vec<_> = face.iter().map(|&i| out.vertices[i]).collect();
        let centroid: [f64; 3] =
            std::array::from_fn(|axis| points.iter().map(|p| p[axis]).sum::<f64>() / 3.);
        for point in points.iter().chain(std::iter::once(&centroid)) {
            let x = point[0] + t.origin[0];
            let y = point[1] + t.origin[1];
            let photo_z = (t.mesh_height(x, y) - t.origin[2]) * t.exaggeration + 2.;
            assert!(
                (point[2] - photo_z - c.water.surface_clearance_m).abs() < 1e-6,
                "face traversante en {point:?}, orthophoto {photo_z}"
            );
        }
    }
    assert_eq!(out.properties["export_eau"]["thickness_m"], 1.0);
}

#[test]
fn mesh_height_uses_triangles_instead_of_a_bilinear_saddle() {
    let mut t = terrain();
    t.count = 2;
    t.heights = vec![100., 100., 100., 120.];
    assert_eq!(t.mesh_height(10., 10.), 110.);
    assert_eq!(t.height(10., 10.).unwrap(), 105.);
    assert_eq!(t.mesh_height(20., 20.), 120.);
    assert_eq!(t.mesh_height(20., 0.), 100.);
}

#[test]
fn water_remains_clipped_and_preserves_islands() {
    let c = config::parse("").unwrap();
    let mut feature = lake();
    feature["geometry"]["coordinates"] = json!([
        [[-5, -5], [25, -5], [25, 25], [-5, 25], [-5, -5]],
        [[8, 8], [8, 12], [12, 12], [12, 8], [8, 8]]
    ]);
    let t = terrain();
    let out = geometry::prepare(
        &feature,
        "bd_topage.PlanEau",
        &c.layers["bd_topage.PlanEau"],
        &t,
        &c,
    )
    .unwrap()
    .unwrap();
    let mut area = 0.;
    for face in &out.faces {
        let [a, b, d] = [
            out.vertices[face[0]],
            out.vertices[face[1]],
            out.vertices[face[2]],
        ];
        let signed = ((b[0] - a[0]) * (d[1] - a[1]) - (b[1] - a[1]) * (d[0] - a[0])) / 2.;
        assert!(signed > 0., "normale d'eau orientée vers le bas");
        area += signed;
    }
    assert!((area - 384.).abs() < 1e-6);
    assert!(
        out.vertices
            .iter()
            .all(|p| p[0].abs() <= 10.000001 && p[1].abs() <= 10.000001)
    );
}

#[test]
fn water_density_and_invalid_settings_fail_explicitly() {
    let mut c = config::parse("").unwrap();
    c.water.max_vertices = 3;
    assert!(
        geometry::prepare(
            &lake(),
            "bd_topage.PlanEau",
            &c.layers["bd_topage.PlanEau"],
            &terrain(),
            &c
        )
        .unwrap_err()
        .to_string()
        .contains("surface d'eau trop dense")
    );
    for bad in [
        "[water]\nthickness_m = 0",
        "[water]\nsurface_clearance_m = -1",
        "[water]\nmax_vertices = 2",
        "[view]\noutliner_timeout_seconds = 0",
    ] {
        assert!(
            config::parse(bad).is_err(),
            "configuration acceptée : {bad}"
        );
    }
}

#[test]
fn empty_or_degenerate_features_do_not_create_objects() {
    let c = config::parse("").unwrap();
    for shape in [
        json!({"type":"Polygon","coordinates":[]}),
        json!({"type":"Polygon","coordinates":[[[1,1],[2,1],[3,1],[1,1]]]}),
        json!({"type":"LineString","coordinates":[[1,1],[1,1]]}),
        json!({"type":"MultiPoint","coordinates":[]}),
    ] {
        let mut feature = lake();
        feature["geometry"] = shape;
        assert!(
            geometry::prepare(
                &feature,
                "bd_topage.PlanEau",
                &c.layers["bd_topage.PlanEau"],
                &terrain(),
                &c
            )
            .unwrap()
            .is_none()
        );
    }
}
