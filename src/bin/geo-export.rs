//! Contexte : commande locale commune aux exports STL, STL par couches et Blender.
//! Usage : aucun serveur séparé n'est nécessaire ; les résultats rejoignent le même catalogue.

use clap::Parser;
use geo_platform::cli::{ExportCli, run_export};

/// Exécute le job local puis imprime ses fichiers en JSON.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if let Some(arguments) = geo_platform::backend::engine_worker_arguments() {
        return geo_platform::backend::run_engine_worker(arguments).await;
    }
    let job = run_export(ExportCli::parse()).await?;
    println!("{}", serde_json::to_string_pretty(&job)?);
    Ok(())
}
