//! Contexte : seules la configuration et sa création sont des options du binaire.
//! Usage : tous les paramètres du rendu sont dans le TOML, pas dispersés dans la CLI.

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "geo-export",
    about = "Export hors ligne du cache géographique vers Blender"
)]
pub struct ExportArguments {
    /// Fichier TOML à lire. Ses chemins relatifs partent du dossier de ce fichier.
    #[arg(long, default_value = "export.toml")]
    pub config: PathBuf,
    /// Écrit tous les paramètres et leurs valeurs par défaut, sans lancer l'export.
    #[arg(long)]
    pub init_config: Option<PathBuf>,
}
