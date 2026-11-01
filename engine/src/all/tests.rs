//! Contexte : ALL doit préserver les options et tenter chaque produit même après un échec.
//! Usage : cargo test all::tests ; les téléchargeurs sont simulés, sans accès réseau.

use super::*;
use anyhow::anyhow;

fn options() -> DownloadOptions {
    DownloadOptions {
        center: vec![48.26183052164179, 4.738721628572549],
        crs: "latlon".into(),
        side: 3000.0,
        root: "données locales".into(),
        concurrency: 2,
        force: false,
        dry_run: false,
    }
}

#[tokio::test]
async fn downloads_all_six_products_once_with_lidar_last() {
    let mut called = Vec::new();
    run_with(options(), |product, _| {
        called.push(product.name());
        async { Ok(()) }
    })
    .await
    .unwrap();
    assert_eq!(
        called,
        [
            "RGE_ALTI",
            "BD_ORTHO",
            "BD_TOPO",
            "CADASTRE",
            "BD_TOPAGE",
            "LIDAR_HD"
        ]
    );
}

#[tokio::test]
async fn forwards_every_option_to_every_product() {
    let mut input = options();
    input.crs = "lonlat".into();
    input.center = vec![4.73, 48.26];
    input.force = true;
    input.dry_run = true;
    let mut count = 0;
    run_with(input.clone(), |_, received| {
        assert_eq!(received.center, input.center);
        assert_eq!(received.crs, input.crs);
        assert_eq!(received.side, input.side);
        assert_eq!(received.root, input.root);
        assert_eq!(received.concurrency, input.concurrency);
        assert!(received.force);
        assert!(received.dry_run);
        count += 1;
        async { Ok(()) }
    })
    .await
    .unwrap();
    assert_eq!(count, 6);
}

#[tokio::test]
async fn continues_after_failure_and_returns_the_product_and_cause() {
    let mut called = Vec::new();
    let error = run_with(options(), |product, _| {
        called.push(product);
        async move {
            if product == Dataset::BdOrtho {
                Err(anyhow!("serveur indisponible"))
            } else {
                Ok(())
            }
        }
    })
    .await
    .unwrap_err();
    assert_eq!(called.len(), 6);
    assert_eq!(called.last(), Some(&Dataset::LidarHd));
    assert!(
        error
            .to_string()
            .contains("BD_ORTHO : serveur indisponible")
    );
}

#[tokio::test]
async fn reports_all_failures_instead_of_only_the_first_one() {
    let error = run_with(options(), |_, _| async { Err(anyhow!("hors ligne")) })
        .await
        .unwrap_err();
    for name in [
        "RGE_ALTI",
        "BD_ORTHO",
        "BD_TOPO",
        "CADASTRE",
        "BD_TOPAGE",
        "LIDAR_HD",
    ] {
        assert!(error.to_string().contains(&format!("{name} : hors ligne")));
    }
}

#[tokio::test]
async fn invalid_options_do_not_start_any_download() {
    let mut input = options();
    input.side = 0.0;
    let mut called = false;
    assert!(
        run_with(input, |_, _| {
            called = true;
            async { Ok(()) }
        })
        .await
        .is_err()
    );
    assert!(!called);
}

#[tokio::test]
async fn invalid_coordinates_do_not_start_any_download() {
    let mut input = options();
    input.center = vec![91.0, 4.0];
    let mut called = false;
    assert!(
        run_with(input, |_, _| {
            called = true;
            async { Ok(()) }
        })
        .await
        .is_err()
    );
    assert!(!called);
}
