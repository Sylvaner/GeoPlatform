//! Contexte : déroulement commun des commandes RGE_ALTI et BD_ORTHO.
//! Usage : `main.rs` appelle `run` avec les options lues dans le terminal.
//! Lire cette fonction de haut en bas, puis ouvrir coordinates, geometry, wmts, cache ou download
//! pour comprendre les détails d'une étape. Les données sont définies dans models/.

use anyhow::Result;

use crate::{
    cache,
    cli::DownloadOptions,
    coordinates, download, geometry, http,
    models::{Bbox, WmtsProduct},
    wmts,
};

/// Prépare la zone, découvre les tuiles disponibles et complète le cache local.
// options est passé par valeur : cette fonction en devient propriétaire.
// async permet d'attendre le réseau ou le disque sans bloquer le moteur Tokio.
pub async fn run(options: DownloadOptions, product: WmtsProduct) -> Result<()> {
    // ? continue en cas de succès et renvoie immédiatement toute erreur à main.
    options.validate()?;

    // 1. Convertir le centre avant de construire le carré : sa taille est en mètres.
    // &options.crs prête le texte à normalize_crs sans le retirer des options.
    let source_crs = coordinates::normalize_crs(&options.crs)?;
    // Le tuple renvoyé (deux nombres) est décomposé en deux variables.
    let (center_x, center_y) =
        coordinates::to_lambert93(options.center[0], options.center[1], &options.crs)?;
    let bbox = Bbox::square(center_x, center_y, options.side);

    println!(
        "Centre source : {}, {} ({} ; saisie {})",
        options.center[0], options.center[1], source_crs, options.crs
    );
    println!("Centre L93    : {:.3}, {:.3}", center_x, center_y);
    println!(
        "Emprise L93   : {:.3},{:.3},{:.3},{:.3}",
        bbox.xmin, bbox.ymin, bbox.xmax, bbox.ymax
    );

    // 2. Lire le catalogue du serveur pour connaître les formats et la grille.
    // Le client HTTP est créé une fois et réutilisé pour toutes les requêtes.
    let client = http::create_client()?;
    let cache_root = cache::prepare_root(&options.root, product.directory()).await?;
    // .await? attend le résultat puis propage son éventuelle erreur.
    let capabilities = wmts::get_capabilities(&client).await?;
    cache::save_capabilities(&cache_root, &capabilities).await?;
    let (layer, matrix) = wmts::parse_capabilities(&capabilities, product)?;

    println!("Produit       : {}", product.directory());
    println!("Couche        : {}", layer.identifier);
    println!("TMS           : {}", matrix.matrix_set);
    println!("CRS des tuiles: {}", matrix.crs);
    println!("Matrice       : {}", matrix.identifier);
    // L'échelle nominale WMTS ne doit pas être présentée comme une mesure locale.
    // Le niveau le plus fin du service n'est pas nécessairement celui du produit source.
    println!(
        "Échelle WMTS  : {:.6} m/pixel (nominale)",
        matrix.nominal_resolution_m_per_px
    );
    if matrix.crs == "EPSG:4326" {
        println!(
            "Pas des tuiles: {:.12} degrés/pixel",
            matrix.resolution_units_per_px
        );
    }
    println!("Niveau choisi : le plus fin disponible ; tuiles conservées telles que diffusées");
    println!("Format        : {}", layer.format);

    // 3. Le carré reste défini en mètres, mais les indices se calculent dans le
    // système de la grille : en degrés pour WGS84G. Diviser des mètres Lambert
    // par un pas angulaire donnerait des indices faux, même si le serveur répondait.
    let grid_bbox = coordinates::bbox_from_lambert93(bbox, &matrix.crs)?;
    println!(
        "Emprise grille: {:.9},{:.9},{:.9},{:.9}",
        grid_bbox.xmin, grid_bbox.ymin, grid_bbox.xmax, grid_bbox.ymax
    );
    let range = geometry::tile_range(grid_bbox, &matrix)?;
    println!(
        "Tuiles        : {} (cols {}..{}, rows {}..{})",
        range.count(),
        range.col_min,
        range.col_max,
        range.row_min,
        range.row_max
    );

    // 4. Écrire les métadonnées, puis comparer les tuiles demandées au cache.
    // find_missing_tiles renvoie une liste et le nombre de tuiles déjà présentes.
    let level_dir = cache::prepare_level(&cache_root, &layer, &matrix).await?;
    let (missing, already_present) =
        cache::find_missing_tiles(&level_dir, range, &layer.format, options.force).await;

    println!("Déjà en cache : {}", already_present);
    println!("À télécharger  : {}", missing.len());

    // 5. En simulation, les étapes précédentes ont lieu, mais pas GetTile.
    // Ok(()) termine la fonction avec un succès sans valeur supplémentaire.
    if options.dry_run || missing.is_empty() {
        return Ok(());
    }

    // La liste missing est transférée et consommée par download_tiles.
    // Les paramètres précédés de & sont seulement prêtés pendant cet appel.
    download::download_tiles(&client, &layer, &matrix, missing, options.concurrency).await
}
