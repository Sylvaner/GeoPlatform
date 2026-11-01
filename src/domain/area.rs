//! Contexte : zone géographique minimale demandée par un téléchargement ou un export.
//! Usage : le cas courant demande seulement un centre GPS et une taille en mètres.

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// Zone carrée exprimée par son centre et la longueur de son côté.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Area {
    /// Centre sous la forme latitude, longitude dans le cas courant.
    pub center: [f64; 2],
    /// Longueur du côté du carré en mètres.
    pub size_m: f64,
    /// Réglages réservés aux coordonnées projetées.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advanced: Option<AdvancedArea>,
}

/// Options rarement nécessaires, séparées du contrat principal.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AdvancedArea {
    /// Repère des coordonnées. La valeur par défaut est `EPSG:4326`.
    #[serde(default = "default_crs")]
    pub crs: String,
}

impl Area {
    /// Vérifie les limites qui évitent une requête ambiguë ou dangereusement grande.
    pub fn validate(&self) -> AppResult<()> {
        if self.center.iter().any(|value| !value.is_finite()) {
            return Err(AppError::validation(
                "le centre doit contenir deux nombres finis",
            ));
        }
        if !self.size_m.is_finite() || self.size_m <= 0.0 {
            return Err(AppError::validation("size_m doit être strictement positif"));
        }
        if self.is_wgs84()
            && (!(-90.0..=90.0).contains(&self.center[0])
                || !(-180.0..=180.0).contains(&self.center[1]))
        {
            return Err(AppError::validation(
                "un centre GPS doit respecter latitude [-90, 90] et longitude [-180, 180]",
            ));
        }
        Ok(())
    }

    /// Renvoie le CRS explicite, ou WGS84 quand aucun réglage avancé n'est fourni.
    pub fn crs(&self) -> &str {
        self.advanced
            .as_ref()
            .map(|advanced| advanced.crs.as_str())
            .unwrap_or("EPSG:4326")
    }

    /// Indique si le centre peut être interprété directement comme latitude et longitude.
    pub fn is_wgs84(&self) -> bool {
        matches!(
            self.crs().to_ascii_lowercase().as_str(),
            "epsg:4326" | "4326" | "wgs84" | "latlon" | "gps"
        )
    }
}

/// Fournit la valeur de sérialisation par défaut du CRS avancé.
fn default_crs() -> String {
    "EPSG:4326".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vérifie que le contrat court suffit au cas GPS habituel.
    #[test]
    fn simple_gps_area_is_valid() {
        let area = Area {
            center: [48.26, 4.73],
            size_m: 3_000.0,
            advanced: None,
        };
        assert!(area.validate().is_ok());
        assert_eq!(area.crs(), "EPSG:4326");
    }

    /// Vérifie que l'ordre latitude-longitude est contrôlé sans option supplémentaire.
    #[test]
    fn invalid_latitude_is_rejected() {
        let area = Area {
            center: [120.0, 4.73],
            size_m: 3_000.0,
            advanced: None,
        };
        assert!(area.validate().is_err());
    }
}
