//! Contexte : protège les téléchargements BD TOPO et le catalogue LiDAR.
//! Usage : cargo test wfs ; toutes les réponses proviennent d'un serveur local.
//! Les objets de test ont une coordonnée Z pour vérifier qu'elle est préservée.

use super::*;
use crate::test_support as support;
use serde_json::json;

#[tokio::test]
async fn cadastral_download_preserves_references_containment_and_geometry() {
    // Réponse IGN figée : les zéros de section/numéro font partie de la référence.
    let mut original: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/cadastre-page.json")).unwrap();
    original["numberMatched"] = json!(2);
    let bytes = serde_json::to_vec(&original).unwrap();
    let (url, server) = support::server(vec![reply(bytes.clone())]);
    let root = support::directory();
    let features = download_layer(
        &support::client(),
        &url,
        &crate::cadastre::LAYERS[0],
        bbox(),
        &root,
        false,
    )
    .await
    .unwrap();
    assert_eq!(features[0]["properties"]["idu"], "101370000B0001");
    assert_eq!(features[0]["properties"]["numero"], "0001");
    assert_eq!(features[0]["properties"]["section"], "0B");
    assert_eq!(features[0]["properties"]["contenance"], 1680);
    assert_eq!(features[0]["geometry"]["type"], "MultiPolygon");
    assert_eq!(
        fs::read(root.join("page-000000.geojson")).await.unwrap(),
        bytes
    );
    let requests = server.join().unwrap();
    let params: std::collections::HashMap<_, _> = requests[0].query_pairs().into_owned().collect();
    assert_eq!(
        params["TYPENAMES"],
        "CADASTRALPARCELS.PARCELLAIRE_EXPRESS:parcelle"
    );
    assert_eq!(params["SORTBY"], "idu A");
    support::cleanup(&root);
}

#[tokio::test]
async fn sandre_without_total_or_feature_id_preserves_hydrological_attributes() {
    let body = include_bytes!("../../tests/fixtures/topage-page.json").to_vec();
    let (url, server) = support::server(vec![
        reply(body.clone()),
        reply(page(&[], json!("unknown"))),
    ]);
    let root = support::directory();
    let features = download_layer(
        &support::client(),
        &url,
        &crate::topage::LAYERS[1],
        bbox(),
        &root,
        false,
    )
    .await
    .unwrap();
    assert_eq!(features.len(), 2);
    assert_eq!(features[0]["properties"]["CdOH"], "03T0000000123705519");
    assert_eq!(features[0]["properties"]["PersistanceTH"], "intermittent");
    assert_eq!(
        features[0]["properties"]["SensEcoulementTH"],
        "dans direction tronçon"
    );
    assert_eq!(features[0]["properties"]["ClasseLargeurTH"], "0_5");
    assert_eq!(
        fs::read(root.join("page-000000.geojson")).await.unwrap(),
        body
    );
    let requests = server.join().unwrap();
    let params: std::collections::HashMap<_, _> = requests[0].query_pairs().into_owned().collect();
    assert_eq!(params["OUTPUTFORMAT"], "application/json; subtype=geojson");
    assert_eq!(params["SORTBY"], "gid A");
    assert_eq!(params["SRSNAME"], "EPSG:2154");
    assert_eq!(
        requests[1]
            .query_pairs()
            .find(|(key, _)| key == "STARTINDEX")
            .unwrap()
            .1,
        "2"
    );
    support::cleanup(&root);
}

#[tokio::test]
async fn duplicate_sandre_gid_without_feature_id_is_not_silently_accepted() {
    let first: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/topage-page.json")).unwrap();
    let mut next = first.clone();
    // Le contenu diffère, mais gid est répété : la détection de page identique seule ne suffirait pas.
    next["features"][0]["properties"]["TopoOH"] = json!("nom modifié");
    fails_without_receipt(vec![
        reply(serde_json::to_vec(&first).unwrap()),
        reply(serde_json::to_vec(&next).unwrap()),
    ])
    .await;
}

#[tokio::test]
async fn missing_page_in_a_complete_cache_forces_a_new_download() {
    let (url, server) = support::server(vec![
        reply(page(&[1], json!(1))),
        reply(page(&[2], json!(1))),
    ]);
    let root = support::directory();
    let client = support::client();
    download_layer(&client, &url, &LAYER, bbox(), &root, false)
        .await
        .unwrap();
    fs::remove_file(root.join("page-000000.geojson"))
        .await
        .unwrap();
    assert_eq!(
        download_layer(&client, &url, &LAYER, bbox(), &root, false)
            .await
            .unwrap()[0]["id"],
        "batiment.2"
    );
    server.join().unwrap();
    support::cleanup(&root);
}

#[tokio::test]
async fn a_different_layer_never_reuses_another_layers_cache() {
    let (url, server) = support::server(vec![
        reply(page(&[1], json!(1))),
        reply(page(&[2], json!(1))),
    ]);
    let root = support::directory();
    let client = support::client();
    download_layer(&client, &url, &LAYER, bbox(), &root, false)
        .await
        .unwrap();
    assert_eq!(
        download_layer(
            &client,
            &url,
            &crate::cadastre::LAYERS[0],
            bbox(),
            &root,
            false
        )
        .await
        .unwrap()[0]["id"],
        "batiment.2"
    );
    server.join().unwrap();
    support::cleanup(&root);
}

#[tokio::test]
async fn overlapping_pages_with_duplicate_ids_are_rejected() {
    fails_without_receipt(vec![
        reply(page(&[1, 2], json!(4))),
        reply(page(&[2, 3], json!(4))),
    ])
    .await;
}

const LAYER: WfsLayer = WfsLayer {
    identifier: "BDTOPO_V3:batiment",
    label: "Bâtiments",
    sort_by: "cleabs A",
    output_format: "application/json",
};
fn bbox() -> Bbox {
    Bbox::square(829000.0, 6797000.0, 3000.0)
}
fn page(ids: &[u32], total: Value) -> Vec<u8> {
    let features: Vec<_> = ids
        .iter()
        .map(|id| {
            json!({
                "type":"Feature", "id":format!("batiment.{id}"),
                "properties":{"cleabs":id,"hauteur":12.5},
                "geometry":{"type":"Point","coordinates":[829000,6797000,123.4]}
            })
        })
        .collect();
    serde_json::to_vec(&json!({"type":"FeatureCollection", "features":features,
        "numberMatched":total,"numberReturned":ids.len()}))
    .unwrap()
}
fn reply(body: Vec<u8>) -> (u16, &'static str, Vec<u8>) {
    (200, "application/json", body)
}

#[tokio::test]
async fn pagination_keeps_all_objects_and_original_attributes_and_z() {
    let original = page(&[1], json!(2));
    let (url, server) = support::server(vec![reply(original.clone()), reply(page(&[2], json!(2)))]);
    let root = support::directory();
    let objects = download_layer(&support::client(), &url, &LAYER, bbox(), &root, false)
        .await
        .unwrap();
    assert_eq!(objects.len(), 2);
    assert_eq!(objects[0]["geometry"]["coordinates"][2], 123.4);
    assert_eq!(objects[0]["properties"]["hauteur"], 12.5);
    assert_eq!(
        fs::read(root.join("page-000000.geojson")).await.unwrap(),
        original
    );
    let requests = server.join().unwrap();
    for (index, request) in requests.iter().enumerate() {
        let params: std::collections::HashMap<_, _> = request.query_pairs().into_owned().collect();
        assert_eq!(params["STARTINDEX"], index.to_string());
        assert_eq!(params["COUNT"], "500");
        assert_eq!(params["BBOX"], "827500,6795500,830500,6798500,EPSG:2154");
        assert_eq!(params["SRSNAME"], "EPSG:2154");
        assert_eq!(params["SORTBY"], "cleabs A");
        assert_eq!(params["TYPENAMES"], LAYER.identifier);
    }
    support::cleanup(&root);
}

#[tokio::test]
async fn unknown_total_continues_after_short_pages_until_empty_page() {
    let (url, server) = support::server(vec![
        reply(page(&[1], json!("unknown"))),
        reply(page(&[2], json!("unknown"))),
        reply(page(&[], json!("unknown"))),
    ]);
    let root = support::directory();
    assert_eq!(
        download_layer(&support::client(), &url, &LAYER, bbox(), &root, false)
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(server.join().unwrap().len(), 3);
    support::cleanup(&root);
}

#[tokio::test]
async fn total_may_be_a_string() {
    let (url, server) = support::server(vec![reply(page(&[1], json!("1")))]);
    let root = support::directory();
    assert_eq!(
        download_layer(&support::client(), &url, &LAYER, bbox(), &root, false)
            .await
            .unwrap()
            .len(),
        1
    );
    server.join().unwrap();
    support::cleanup(&root);
}

#[tokio::test]
async fn empty_layer_is_a_successful_reusable_download() {
    let (url, server) = support::server(vec![reply(page(&[], json!(0)))]);
    let root = support::directory();
    let client = support::client();
    assert!(
        download_layer(&client, &url, &LAYER, bbox(), &root, false)
            .await
            .unwrap()
            .is_empty()
    );
    server.join().unwrap();
    assert!(
        download_layer(&client, &url, &LAYER, bbox(), &root, false)
            .await
            .unwrap()
            .is_empty()
    );
    support::cleanup(&root);
}

async fn fails_without_receipt(replies: Vec<(u16, &'static str, Vec<u8>)>) {
    let (url, server) = support::server(replies);
    let root = support::directory();
    assert!(
        download_layer(&support::client(), &url, &LAYER, bbox(), &root, false)
            .await
            .is_err()
    );
    server.join().unwrap();
    assert!(!root.join("complete.json").exists());
    support::cleanup(&root);
}
#[tokio::test]
async fn repeated_page_cannot_loop_forever() {
    fails_without_receipt(vec![
        reply(page(&[1], json!(3))),
        reply(page(&[1], json!(3))),
    ])
    .await;
}
#[tokio::test]
async fn pagination_cycles_are_also_rejected() {
    fails_without_receipt(vec![
        reply(page(&[1], json!("unknown"))),
        reply(page(&[2], json!("unknown"))),
        reply(page(&[1], json!("unknown"))),
    ])
    .await;
}
#[tokio::test]
async fn premature_empty_page_is_not_a_complete_download() {
    fails_without_receipt(vec![
        reply(page(&[1], json!(3))),
        reply(page(&[], json!(3))),
    ])
    .await;
}
#[tokio::test]
async fn changing_total_is_reported() {
    fails_without_receipt(vec![
        reply(page(&[1], json!(2))),
        reply(page(&[2], json!(3))),
    ])
    .await;
}
#[tokio::test]
async fn more_features_than_announced_is_rejected() {
    fails_without_receipt(vec![reply(page(&[1, 2], json!(1)))]).await;
}
#[tokio::test]
async fn http_error_is_not_cached() {
    fails_without_receipt(vec![(503, "text/plain", b"indisponible".to_vec())]).await;
}

#[tokio::test]
async fn a_504_after_one_page_preserves_its_identity_and_received_data() {
    let original = page(&[1], json!(2));
    let (url, server) = support::server(vec![
        reply(original.clone()),
        (504, "text/plain", b"timeout".to_vec()),
    ]);
    let root = support::directory();
    assert!(
        download_layer(&support::client(), &url, &LAYER, bbox(), &root, false)
            .await
            .is_err()
    );
    assert_eq!(
        fs::read(root.join("page-000000.geojson")).await.unwrap(),
        original
    );
    let request: WfsRequest =
        serde_json::from_slice(&fs::read(root.join("request.json")).await.unwrap()).unwrap();
    assert_eq!(request.layer, LAYER.identifier);
    assert_eq!(request.crs, "EPSG:2154");
    let bounds = bbox();
    assert_eq!(
        request.bbox,
        [bounds.xmin, bounds.ymin, bounds.xmax, bounds.ymax]
    );
    assert!(!root.join("complete.json").exists());
    assert_eq!(server.join().unwrap().len(), 2);
    support::cleanup(&root);
}
#[tokio::test]
async fn ogc_xml_error_with_http_200_is_not_cached() {
    fails_without_receipt(vec![(200, "text/xml", b"<ExceptionReport/>".to_vec())]).await;
}
#[test]
fn missing_features_is_rejected() {
    assert!(parse_page(r#"{"type":"FeatureCollection"}"#).is_err());
}
#[test]
fn incorrect_collection_type_is_rejected() {
    assert!(parse_page(r#"{"type":"Error","features":[]}"#).is_err());
}
#[test]
fn malformed_feature_is_rejected() {
    assert!(parse_page(r#"{"type":"FeatureCollection","features":[{}]}"#).is_err());
}
#[test]
fn incorrect_returned_count_is_rejected() {
    let mut value: Value = serde_json::from_slice(&page(&[1], json!(1))).unwrap();
    value["numberReturned"] = json!(9);
    assert!(parse_page(&value.to_string()).is_err());
}

#[tokio::test]
async fn complete_cache_can_be_reused_with_server_offline() {
    let (url, server) = support::server(vec![reply(page(&[1], json!(1)))]);
    let root = support::directory();
    let client = support::client();
    let first = download_layer(&client, &url, &LAYER, bbox(), &root, false)
        .await
        .unwrap();
    server.join().unwrap();
    assert_eq!(
        download_layer(&client, &url, &LAYER, bbox(), &root, false)
            .await
            .unwrap(),
        first
    );
    support::cleanup(&root);
}

#[tokio::test]
async fn force_replaces_a_previously_complete_layer() {
    let (url, server) = support::server(vec![
        reply(page(&[1], json!(1))),
        reply(page(&[2], json!(1))),
    ]);
    let root = support::directory();
    let client = support::client();
    download_layer(&client, &url, &LAYER, bbox(), &root, false)
        .await
        .unwrap();
    let second = download_layer(&client, &url, &LAYER, bbox(), &root, true)
        .await
        .unwrap();
    assert_eq!(second[0]["id"], "batiment.2");
    server.join().unwrap();
    support::cleanup(&root);
}

#[tokio::test]
async fn damaged_page_is_downloaded_again() {
    let (url, server) = support::server(vec![
        reply(page(&[1], json!(1))),
        reply(page(&[2], json!(1))),
    ]);
    let root = support::directory();
    let client = support::client();
    download_layer(&client, &url, &LAYER, bbox(), &root, false)
        .await
        .unwrap();
    fs::write(root.join("page-000000.geojson"), b"{")
        .await
        .unwrap();
    assert_eq!(
        download_layer(&client, &url, &LAYER, bbox(), &root, false)
            .await
            .unwrap()[0]["id"],
        "batiment.2"
    );
    server.join().unwrap();
    support::cleanup(&root);
}

#[tokio::test]
async fn nearby_but_different_bbox_cannot_reuse_the_same_cache() {
    let (url, server) = support::server(vec![
        reply(page(&[1], json!(1))),
        reply(page(&[2], json!(1))),
    ]);
    let root = support::directory();
    let client = support::client();
    download_layer(&client, &url, &LAYER, bbox(), &root, false)
        .await
        .unwrap();
    let mut other = bbox();
    other.xmin += 0.0001;
    assert_eq!(
        download_layer(&client, &url, &LAYER, other, &root, false)
            .await
            .unwrap()[0]["id"],
        "batiment.2"
    );
    server.join().unwrap();
    support::cleanup(&root);
}

#[tokio::test]
async fn failed_force_invalidates_the_old_completion_receipt() {
    let (url, server) = support::server(vec![
        reply(page(&[1], json!(1))),
        (500, "text/plain", vec![]),
    ]);
    let root = support::directory();
    let client = support::client();
    download_layer(&client, &url, &LAYER, bbox(), &root, false)
        .await
        .unwrap();
    assert!(
        download_layer(&client, &url, &LAYER, bbox(), &root, true)
            .await
            .is_err()
    );
    assert!(!root.join("complete.json").exists());
    server.join().unwrap();
    support::cleanup(&root);
}

#[tokio::test]
async fn invalid_bbox_is_rejected_before_network_or_cache() {
    let root = support::directory();
    let mut area = bbox();
    area.xmin = f64::NAN;
    assert!(
        download_layer(
            &support::client(),
            "http://127.0.0.1:1",
            &LAYER,
            area,
            &root,
            false
        )
        .await
        .is_err()
    );
    support::cleanup(&root);
}
