//! Contexte : cadastre, BD TOPO et BD TOPAGE sont des objets vectoriels servis par WFS.
//! Usage : chaque produit décrit ses couches ; run applique le même parcours simple.
//! Le centre GPS se saisit latitude puis longitude. Le carré est calculé en mètres
//! Lambert-93, puis utilisé comme filtre d'intersection, sans découper les objets.

use crate::{
    cli::DownloadOptions,
    coordinates, http,
    models::{Bbox, WfsLayer},
    wfs,
};
use anyhow::Result;

/// Télécharge les couches successivement, pour garder le déroulement facile à suivre.
pub async fn run(
    options: DownloadOptions,
    product: &str,
    endpoint: &str,
    layers: &[WfsLayer],
) -> Result<()> {
    options.validate()?;
    let (x, y) = coordinates::to_lambert93(options.center[0], options.center[1], &options.crs)?;
    let bbox = Bbox::square(x, y, options.side);
    let root = crate::cache::prepare_root(&options.root, product).await?;
    // Ne pas arrondir les coordonnées du nom : deux zones très proches doivent
    // rester deux recherches distinctes. Le reçu garde aussi les valeurs exactes.
    let directory = root.join(format!(
        "{}_{}_{}_{}",
        bbox.xmin, bbox.ymin, bbox.xmax, bbox.ymax
    ));
    println!("Produit       : {product} — {} couche(s)", layers.len());
    println!("Centre L93    : {x:.3}, {y:.3}");
    println!(
        "Emprise L93   : {},{},{},{}",
        bbox.xmin, bbox.ymin, bbox.xmax, bbox.ymax
    );
    println!("Source        : {endpoint}");
    if options.dry_run {
        // Les réponses WFS sont déjà les données : ne pas les demander en simulation.
        // Le nombre d'objets ne sera connu qu'au téléchargement effectif.
        for layer in layers {
            println!("[PRÉVU] {} : {}", layer.label, layer.identifier);
        }
        println!("Destination   : {}", directory.display());
        return Ok(());
    }
    let client = http::create_client()?;
    for layer in layers {
        // Retirer le préfixe XML (avant :) pour obtenir un nom de dossier lisible.
        let name = layer.identifier.split(':').next_back().unwrap();
        wfs::download_layer(
            &client,
            endpoint,
            layer,
            bbox,
            &directory.join(name),
            options.force,
        )
        .await?;
    }
    println!("Données       : {}", directory.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support as support;

    fn options(root: &std::path::Path) -> DownloadOptions {
        DownloadOptions {
            center: vec![48.26183052164179, 4.738721628572549],
            crs: "latlon".into(),
            side: 3000.0,
            root: root.to_owned(),
            concurrency: 6,
            force: false,
            dry_run: true,
        }
    }

    #[tokio::test]
    async fn cadastre_dry_run_creates_no_data_and_needs_no_network() {
        let root = support::directory();
        // Une URL volontairement inutilisable prouve que la simulation ne contacte pas WFS.
        run(
            options(&root),
            "CADASTRE",
            "http://127.0.0.1:1",
            crate::cadastre::LAYERS,
        )
        .await
        .unwrap();
        assert!(root.join("CADASTRE").is_dir());
        assert!(
            std::fs::read_dir(root.join("CADASTRE"))
                .unwrap()
                .next()
                .is_none()
        );
        support::cleanup(&root);
    }
    #[tokio::test]
    async fn topage_dry_run_separates_the_edition_without_downloading() {
        let root = support::directory();
        run(
            options(&root),
            "BD_TOPAGE/2026",
            "http://127.0.0.1:1",
            crate::topage::LAYERS,
        )
        .await
        .unwrap();
        assert!(root.join("BD_TOPAGE/2026").is_dir());
        assert!(
            std::fs::read_dir(root.join("BD_TOPAGE/2026"))
                .unwrap()
                .next()
                .is_none()
        );
        support::cleanup(&root);
    }
    #[tokio::test]
    async fn bdtopo_still_uses_the_common_dry_run_without_network() {
        let root = support::directory();
        run(
            options(&root),
            "BD_TOPO",
            "http://127.0.0.1:1",
            crate::bdtopo::LAYERS,
        )
        .await
        .unwrap();
        assert!(root.join("BD_TOPO").is_dir());
        support::cleanup(&root);
    }
    #[tokio::test]
    async fn invalid_area_is_rejected_before_creating_a_product_directory() {
        let root = support::directory();
        let mut input = options(&root);
        input.side = 0.0;
        assert!(
            run(
                input,
                "CADASTRE",
                "http://127.0.0.1:1",
                crate::cadastre::LAYERS
            )
            .await
            .is_err()
        );
        assert!(!root.join("CADASTRE").exists());
        support::cleanup(&root);
    }
}
