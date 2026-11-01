//! Contexte : une enum énumère les actions possibles du programme.
//! Usage : clap reconnaît le produit demandé ; main.rs lance son traitement.
//! Ajouter une autre commande commence par ajouter une variante ici.

use clap::Subcommand;

use super::DownloadOptions;

// La variante RgeAlti contient les options propres à cette commande.
// Clap expose le nom RGE_ALTI et accepte aussi les deux alias indiqués ci-dessous.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Télécharge les six jeux de données sur la même zone, LiDAR compris.
    #[command(name = "ALL", alias = "all")]
    All(DownloadOptions),
    /// Télécharge les parcelles cadastrales avec leurs références et attributs.
    #[command(
        name = "CADASTRE",
        alias = "PARCELLES",
        alias = "cadastre",
        alias = "parcelles"
    )]
    Cadastre(DownloadOptions),
    /// Télécharge les huit couches hydrographiques BD TOPAGE 2026 de métropole.
    #[command(
        name = "BD_TOPAGE",
        alias = "TOPAGE",
        alias = "bd-topage",
        alias = "bd_topage"
    )]
    BdTopage(DownloadOptions),
    /// Télécharge bâtiments, routes, voies ferrées, ouvrages et réseaux BD TOPO.
    #[command(name = "BD_TOPO", alias = "bd-topo", alias = "bd_topo")]
    BdTopo(DownloadOptions),
    /// Télécharge dans ./RGE_ALTI les tuiles RGE ALTI couvrant un carré.
    #[command(name = "RGE_ALTI", alias = "rge-alti", alias = "rge_alti")]
    RgeAlti(DownloadOptions),
    /// Télécharge les photographies aériennes BD ORTHO sous forme de tuiles JPEG.
    #[command(
        name = "BD_ORTHO",
        alias = "ORTHO",
        alias = "bd-ortho",
        alias = "bd_ortho"
    )]
    BdOrtho(DownloadOptions),
    /// Télécharge les dalles de nuages de points LiDAR HD classés (.copc.laz).
    #[command(
        name = "LIDAR_HD",
        alias = "LIDAR",
        alias = "lidar-hd",
        alias = "lidar_hd"
    )]
    LidarHd(DownloadOptions),
}
