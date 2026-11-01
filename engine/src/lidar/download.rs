//! Contexte : une dalle LiDAR peut contenir des millions de points, dans un gros LAZ.
//! Usage : download_tiles traite les dalles manquantes et conserve les fichiers originaux.
//! Le corps HTTP est écrit par petits morceaux : la mémoire ne dépend pas de la taille
//! totale du fichier. La compression LAZ/COPC reste intacte ; il n'y a aucun export.

use anyhow::{Context, Result, bail};
use futures::{StreamExt, stream};
use reqwest::Client;
use std::path::Path;
use tokio::{fs, io::AsyncWriteExt};

use super::cache;
use crate::models::LidarTile;

/// Télécharge les dalles en continu, avec au plus concurrency fichiers actifs.
pub async fn download_tiles(
    client: &Client,
    root: &Path,
    tiles: Vec<LidarTile>,
    concurrency: usize,
) -> Result<()> {
    let downloads = stream::iter(tiles).map(|tile| async move {
        let result = download_one(client, root, &tile).await;
        (tile, result)
    });
    let mut downloads = downloads.buffer_unordered(concurrency);
    let mut succeeded = 0;
    let mut failed = 0;
    while let Some((tile, result)) = downloads.next().await {
        match result {
            Ok(size) => {
                succeeded += 1;
                println!(
                    "[OK] {} ({size} octets)",
                    root.join(tile.relative_path).display()
                );
            }
            Err(error) => {
                failed += 1;
                eprintln!("[ERREUR] {} : {error:#}", tile.relative_path.display());
            }
        }
    }
    println!("Téléchargées   : {succeeded}");
    println!("Échecs         : {failed}");
    if failed > 0 {
        bail!("{failed} dalle(s) LiDAR n'ont pas pu être téléchargées");
    }
    Ok(())
}

/// Écrit une dalle temporaire, contrôle le transfert puis publie le fichier et son reçu.
async fn download_one(client: &Client, root: &Path, tile: &LidarTile) -> Result<u64> {
    // LAZ est déjà compressé. Demander identity évite une seconde compression HTTP.
    let mut response = client
        .get(&tile.url)
        .header("Accept-Encoding", "identity")
        .send()
        .await
        .context("requête de téléchargement LAZ")?
        .error_for_status()
        .context("erreur HTTP de téléchargement LAZ")?;
    let expected_size = response.content_length();
    let path = root.join(&tile.relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let temporary = path.with_extension("laz.part");
    let mut file = fs::File::create(&temporary).await?;
    let mut size = 0_u64;
    let mut signature = Vec::new();

    // chunk fournit le morceau suivant sans charger le fichier entier en mémoire.
    // En cas d'interruption, ? laisse le .part sans reçu : le cache ne le réutilisera pas.
    while let Some(chunk) = response.chunk().await.context("lecture du flux LAZ")? {
        if signature.len() < 4 {
            let count = (4 - signature.len()).min(chunk.len());
            signature.extend_from_slice(&chunk[..count]);
            if signature.len() == 4 && signature != b"LASF" {
                bail!("la réponse reçue n'est pas un fichier LAS/LAZ (signature LASF absente)");
            }
        }
        file.write_all(&chunk).await?;
        size += chunk.len() as u64;
    }
    // LAS et LAZ ont un en-tête LASF. Contrôler le transport ne décompresse pas les
    // points et ne remplace pas une validation complète du contenu par un lecteur LAZ.
    if size < 227 || signature != b"LASF" {
        bail!("fichier LAS/LAZ vide ou incomplet");
    }
    // La seconde condition est évaluée uniquement si une taille est annoncée.
    if let Some(expected) = expected_size
        && size != expected
    {
        bail!("transfert LAZ incomplet : {size} octets sur {expected}");
    }
    file.flush().await?;
    // Fermer le fichier avant rename est nécessaire sous Windows.
    drop(file);
    fs::rename(&temporary, &path).await?;
    cache::save_receipt(&path, &tile.url, size).await?;
    Ok(size)
}

// Serveur local contrôlé : ces tests vérifient le transfert et le cache sans appeler IGN.
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        path::PathBuf,
        thread,
    };

    fn serve(body: Vec<u8>, announced_size: usize) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                if socket.read(&mut byte).unwrap() == 0 {
                    break;
                }
                request.extend_from_slice(&byte);
            }
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {announced_size}\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            socket.write_all(&body).unwrap();
        });
        (format!("http://{address}/test.copc.laz"), server)
    }

    fn directory() -> PathBuf {
        crate::test_support::directory()
    }

    async fn cleanup(root: &Path) {
        crate::test_support::cleanup(root);
    }

    fn tile(url: String) -> LidarTile {
        LidarTile {
            id: "test".to_owned(),
            url,
            relative_path: PathBuf::from("edition/test.copc.laz"),
            properties: serde_json::json!({}),
        }
    }

    #[tokio::test]
    async fn completed_transfer_gets_a_receipt_and_is_reused_unless_forced() {
        // Corps synthétique : on teste le transport binaire, pas l'interprétation des points.
        let mut body = vec![0; 300];
        body[..4].copy_from_slice(b"LASF");
        let (url, server) = serve(body.clone(), body.len());
        let tile = tile(url);
        let root = directory();
        let client = Client::builder().no_proxy().build().unwrap();
        assert_eq!(download_one(&client, &root, &tile).await.unwrap(), 300);
        server.join().unwrap();
        let path = root.join(&tile.relative_path);
        assert_eq!(fs::read(&path).await.unwrap(), body);
        assert!(cache::is_complete(&path, &tile.url).await);
        assert!(!cache::is_complete(&path, "https://data.geopf.fr/another-edition.laz").await);
        let (missing, present) = cache::find_missing(&root, vec![tile], false).await;
        assert!(missing.is_empty());
        assert_eq!(present, 1);

        let tile = super::tests::tile("new-source".to_owned());
        cache::save_receipt(&path, &tile.url, 300).await.unwrap();
        let (missing, present) = cache::find_missing(&root, vec![tile], true).await;
        assert_eq!(missing.len(), 1);
        assert_eq!(present, 0);
        cleanup(&root).await;
    }

    #[tokio::test]
    async fn truncated_or_non_laz_responses_never_get_a_complete_file_or_receipt() {
        for (mut body, announced_size) in [(vec![0; 300], 400), (vec![0; 300], 300)] {
            if announced_size == 400 {
                body[..4].copy_from_slice(b"LASF");
            }
            let (url, server) = serve(body, announced_size);
            let tile = tile(url);
            let root = directory();
            let client = Client::builder().no_proxy().build().unwrap();
            assert!(download_one(&client, &root, &tile).await.is_err());
            server.join().unwrap();
            let path = root.join(&tile.relative_path);
            assert!(!path.exists());
            assert!(!cache::receipt_path(&path).exists());
            assert!(!cache::is_complete(&path, &tile.url).await);
            cleanup(&root).await;
        }
    }
}
