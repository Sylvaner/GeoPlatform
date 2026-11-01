//! Contexte : processus HTTP qui commande les files de téléchargement et d'export.
//! Usage : `geo-server --config geo.toml`, puis arrêter proprement avec Ctrl+C.

use std::path::PathBuf;

use clap::Parser;
use geo_platform::{AppConfig, AppService, api};
use tracing_subscriber::EnvFilter;

/// Arguments strictement nécessaires au démarrage du serveur.
#[derive(Debug, Parser)]
#[command(
    name = "geo-server",
    about = "API et workers de la plateforme géographique"
)]
struct Arguments {
    /// Fichier de configuration court.
    #[arg(long, default_value = "geo.toml")]
    config: PathBuf,
}

/// Charge la configuration, démarre les workers puis sert l'API.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if let Some(arguments) = geo_platform::backend::engine_worker_arguments() {
        return geo_platform::backend::run_engine_worker(arguments).await;
    }
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let arguments = Arguments::parse();
    let config = AppConfig::load(&arguments.config)?;
    let address = config.server.bind;
    let service = AppService::start(config).await?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!(%address, "API prête");
    axum::serve(listener, api::router(service))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Attend Ctrl+C afin de laisser Axum terminer les réponses actives.
async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "impossible d'écouter Ctrl+C");
    }
}
