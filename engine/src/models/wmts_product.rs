//! Contexte : l'altitude et BD ORTHO suivent le même protocole de tuiles WMTS.
//! Usage : main.rs choisit un produit ; app.rs réutilise les mêmes étapes de cache.
//! Ce fichier ne contient que les différences entre produits : couche, dossier et format.

/// Produits accessibles par le service WMTS. Le LiDAR utilise un catalogue distinct.
#[derive(Debug, Clone, Copy)]
pub enum WmtsProduct {
    RgeAlti,
    BdOrtho,
}

impl WmtsProduct {
    /// Nom du répertoire créé dans la racine choisie avec --root.
    pub fn directory(self) -> &'static str {
        match self {
            Self::RgeAlti => "RGE_ALTI",
            Self::BdOrtho => "BD_ORTHO",
        }
    }

    /// Identifiant officiel à retrouver dans GetCapabilities et à envoyer à GetTile.
    pub fn layer_id(self) -> &'static str {
        match self {
            Self::RgeAlti => "ELEVATION.ELEVATIONGRIDCOVERAGE.HIGHRES",
            Self::BdOrtho => "HR.ORTHOIMAGERY.ORTHOPHOTOS",
        }
    }

    /// Format souhaité : valeurs numériques pour le relief, photographie JPEG pour l'ortho.
    pub fn preferred_format(self) -> &'static str {
        match self {
            Self::RgeAlti => "bil",
            Self::BdOrtho => "jpeg",
        }
    }
}
