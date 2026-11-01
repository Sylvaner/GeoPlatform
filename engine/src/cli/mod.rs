//! Contexte : ce module décrit l'interface de commande utilisée dans le terminal.
//! Usage : `main.rs` appelle `Cli::parse()`, puis transmet les options à `app.rs`.
//! Chaque type de l'interface est défini dans son propre fichier.

mod arguments;
mod command;
mod download_options;

// Ces réexportations rendent accessibles les types via `crate::cli::NomDuType`.
pub use arguments::Cli;
pub use command::Command;
pub use download_options::DownloadOptions;
