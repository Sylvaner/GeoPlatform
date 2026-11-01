//! Contexte : une liste de tuiles manquantes a été préparée par `cache.rs`.
//! Usage : `app.rs` appelle `download_tiles` pour récupérer la liste et afficher
//! un bilan. Pour chaque tuile, wmts lit le réseau et cache écrit le fichier.
//! La concurrence reste limitée par l'option --concurrency.

use anyhow::{Result, bail};
use futures::{StreamExt, stream};
use reqwest::Client;

use crate::{
    cache,
    models::{CachedTile, TileMatrix, WmtsLayer},
    wmts,
};

enum DownloadOutcome {
    Downloaded(u64),
    NoData,
}

/// Traite les tuiles et continue après les erreurs individuelles avant de faire le bilan.
pub async fn download_tiles(
    client: &Client,
    layer: &WmtsLayer,
    matrix: &TileMatrix,
    tiles: Vec<CachedTile>,
    concurrency: usize,
) -> Result<()> {
    // stream::iter transforme la liste en flux ; map associe un traitement à chaque tuile.
    // |tile| est une fonction anonyme recevant une tuile de la liste.
    // async move crée un traitement asynchrone qui prend possession de cette tuile.
    // Les &Client, &WmtsLayer et &TileMatrix restent des références empruntées.
    let downloads = stream::iter(tiles).map(|tile| async move {
        // On garde le Result sans ? : on pourra afficher l'erreur et continuer.
        let result = download_one(client, layer, matrix, &tile).await;
        (tile, result)
    });

    // buffer_unordered garde au plus concurrency traitements actifs.
    // Les résultats arrivent dans leur ordre de fin, pas dans l'ordre de la liste.
    let mut downloads = downloads.buffer_unordered(concurrency);
    let mut succeeded = 0;
    let mut no_data = 0;
    let mut failed = 0;

    // next renvoie Some(résultat) tant qu'il reste des tuiles, puis None à la fin.
    // Le tuple (tile, result) associe chaque résultat à son fichier de destination.
    while let Some((tile, result)) = downloads.next().await {
        match result {
            Ok(DownloadOutcome::Downloaded(size)) => {
                succeeded += 1;
                println!(
                    "[OK] row={} col={} -> {} ({} octets)",
                    tile.row,
                    tile.col,
                    tile.path.display(),
                    size
                );
            }
            Ok(DownloadOutcome::NoData) => {
                no_data += 1;
            }
            Err(error) => {
                // eprintln écrit sur la sortie d'erreur ; {:#} affiche aussi les causes.
                failed += 1;
                eprintln!("[ERREUR] row={} col={} : {:#}", tile.row, tile.col, error);
            }
        }
    }

    println!("Téléchargées   : {}", succeeded);
    println!("Sans donnée   : {}", no_data);
    println!("Échecs         : {}", failed);
    // Toutes les tuiles ont été tentées avant de signaler un échec global.
    if failed > 0 {
        let total = succeeded + no_data + failed;
        bail!(
            "{failed} tuile(s) n'ont pas pu être téléchargées\nÉchecs : {failed}\nTuiles : {total}"
        );
    }
    Ok(())
}

/// Reçoit les octets d'une tuile, les enregistre et renvoie leur nombre.
async fn download_one(
    client: &Client,
    layer: &WmtsLayer,
    matrix: &TileMatrix,
    tile: &CachedTile,
) -> Result<DownloadOutcome> {
    // Ici ? arrête seulement le traitement de cette tuile en cas d'erreur.
    // download_tiles reçoit cette erreur et peut continuer avec les autres tuiles.
    let Some(bytes) = wmts::get_tile(client, layer, matrix, tile.row, tile.col).await? else {
        cache::write_no_data_marker(&tile.path).await?;
        return Ok(DownloadOutcome::NoData);
    };
    cache::write_tile(&tile.path, &bytes).await?;
    Ok(DownloadOutcome::Downloaded(bytes.len() as u64))
}
