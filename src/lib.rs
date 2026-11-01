//! Contexte : bibliothèque commune au serveur HTTP et aux deux outils en ligne de commande.
//! Usage : les binaires ne font que lire leurs arguments puis appellent les services de ce module.

pub mod api;
pub mod backend;
pub mod cache;
pub mod cli;
pub mod config;
pub mod dashboard;
pub mod domain;
pub mod error;
pub mod logs;
pub mod map;
pub mod presets;
pub mod preview;
pub mod road_repair;
pub mod service;
pub mod store;
pub mod workers;

pub use config::AppConfig;
pub use service::AppService;
