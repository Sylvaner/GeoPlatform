//! Contexte : ALL doit connaître les six jeux de données disponibles.
//! Usage : parcourir Dataset::ALL ; name fournit le nom affiché dans le bilan.
//! Ce type ne télécharge rien. Les appels aux services restent dans all.rs.

/// Une valeur de cette enum désigne un seul jeu de données, jamais la commande ALL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dataset {
    RgeAlti,
    BdOrtho,
    BdTopo,
    Cadastre,
    BdTopage,
    LidarHd,
}

impl Dataset {
    /// Ordre des téléchargements. Le LiDAR, plus volumineux, passe en dernier.
    pub const ALL: [Self; 6] = [
        Self::RgeAlti,
        Self::BdOrtho,
        Self::BdTopo,
        Self::Cadastre,
        Self::BdTopage,
        Self::LidarHd,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::RgeAlti => "RGE_ALTI",
            Self::BdOrtho => "BD_ORTHO",
            Self::BdTopo => "BD_TOPO",
            Self::Cadastre => "CADASTRE",
            Self::BdTopage => "BD_TOPAGE",
            Self::LidarHd => "LIDAR_HD",
        }
    }
}
