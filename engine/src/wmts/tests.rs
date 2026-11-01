//! Contexte : une réponse HTTP 200 n'est pas toujours une tuile exploitable.
//! Usage : cargo test wmts ; les faux serveurs locaux simulent les réponses IGN.

use super::*;
use crate::{models::WmtsProduct, test_support as support};

#[tokio::test]
async fn gzip_http_is_decoded_before_checking_bil_size() {
    // En-tête ajouté par notre serveur de test, comme sur le vrai transport HTTP.
    let bytes = include_bytes!("../../tests/fixtures/tile.bil.gz").to_vec();
    assert_eq!(
        fetch(bytes, 200, "image/x-bil\r\nContent-Encoding: gzip", false)
            .await
            .unwrap()
            .unwrap(),
        (0..16).collect::<Vec<u8>>()
    );
}
#[tokio::test]
async fn deflate_http_is_decoded_before_checking_bil_size() {
    let bytes = include_bytes!("../../tests/fixtures/tile.bil.deflate").to_vec();
    assert_eq!(
        fetch(
            bytes,
            200,
            "image/x-bil\r\nContent-Encoding: deflate",
            false
        )
        .await
        .unwrap()
        .unwrap(),
        (0..16).collect::<Vec<u8>>()
    );
}

async fn fetch(
    body: Vec<u8>,
    status: u16,
    content_type: &'static str,
    jpeg: bool,
) -> Result<Option<Vec<u8>>> {
    let (url, server) = support::server(vec![(status, content_type, body)]);
    let (mut layer, mut matrix) = parse_capabilities(
        include_str!("../../tests/fixtures/capabilities.xml"),
        WmtsProduct::RgeAlti,
    )
    .unwrap();
    // Une grille minuscule rend le contenu attendu lisible : 2 × 2 × 4 = 16 octets.
    matrix.tile_width = 2;
    matrix.tile_height = 2;
    if jpeg {
        layer.format = "image/jpeg".to_owned();
    }
    let result = get_tile_at(&support::client(), &url, &layer, &matrix, 4, 7).await;
    let requests = server.join().unwrap();
    let params: std::collections::HashMap<_, _> = requests[0].query_pairs().into_owned().collect();
    assert_eq!(params["TILEMATRIXSET"], "WGS84G_6_14");
    assert_eq!(params["TILEMATRIX"], "14");
    assert_eq!(params["TILEROW"], "4");
    assert_eq!(params["TILECOL"], "7");
    assert_eq!(params["LAYER"], layer.identifier);
    result
}

#[tokio::test]
async fn valid_bil_bytes_and_request_parameters_are_preserved() {
    let body: Vec<u8> = (0..16).collect();
    assert_eq!(
        fetch(body.clone(), 200, "image/x-bil", false)
            .await
            .unwrap()
            .unwrap(),
        body
    );
}
#[tokio::test]
async fn truncated_bil_is_rejected() {
    assert!(fetch(vec![0; 15], 200, "image/x-bil", false).await.is_err());
}
#[tokio::test]
async fn oversized_bil_is_rejected() {
    assert!(fetch(vec![0; 17], 200, "image/x-bil", false).await.is_err());
}
#[tokio::test]
async fn empty_tile_is_rejected() {
    assert!(fetch(vec![], 200, "image/x-bil", false).await.is_err());
}
#[tokio::test]
async fn xml_exception_with_success_status_is_rejected() {
    assert!(
        fetch(b"<?xml error/>".to_vec(), 200, "text/xml", false)
            .await
            .is_err()
    );
}
#[tokio::test]
async fn html_error_with_success_status_is_rejected() {
    assert!(fetch(vec![0; 16], 200, "text/html", false).await.is_err());
}
#[tokio::test]
async fn http_failure_is_rejected() {
    assert!(fetch(vec![0; 16], 500, "image/x-bil", false).await.is_err());
}
#[tokio::test]
async fn http_not_found_is_a_valid_no_data_tile() {
    assert_eq!(
        fetch(Vec::new(), 404, "text/plain", false).await.unwrap(),
        None
    );
}
#[tokio::test]
async fn jpeg_requires_a_binary_signature() {
    assert!(
        fetch(b"not a photograph".to_vec(), 200, "image/jpeg", true)
            .await
            .is_err()
    );
}
#[tokio::test]
async fn jpeg_transport_keeps_original_bytes() {
    // Signature synthétique : ce test protège le transfert, pas le décodage de JPEG.
    let body = vec![0xff, 0xd8, 0xff, 0xe0, 1, 2, 3];
    assert_eq!(
        fetch(body.clone(), 200, "image/jpeg", true)
            .await
            .unwrap()
            .unwrap(),
        body
    );
}
