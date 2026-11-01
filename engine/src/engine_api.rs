//! Entrées stables du moteur pour l'orchestrateur de l'application.

use anyhow::{Result, bail};
use clap::Parser;
use std::ffi::OsString;

use crate::{
    all, app, bdtopo, cadastre,
    cli::{Cli, Command, DownloadOptions},
    lidar,
    models::WmtsProduct,
    topage,
};

/// Télécharge un produit historique avec son cache et ses options natifs.
pub async fn download(dataset: &str, options: DownloadOptions) -> Result<()> {
    match dataset {
        "RGE_ALTI" => app::run(options, WmtsProduct::RgeAlti).await,
        "BD_ORTHO" => app::run(options, WmtsProduct::BdOrtho).await,
        "LIDAR_HD" => lidar::run(options).await,
        "BD_TOPO" => bdtopo::run(options).await,
        "CADASTRE" => cadastre::run(options).await,
        "BD_TOPAGE" => topage::run(options).await,
        unknown => bail!("produit géographique inconnu : {unknown}"),
    }
}

/// Exécute une commande de téléchargement avec les options CLI natives du moteur.
pub async fn download_cli(arguments: Vec<OsString>) -> Result<()> {
    let values = std::iter::once(OsString::from("geo-cache")).chain(arguments);
    let parsed = Cli::try_parse_from(values)?;
    match parsed.command {
        Command::All(options) => all::run(options).await,
        Command::RgeAlti(options) => app::run(options, WmtsProduct::RgeAlti).await,
        Command::BdOrtho(options) => app::run(options, WmtsProduct::BdOrtho).await,
        Command::LidarHd(options) => lidar::run(options).await,
        Command::BdTopo(options) => bdtopo::run(options).await,
        Command::Cadastre(options) => cadastre::run(options).await,
        Command::BdTopage(options) => topage::run(options).await,
    }
}
