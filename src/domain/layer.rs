//! Contexte : liste courte des couches exposées aux utilisateurs.
//! Usage : chaque couche masque les noms techniques des produits et services sources.

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// Couche fonctionnelle téléchargeable et exportable.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
#[clap(rename_all = "snake_case")]
pub enum DataLayer {
    Elevation,
    Orthophoto,
    Lidar,
    Buildings,
    Transport,
    Cadastre,
    Hydrography,
}

impl DataLayer {
    /// Ordre stable utilisé quand l'utilisateur demande toutes les couches.
    pub const ALL: [Self; 7] = [
        Self::Elevation,
        Self::Orthophoto,
        Self::Lidar,
        Self::Buildings,
        Self::Transport,
        Self::Cadastre,
        Self::Hydrography,
    ];

    /// Nom de la commande historique qui fournit cette couche.
    pub fn legacy_dataset(self) -> &'static str {
        match self {
            Self::Elevation => "RGE_ALTI",
            Self::Orthophoto => "BD_ORTHO",
            Self::Lidar => "LIDAR_HD",
            Self::Buildings | Self::Transport => "BD_TOPO",
            Self::Cadastre => "CADASTRE",
            Self::Hydrography => "BD_TOPAGE",
        }
    }

    /// Nom lisible destiné au dashboard.
    pub fn label(self) -> &'static str {
        match self {
            Self::Elevation => "Relief",
            Self::Orthophoto => "Orthophoto",
            Self::Lidar => "LiDAR HD",
            Self::Buildings => "Bâtiments",
            Self::Transport => "Transports",
            Self::Cadastre => "Cadastre",
            Self::Hydrography => "Hydrographie",
        }
    }
}
