//! Contexte : le cache conserve les tuiles et leur description sur le disque local.
//! Usage : `app.rs` prépare les dossiers et cherche les tuiles manquantes ;
//! `download.rs` appelle `write_tile` après réception des données.
//! Chemins : <produit>/<jeu de grilles>/<niveau>/<ligne>/<colonne>.<extension>.
//! Les structures utilisées ici sont définies séparément dans `models/`.

use anyhow::Result;
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::models::{CacheMetadata, CachedTile, TileMatrix, TileRange, WmtsLayer};

/// Couleur de repli utilisée uniquement là où IGN annonce explicitement l'absence d'imagerie.
pub(crate) const NO_DATA_RGB: [u8; 3] = [91, 126, 138];

/// Crée le dossier du produit sous le répertoire demandé et renvoie son chemin.
// &Path emprunte un chemin ; PathBuf est un chemin possédé que l'on peut renvoyer.
pub async fn prepare_root(root: &Path, product_directory: &str) -> Result<PathBuf> {
    // join construit un chemin adapté au système, sans concaténer des séparateurs.
    let cache_root = root.join(product_directory);
    fs::create_dir_all(&cache_root).await?;
    Ok(cache_root)
}

/// Enregistre le catalogue XML reçu du serveur pour faciliter le diagnostic.
pub async fn save_capabilities(cache_root: &Path, xml: &str) -> Result<()> {
    // Garder le XML aide à comprendre une éventuelle évolution du service IGN.
    fs::write(cache_root.join("wmts-capabilities.xml"), xml).await?;
    Ok(())
}

/// Crée le dossier du niveau et son fichier matrix.json.
pub async fn prepare_level(
    cache_root: &Path,
    layer: &WmtsLayer,
    matrix: &TileMatrix,
) -> Result<PathBuf> {
    // L'identifiant provient du serveur : retirer les caractères gênants pour un nom.
    let level_dir = cache_root
        .join(sanitize_filename(&matrix.matrix_set))
        .join(sanitize_filename(&matrix.identifier));
    fs::create_dir_all(&level_dir).await?;

    // Les & empruntent les valeurs existantes le temps de produire le JSON.
    let metadata = CacheMetadata {
        schema_version: 2,
        layer: &layer.identifier,
        tile_matrix_set: &matrix.matrix_set,
        crs: &matrix.crs,
        coordinate_order: if matrix.crs == "EPSG:4326" {
            "longitude_latitude"
        } else {
            "x_y"
        },
        format: &layer.format,
        style: &layer.style,
        matrix,
    };
    // to_vec_pretty produit les octets d'un JSON indenté, lisible dans un éditeur.
    let json = serde_json::to_vec_pretty(&metadata)?;
    fs::write(level_dir.join("matrix.json"), json).await?;

    Ok(level_dir)
}

/// Renvoie (tuiles à télécharger, nombre de tuiles déjà présentes).
/// Avec force=true, toutes les tuiles demandées sont placées dans la liste.
pub async fn find_missing_tiles(
    level_dir: &Path,
    range: TileRange,
    format: &str,
    force: bool,
) -> (Vec<CachedTile>, u64) {
    let extension = extension_for_format(format);
    // mut autorise les modifications ; Vec::new crée une liste vide.
    let mut missing = Vec::new();
    let mut already_present = 0;

    // ..= inclut la borne finale, contrairement à .. .
    for row in range.row_min..=range.row_max {
        for col in range.col_min..=range.col_max {
            let path = level_dir
                .join(row.to_string())
                .join(format!("{col}.{extension}"));
            // && évalue sa partie droite seulement si la gauche est vraie.
            // On ne consulte donc pas le disque lorsque force est activé.
            if !force
                && (is_valid_existing_file(&path).await
                    || is_valid_existing_file(&no_data_marker_path(&path)).await)
            {
                already_present += 1;
            } else {
                // L'écriture courte { row } signifie { row: row }.
                missing.push(CachedTile { row, col, path });
            }
        }
    }

    (missing, already_present)
}

/// Écrit les données dans un fichier temporaire, puis le renomme à sa destination.
// &[u8] est une vue empruntée sur une suite d'octets, sans copie des données.
pub async fn write_tile(path: &Path, bytes: &[u8]) -> Result<()> {
    // parent renvoie Option<&Path> : Some si un parent existe, sinon None.
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    // Un téléchargement interrompu ne doit pas apparaître comme une tuile complète.
    // Exemple : 12.bil.part devient 12.bil seulement après l'écriture.
    // and_then enchaîne les opérations pouvant ne rien renvoyer (Option).
    // unwrap_or fournit une valeur de secours, sans provoquer de panic.
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("tile");
    let temporary_path = path.with_extension(format!("{extension}.part"));
    fs::write(&temporary_path, bytes).await?;
    fs::rename(&temporary_path, path).await?;
    // Une donnée redevenue disponible remplace le cache négatif créé lors d'un 404.
    let _ = fs::remove_file(no_data_marker_path(path)).await;
    Ok(())
}

/// Mémorise une absence annoncée par le serveur afin de ne pas redemander la tuile à chaque zone.
pub async fn write_no_data_marker(path: &Path) -> Result<()> {
    let marker = no_data_marker_path(path);
    if let Some(parent) = marker.parent() {
        fs::create_dir_all(parent).await?;
    }
    fs::write(marker, b"wmts-no-data-v1\n").await?;
    Ok(())
}

/// Le marqueur garde l'extension d'origine dans son nom : `12.jpg.nodata`.
pub(crate) fn no_data_marker_path(path: &Path) -> PathBuf {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("tile");
    path.with_extension(format!("{extension}.nodata"))
}

pub(crate) fn is_no_data_tile(path: &Path) -> bool {
    no_data_marker_path(path).is_file()
}

/// Une tuile est présente si son fichier existe et n'est pas vide.
async fn is_valid_existing_file(path: &Path) -> bool {
    // Une erreur de lecture est traitée comme une absence : la tuile sera récupérée.
    match fs::metadata(path).await {
        Ok(metadata) => metadata.is_file() && metadata.len() > 0,
        Err(_) => false,
    }
}

/// Associe un type de contenu WMTS à une extension de fichier.
// &'static str désigne ici un texte littéral, disponible pendant tout le programme.
fn extension_for_format(format: &str) -> &'static str {
    let format = format.to_ascii_lowercase();
    if format.contains("bil") {
        "bil"
    } else if format.contains("tif") {
        "tif"
    } else if format.contains("png") {
        "png"
    } else if format.contains("jpeg") || format.contains("jpg") {
        "jpg"
    } else {
        "bin"
    }
}

/// Remplace par _ les caractères non retenus dans un nom de niveau.
fn sanitize_filename(value: &str) -> String {
    // chars parcourt le texte ; map transforme chaque caractère ; collect
    // rassemble les caractères transformés dans une nouvelle String.
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support as support;
    fn range() -> TileRange {
        TileRange {
            row_min: 1,
            row_max: 1,
            col_min: 2,
            col_max: 3,
        }
    }
    #[tokio::test]
    async fn missing_empty_and_partial_files_are_downloaded_again() {
        let root = support::directory();
        fs::create_dir_all(root.join("1")).await.unwrap();
        fs::write(root.join("1/2.bil"), []).await.unwrap();
        fs::write(root.join("1/3.bil.part"), [1, 2]).await.unwrap();
        let (missing, present) =
            find_missing_tiles(&root, range(), "image/x-bil;bits=32", false).await;
        assert_eq!(missing.len(), 2);
        assert_eq!(present, 0);
        support::cleanup(&root);
    }
    #[tokio::test]
    async fn force_bypasses_non_empty_tile_cache() {
        let root = support::directory();
        write_tile(&root.join("1/2.jpg"), &[1, 2]).await.unwrap();
        let (missing, present) = find_missing_tiles(&root, range(), "image/jpeg", false).await;
        assert_eq!(missing.len(), 1);
        assert_eq!(present, 1);
        let (missing, present) = find_missing_tiles(&root, range(), "image/jpeg", true).await;
        assert_eq!(missing.len(), 2);
        assert_eq!(present, 0);
        support::cleanup(&root);
    }
    #[tokio::test]
    async fn no_data_markers_are_cached_but_force_retries_them() {
        let root = support::directory();
        let path = root.join("1/2.jpg");
        write_no_data_marker(&path).await.unwrap();
        let (missing, present) = find_missing_tiles(&root, range(), "image/jpeg", false).await;
        assert_eq!(missing.len(), 1);
        assert_eq!(present, 1);
        let (missing, present) = find_missing_tiles(&root, range(), "image/jpeg", true).await;
        assert_eq!(missing.len(), 2);
        assert_eq!(present, 0);
        support::cleanup(&root);
    }
    #[tokio::test]
    async fn downloaded_tile_removes_its_old_no_data_marker() {
        let root = support::directory();
        let path = root.join("1/2.jpg");
        write_no_data_marker(&path).await.unwrap();
        write_tile(&path, &[1, 2, 3]).await.unwrap();
        assert!(path.is_file());
        assert!(!no_data_marker_path(&path).exists());
        support::cleanup(&root);
    }
    #[tokio::test]
    async fn atomic_write_creates_parents_and_removes_temporary_file() {
        let root = support::directory();
        let path = root.join("a/b/12.bil");
        write_tile(&path, &[1, 2, 3]).await.unwrap();
        assert_eq!(fs::read(&path).await.unwrap(), [1, 2, 3]);
        assert!(!path.with_extension("bil.part").exists());
        support::cleanup(&root);
    }
    #[tokio::test]
    async fn atomic_write_replaces_an_existing_file_on_windows() {
        let root = support::directory();
        let path = root.join("12.bil");
        write_tile(&path, &[1]).await.unwrap();
        write_tile(&path, &[2, 3]).await.unwrap();
        assert_eq!(fs::read(&path).await.unwrap(), [2, 3]);
        support::cleanup(&root);
    }
    #[test]
    fn content_types_choose_expected_extensions() {
        for (format, extension) in [
            ("image/x-bil;bits=32", "bil"),
            ("image/tiff", "tif"),
            ("image/jpeg", "jpg"),
            ("image/png", "png"),
            ("unknown", "bin"),
        ] {
            assert_eq!(extension_for_format(format), extension);
        }
    }
}
