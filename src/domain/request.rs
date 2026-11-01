//! Contexte : contrats simples de téléchargement et d'export.
//! Usage : les réglages avancés restent facultatifs et les valeurs courantes sont calculées.

use std::path::Path;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

use super::{Area, DataLayer};

/// Demande de mise en cache d'une ou plusieurs couches.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DownloadRequest {
    /// Nom humain de la zone, indépendant de la clé de cache des données.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub area: Area,
    /// Une liste absente ou vide signifie toutes les couches.
    #[serde(default)]
    pub layers: Vec<DataLayer>,
    /// Ignore le résultat final mis en cache, tout en laissant les moteurs réutiliser leurs tuiles.
    #[serde(default)]
    pub force: bool,
}

impl DownloadRequest {
    /// Valide la zone puis remplace une sélection vide par toutes les couches.
    pub fn normalized(mut self) -> AppResult<Self> {
        self.area.validate()?;
        if let Some(name) = self.name.take() {
            self.name = Some(validate_zone_name(name)?);
        }
        if self.layers.is_empty() {
            self.layers = DataLayer::ALL.to_vec();
        }
        self.layers.sort_unstable();
        self.layers.dedup();
        Ok(self)
    }

    /// Remplace uniquement le libellé visible sans modifier l'emprise ni le cache.
    pub fn rename(&mut self, name: String) -> AppResult<()> {
        self.name = Some(validate_zone_name(name)?);
        Ok(())
    }
}

/// Nettoie un nom court tout en refusant les valeurs vides ou démesurées.
fn validate_zone_name(name: String) -> AppResult<String> {
    let name = name.trim().to_owned();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(AppError::validation(
            "le nom de la zone doit contenir entre 1 et 80 caractères".to_owned(),
        ));
    }
    Ok(name)
}

/// Format produit par le worker d'export.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
#[clap(rename_all = "snake_case")]
pub enum ExportFormat {
    Stl,
    StlLayers,
    Blender,
}

impl ExportFormat {
    /// Extension attendue pour le nom de fichier final.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Stl | Self::StlLayers => "stl",
            Self::Blender => "blend",
        }
    }
}

/// Trois niveaux compréhensibles remplacent les réglages techniques dispersés.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
#[clap(rename_all = "snake_case")]
pub enum ExportQuality {
    Draft,
    #[default]
    Standard,
    High,
}

impl ExportQuality {
    /// Valeur reconnue par le moteur STL existant.
    pub fn legacy_stl_name(self) -> &'static str {
        match self {
            Self::Draft => "fast",
            Self::Standard => "balanced",
            Self::High => "full",
        }
    }

    /// Pas de grille adapté automatiquement à la qualité Blender.
    pub fn grid_step_m(self) -> f64 {
        match self {
            Self::Draft => 20.0,
            Self::Standard => 10.0,
            Self::High => 5.0,
        }
    }
}

/// Silhouette du feuillage utilisée pour les arbres imprimables.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
#[clap(rename_all = "snake_case")]
pub enum TreeShape {
    /// Houppier arrondi pour les feuillus.
    #[default]
    Deciduous,
    /// Cime conique étagée pour les sapins et autres conifères.
    Conifer,
    /// Surface continue interpolée depuis les cellules de canopée LiDAR.
    Canopy,
}

impl TreeShape {
    /// Traduit le choix produit vers le vocabulaire du moteur STL existant.
    pub fn legacy_name(self) -> &'static str {
        match self {
            Self::Deciduous => "rounded",
            Self::Conifer => "conifer",
            Self::Canopy => "rounded",
        }
    }

    /// Conserve le mode de canopée compris par notre finition Blender.
    pub fn blender_name(self) -> &'static str {
        match self {
            Self::Deciduous => "deciduous",
            Self::Conifer => "conifer",
            Self::Canopy => "canopy",
        }
    }
}

/// Étendue du plaquage des orthophotos dans une scène Blender.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
#[clap(rename_all = "snake_case")]
pub enum TextureMode {
    /// N'utilise aucune image et laisse les couleurs simples visibles.
    None,
    /// Plaque l'orthophoto uniquement sur le terrain.
    Ground,
    /// Plaque aussi l'orthophoto sur les faces de toit disponibles.
    #[default]
    GroundAndRoofs,
}

/// Organisation des images orthophoto dans le fichier Blender final.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
#[clap(rename_all = "snake_case")]
pub enum OrthophotoStorage {
    /// Regroupe les tuiles dans une seule image de définition réglable.
    #[default]
    Atlas,
    /// Conserve chaque JPEG IGN et sa qualité d'origine au prix d'une forte consommation mémoire.
    Native,
}

/// Aspect de la surface forestière continue dans une scène Blender.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
#[clap(rename_all = "snake_case")]
pub enum ForestMaterialMode {
    /// Projette l'orthophoto déjà intégrée dans le fichier Blender.
    Texture,
    /// Mélange deux verts avec un bruit procédural réglable dans Blender.
    #[default]
    Procedural,
}

/// Palette courte appliquée aux matériaux Blender non texturés.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExportColors {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ground: Option<[f64; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub walls: Option<[f64; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roofs: Option<[f64; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trees: Option<[f64; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roads: Option<[f64; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub water: Option<[f64; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forest_low: Option<[f64; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forest_high: Option<[f64; 4]>,
}

impl ExportColors {
    /// Refuse les composantes hors de l'intervalle RGBA compris par Blender.
    pub(crate) fn validate(&self) -> AppResult<()> {
        for (name, color) in [
            ("ground", self.ground),
            ("walls", self.walls),
            ("roofs", self.roofs),
            ("trees", self.trees),
            ("roads", self.roads),
            ("water", self.water),
            ("forest_low", self.forest_low),
            ("forest_high", self.forest_high),
        ] {
            if color.is_some_and(|values| {
                values
                    .iter()
                    .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
            }) {
                return Err(AppError::validation(format!(
                    "advanced.colors.{name} doit contenir quatre valeurs entre 0 et 1"
                )));
            }
        }
        Ok(())
    }

    /// Complète une palette partielle avec les valeurs centrales de la configuration.
    pub(crate) fn fill_missing_from(&mut self, fallback: &Self) {
        macro_rules! fill {
            ($field:ident) => {
                if self.$field.is_none() {
                    self.$field = fallback.$field;
                }
            };
        }
        fill!(ground);
        fill!(walls);
        fill!(roofs);
        fill!(trees);
        fill!(roads);
        fill!(water);
        fill!(forest_low);
        fill!(forest_high);
    }
}

/// Réglages d'impression rarement nécessaires au premier export.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AdvancedExport {
    /// Inclut les volumes de bâtiments lorsque la couche source est disponible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_buildings: Option<bool>,
    /// Inclut les chaussées lorsque la couche source est disponible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_roads: Option<bool>,
    /// Inclut les arbres sans désactiver le LiDAR utilisé par le terrain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_trees: Option<bool>,
    /// Largeur physique d'un STL. La valeur calculée est 150 mm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_mm: Option<f64>,
    /// Pas horizontal du terrain, réglable directement de 5 à 100 mètres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terrain_grid_step_m: Option<f64>,
    /// Épaisseur du socle. La valeur calculée est 3 mm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_mm: Option<f64>,
    /// Écart vertical réel entre les strates d'un STL par couches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer_height_m: Option<f64>,
    /// Exagération verticale appliquée aux exports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertical_exaggeration: Option<f64>,
    /// Largeur physique minimale d'une route imprimée.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub road_min_width_mm: Option<f64>,
    /// Relief physique d'une route au-dessus du terrain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub road_height_mm: Option<f64>,
    /// Surélévation visuelle des routes Blender au-dessus du terrain, en mètres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub road_elevation_m: Option<f64>,
    /// Hauteur physique minimale d'un arbre imprimé.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree_min_height_mm: Option<f64>,
    /// Amplification de la hauteur mesurée des arbres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree_height_scale: Option<f64>,
    /// Diamètre physique minimal d'un tronc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree_trunk_diameter_mm: Option<f64>,
    /// Diamètre physique minimal du feuillage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree_crown_diameter_mm: Option<f64>,
    /// Forme du feuillage, calculée comme feuillu lorsqu'elle est omise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree_shape: Option<TreeShape>,
    /// Finesse propre à la canopée LiDAR, indépendante du maillage du terrain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forest_quality: Option<ExportQuality>,
    /// Intensité du lissage de la canopée et de ses lisières, entre 0 et 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canopy_smoothing: Option<f64>,
    /// Matériau propre à la canopée Blender, sans modifier celui du terrain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forest_material_mode: Option<ForestMaterialMode>,
    /// Retire la canopée au droit des chaussées pour rendre les routes visibles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carve_forest_roads: Option<bool>,
    /// Conserve dans Blender les arbres ponctuels situés hors des massifs continus.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_isolated_trees: Option<bool>,
    /// Plaquage Blender ; le mode calculé utilise le terrain et les toits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture_mode: Option<TextureMode>,
    /// Regroupe par défaut les nombreuses tuiles orthophoto dans un atlas unique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orthophoto_storage: Option<OrthophotoStorage>,
    /// Largeur et hauteur maximales de l'atlas Blender.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orthophoto_resolution_px: Option<u32>,
    /// Ajoute de petites variations de matière sans charger d'images externes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub procedural_materials: Option<bool>,
    /// Couleurs Blender utilisées lorsque les orthophotos ne couvrent pas une surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colors: Option<ExportColors>,
}

impl AdvancedExport {
    /// Contrôle uniquement les options explicitement fournies.
    pub fn validate(&self) -> AppResult<()> {
        for (name, value) in [
            ("width_mm", self.width_mm),
            ("base_mm", self.base_mm),
            ("layer_height_m", self.layer_height_m),
            ("vertical_exaggeration", self.vertical_exaggeration),
            ("road_min_width_mm", self.road_min_width_mm),
            ("road_height_mm", self.road_height_mm),
            ("tree_min_height_mm", self.tree_min_height_mm),
            ("tree_height_scale", self.tree_height_scale),
            ("tree_trunk_diameter_mm", self.tree_trunk_diameter_mm),
            ("tree_crown_diameter_mm", self.tree_crown_diameter_mm),
        ] {
            if value.is_some_and(|value| !value.is_finite() || value <= 0.0) {
                return Err(AppError::validation(format!(
                    "advanced.{name} doit être strictement positif"
                )));
            }
        }
        if self
            .terrain_grid_step_m
            .is_some_and(|value| !value.is_finite() || !(5.0..=100.0).contains(&value))
        {
            return Err(AppError::validation(
                "advanced.terrain_grid_step_m doit être compris entre 5 et 100 mètres".to_owned(),
            ));
        }
        if self
            .road_elevation_m
            .is_some_and(|value| !value.is_finite() || !(0.0..=5.0).contains(&value))
        {
            return Err(AppError::validation(
                "advanced.road_elevation_m doit être compris entre 0 et 5 mètres".to_owned(),
            ));
        }
        if let Some(colors) = &self.colors {
            colors.validate()?;
        }
        if self
            .canopy_smoothing
            .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        {
            return Err(AppError::validation(
                "advanced.canopy_smoothing doit être compris entre 0 et 1".to_owned(),
            ));
        }
        if self
            .orthophoto_resolution_px
            .is_some_and(|value| !(512..=8192).contains(&value))
        {
            return Err(AppError::validation(
                "advanced.orthophoto_resolution_px doit être compris entre 512 et 8192 pixels"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

/// Demande d'export commune à STL, STL par couches et Blender.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExportRequest {
    pub area: Area,
    pub format: ExportFormat,
    /// Une liste absente ou vide laisse le format choisir ses couches utiles.
    #[serde(default)]
    pub layers: Vec<DataLayer>,
    #[serde(default)]
    pub quality: ExportQuality,
    /// Nom simple, sans chemin. Le serveur garde ainsi la maîtrise de son stockage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_name: Option<String>,
    #[serde(default)]
    pub advanced: AdvancedExport,
    /// Recalcule l'export final. Les caches intermédiaires LiDAR restent disponibles.
    #[serde(default)]
    pub force: bool,
}

impl ExportRequest {
    /// Valide les options puis rend l'ordre des couches déterministe pour le cache.
    pub fn normalized(mut self) -> AppResult<Self> {
        self.area.validate()?;
        self.advanced.validate()?;
        self.layers.sort_unstable();
        self.layers.dedup();
        if let Some(name) = &self.output_name {
            let path = Path::new(name);
            if path.file_name().and_then(|part| part.to_str()) != Some(name.as_str()) {
                return Err(AppError::validation(
                    "output_name doit être un nom de fichier sans répertoire",
                ));
            }
            if path.extension().and_then(|part| part.to_str()) != Some(self.format.extension()) {
                return Err(AppError::validation(format!(
                    "output_name doit se terminer par .{}",
                    self.format.extension()
                )));
            }
        } else {
            self.output_name = Some(self.generated_output_name());
        }
        Ok(self)
    }

    /// Retourne le nom validé ou en calcule un pour un appel interne non normalisé.
    pub fn resolved_output_name(&self) -> String {
        self.output_name
            .clone()
            .unwrap_or_else(|| self.generated_output_name())
    }

    /// Construit un nom court à partir de la zone et de huit caractères aléatoires.
    fn generated_output_name(&self) -> String {
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let layer = if self.format == ExportFormat::StlLayers {
            let compact = format!("{:.2}", self.advanced.layer_height_m.unwrap_or(1.0))
                .trim_end_matches('0')
                .trim_end_matches('.')
                .replace('.', "p");
            format!("_couche-{compact}m")
        } else {
            String::new()
        };
        format!(
            "export_{:.3}_{:.3}_{:.0}m{}_{}.{}",
            self.area.center[0],
            self.area.center[1],
            self.area.size_m,
            layer,
            &suffix[..8],
            self.format.extension()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Le nom calculé expose la zone sans réutiliser le générique `modele`.
    #[test]
    fn missing_output_name_uses_coordinates_size_and_random_suffix() {
        let request = ExportRequest {
            area: Area {
                center: [48.235_078, 4.701_290],
                size_m: 3_000.0,
                advanced: None,
            },
            format: ExportFormat::Blender,
            layers: Vec::new(),
            quality: ExportQuality::Standard,
            output_name: None,
            advanced: AdvancedExport::default(),
            force: false,
        }
        .normalized()
        .expect("requête valide");
        let name = request.output_name.expect("nom calculé");
        let suffix = name
            .strip_prefix("export_48.235_4.701_3000m_")
            .and_then(|value| value.strip_suffix(".blend"))
            .expect("structure du nom");

        assert_eq!(suffix.len(), 8);
        assert!(
            suffix
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        );
    }

    /// Un STL stratifié reste identifiable par sa hauteur de couche dans le fichier final.
    #[test]
    fn layered_stl_name_contains_layer_height() {
        let request = ExportRequest {
            area: Area {
                center: [45.771, 2.963],
                size_m: 2_000.0,
                advanced: None,
            },
            format: ExportFormat::StlLayers,
            layers: Vec::new(),
            quality: ExportQuality::Standard,
            output_name: None,
            advanced: AdvancedExport {
                layer_height_m: Some(0.25),
                ..AdvancedExport::default()
            },
            force: false,
        }
        .normalized()
        .expect("requête valide");

        assert!(
            request
                .output_name
                .expect("nom calculé")
                .contains("_couche-0p25m_")
        );
    }

    /// Un lissage hors de l'intervalle visible est refusé avant la mise en file.
    #[test]
    fn canopy_smoothing_is_limited_to_a_percentage() {
        let advanced = AdvancedExport {
            canopy_smoothing: Some(1.2),
            ..AdvancedExport::default()
        };

        assert!(advanced.validate().is_err());
    }

    /// La surélévation Blender accepte le terrain réel, mais refuse une valeur excessive.
    #[test]
    fn road_elevation_is_bounded_in_meters() {
        let real_height = AdvancedExport {
            road_elevation_m: Some(0.0),
            ..AdvancedExport::default()
        };
        let excessive = AdvancedExport {
            road_elevation_m: Some(5.1),
            ..AdvancedExport::default()
        };

        assert!(real_height.validate().is_ok());
        assert!(excessive.validate().is_err());
    }

    /// Un atlas 8K est accepté, sans laisser passer une valeur encore plus lourde.
    #[test]
    fn orthophoto_atlas_accepts_eight_kilopixels() {
        let eight_k = AdvancedExport {
            orthophoto_resolution_px: Some(8_192),
            ..AdvancedExport::default()
        };
        let excessive = AdvancedExport {
            orthophoto_resolution_px: Some(8_193),
            ..AdvancedExport::default()
        };

        assert!(eight_k.validate().is_ok());
        assert!(excessive.validate().is_err());
    }

    /// Le nom visible est nettoyé sans rendre le champ obligatoire pour les anciens clients API.
    #[test]
    fn download_zone_name_is_trimmed() {
        let request = DownloadRequest {
            name: Some("  Plateau central  ".to_owned()),
            area: Area {
                center: [48.0, 4.0],
                size_m: 1_000.0,
                advanced: None,
            },
            layers: vec![DataLayer::Lidar],
            force: false,
        }
        .normalized()
        .expect("nom valide");

        assert_eq!(request.name.as_deref(), Some("Plateau central"));
    }
}
