//! Contexte : LiDAR HD fournit des nuages de points 3D classés, pas des images WMTS.
//! Usage : main.rs appelle run pour la commande LIDAR_HD avec les mêmes options de zone.
//! Étapes : carré Lambert-93, catalogue WFS, sélection des dalles, cache puis transfert.
//! Les fichiers .copc.laz sont conservés tels quels, sans décompression LAZ ni export.

mod cache;
mod catalogue;
mod download;

use anyhow::{Result, bail};
use serde_json::json;
use tokio::fs;

use crate::{cli::DownloadOptions, coordinates, http, models::Bbox};

/// Récupère les dalles de points classés qui intersectent la zone demandée.
pub async fn run(options: DownloadOptions) -> Result<()> {
    options.validate()?;
    let (x, y) = coordinates::to_lambert93(options.center[0], options.center[1], &options.crs)?;
    let bbox = Bbox::square(x, y, options.side);
    let root = crate::cache::prepare_root(&options.root, "LIDAR_HD").await?;
    let client = http::create_client()?;
    println!("Produit       : LiDAR HD — nuages de points classés");
    println!("Centre L93    : {x:.3}, {y:.3}");
    println!(
        "Emprise L93   : {:.3},{:.3},{:.3},{:.3}",
        bbox.xmin, bbox.ymin, bbox.xmax, bbox.ymax
    );

    // Contrairement aux tuiles d'images, les liens LAZ sont fournis dans des entités
    // géographiques du catalogue. Ne pas reconstruire une URL à partir des indices.
    let tiles = catalogue::find_tiles(&client, bbox, &root).await?;
    if tiles.is_empty() {
        bail!("aucun nuage de points LiDAR HD publié pour cette zone dans le catalogue IGN");
    }

    // Ce JSON est un inventaire de téléchargement, pas un export des données.
    // Il conserve les propriétés IGN : acquisition, édition, systèmes de référence…
    let selection = json!({
        "source": catalogue::WFS_URL,
        "catalogue_layer": catalogue::LIDAR_LAYER,
        "request_crs": "EPSG:2154",
        "request_bbox": [bbox.xmin, bbox.ymin, bbox.xmax, bbox.ymax],
        "files": &tiles,
    });
    fs::write(
        root.join("selection.json"),
        serde_json::to_vec_pretty(&selection)?,
    )
    .await?;
    let (missing, present) = cache::find_missing(&root, tiles, options.force).await;
    println!("Déjà en cache : {present}");
    println!("À télécharger : {} dalle(s) LAZ complètes", missing.len());
    println!("Les dalles kilométriques entières peuvent dépasser le carré demandé.");
    if options.dry_run || missing.is_empty() {
        return Ok(());
    }
    download::download_tiles(&client, &root, missing, options.concurrency).await
}
