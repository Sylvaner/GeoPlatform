//! Contexte : une dalle LiDAR est un fichier de points 3D, pas une image WMTS.
//! Usage : lidar/catalogue.rs la décrit depuis le WFS ; lidar/download.rs la récupère.
//! Les propriétés d'origine gardent les dates d'acquisition, l'édition et les références.

use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;

/// Dalle de nuage de points classés, avec sa source et sa destination locale.
#[derive(Debug, Serialize)]
pub struct LidarTile {
    /// Identifiant de l'entité dans le catalogue IGN.
    pub id: String,
    /// Lien officiel vers le fichier .laz ou .copc.laz, utilisé sans le reconstruire.
    pub url: String,
    /// Chemin relatif au dossier LIDAR_HD : édition/nom.copc.laz.
    /// Le dossier d'édition évite de confondre deux livraisons de la même dalle.
    pub relative_path: PathBuf,
    /// Métadonnées d'origine, conservées sans inventer ni modifier leurs champs.
    pub properties: Value,
}
