//! Contexte : une couche WFS est une famille d'objets vectoriels servis par l'IGN ou le Sandre.
//! Usage : définir son identifiant et son tri ici, puis la passer au service wfs.

/// Description simple d'une couche ; les textes constants vivent pendant tout le programme.
pub struct WfsLayer {
    pub identifier: &'static str,
    pub label: &'static str,
    /// Un tri stable est indispensable pour parcourir des pages successives.
    pub sort_by: &'static str,
    /// Chaque serveur annonce le nom exact de son format GeoJSON.
    /// IGN accepte application/json ; Sandre attend application/json; subtype=geojson.
    pub output_format: &'static str,
}
