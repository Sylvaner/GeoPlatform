//! Contexte : commande locale de téléchargement utilisant exactement le service de l'API.
//! Usage : aucun serveur séparé n'est nécessaire ; cache et workers restent identiques.

use clap::Parser;
use geo_platform::cli::{DownloadCli, run_download};

/// Exécute le job local puis imprime son manifeste JSON.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if let Some(arguments) = geo_platform::backend::engine_worker_arguments() {
        return geo_platform::backend::run_engine_worker(arguments).await;
    }
    let job = run_download(DownloadCli::parse()).await?;
    println!("{}", serde_json::to_string_pretty(&job)?);
    Ok(())
}
