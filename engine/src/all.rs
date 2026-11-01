//! Contexte : ALL enchaîne les six téléchargements sur une seule zone.
//! Usage : `cargo run -- ALL --center latitude longitude --side mètres`.
//! Les produits passent l'un après l'autre, avec les mêmes options et leurs caches
//! habituels. Un échec n'empêche pas d'essayer les suivants ; le bilan final indique
//! les erreurs et le programme renvoie alors un code d'échec. Relancer réutilise le cache.

use anyhow::{Result, bail};
use std::future::Future;

use crate::{
    app, bdtopo, cadastre,
    cli::DownloadOptions,
    coordinates, lidar,
    models::{Dataset, WmtsProduct},
    topage,
};

/// Point d'entrée de la commande : utiliser les vrais téléchargeurs existants.
pub async fn run(options: DownloadOptions) -> Result<()> {
    run_with(options, download_one).await
}

/// Oriente un produit vers son service. ALL ne change pas sa façon de télécharger.
async fn download_one(product: Dataset, options: DownloadOptions) -> Result<()> {
    match product {
        Dataset::RgeAlti => app::run(options, WmtsProduct::RgeAlti).await,
        Dataset::BdOrtho => app::run(options, WmtsProduct::BdOrtho).await,
        Dataset::BdTopo => bdtopo::run(options).await,
        Dataset::Cadastre => cadastre::run(options).await,
        Dataset::BdTopage => topage::run(options).await,
        Dataset::LidarHd => lidar::run(options).await,
    }
}

/// Enchaînement commun, testable sans Internet ni gros fichiers LiDAR.
/// Le paramètre download est une fonction : en production c'est download_one ;
/// les tests fournissent un remplaçant qui mémorise les appels et simule les erreurs.
async fn run_with<F, Job>(options: DownloadOptions, mut download: F) -> Result<()>
where
    // FnMut signifie que la fonction peut modifier son état (utile pour les tests).
    F: FnMut(Dataset, DownloadOptions) -> Job,
    // Une fonction async produit un Future ; .await permet d'obtenir son Result.
    Job: Future<Output = Result<()>>,
{
    // Refuser une saisie invalide avant de créer un dossier ou de lancer un produit.
    options.validate()?;
    coordinates::to_lambert93(options.center[0], options.center[1], &options.crs)?;
    let mut failures = Vec::new();
    for product in Dataset::ALL {
        println!("\n=== ALL : {} ===", product.name());
        // Chaque service prend possession de ses options. clone donne à chacun
        // une copie indépendante, sans modifier le centre, la racine ou les flags.
        match download(product, options.clone()).await {
            Ok(()) => println!("[OK ALL] {}", product.name()),
            Err(error) => {
                let detail = format!("{} : {error:#}", product.name());
                eprintln!("[ERREUR ALL] {detail}");
                failures.push(detail);
            }
        }
    }
    println!(
        "\nBilan ALL : {} réussite(s), {} échec(s)",
        Dataset::ALL.len() - failures.len(),
        failures.len()
    );
    if !failures.is_empty() {
        // Ne pas renvoyer un succès trompeur après un téléchargement partiel.
        bail!("ALL incomplet :\n{}", failures.join("\n"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
