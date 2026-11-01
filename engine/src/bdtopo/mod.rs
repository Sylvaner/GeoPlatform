//! Contexte : BD TOPO décrit les bâtiments et les infrastructures sous forme d'objets.
//! Usage : `cargo run -- BD_TOPO --center latitude longitude --side mètres`.
//! Chaque couche est téléchargée en GeoJSON Lambert-93 via le WFS officiel IGN.
//! Les attributs et les coordonnées, y compris Z si fourni, restent ceux du service.
//! Les canalisations de BD TOPO ne constituent pas un inventaire exhaustif des réseaux enterrés.

use crate::{cli::DownloadOptions, models::WfsLayer, vector, wfs};
use anyhow::Result;

/// Périmètre explicite : bâtiments, transports, ouvrages et réseaux décrits par BD TOPO.
/// `cleabs` est l'identifiant stable des objets BD TOPO ; il ordonne la pagination.
pub const LAYERS: &[WfsLayer] = &[
    WfsLayer {
        identifier: "BDTOPO_V3:batiment",
        label: "Bâtiments",
        sort_by: "cleabs A",
        output_format: "application/json",
    },
    WfsLayer {
        identifier: "BDTOPO_V3:troncon_de_route",
        label: "Routes et chemins",
        sort_by: "cleabs A",
        output_format: "application/json",
    },
    WfsLayer {
        identifier: "BDTOPO_V3:troncon_de_voie_ferree",
        label: "Voies ferrées",
        sort_by: "cleabs A",
        output_format: "application/json",
    },
    WfsLayer {
        identifier: "BDTOPO_V3:construction_lineaire",
        label: "Ouvrages linéaires",
        sort_by: "cleabs A",
        output_format: "application/json",
    },
    WfsLayer {
        identifier: "BDTOPO_V3:construction_ponctuelle",
        label: "Ouvrages ponctuels",
        sort_by: "cleabs A",
        output_format: "application/json",
    },
    WfsLayer {
        identifier: "BDTOPO_V3:construction_surfacique",
        label: "Ouvrages surfaciques",
        sort_by: "cleabs A",
        output_format: "application/json",
    },
    WfsLayer {
        identifier: "BDTOPO_V3:equipement_de_transport",
        label: "Équipements de transport",
        sort_by: "cleabs A",
        output_format: "application/json",
    },
    WfsLayer {
        identifier: "BDTOPO_V3:ligne_electrique",
        label: "Lignes électriques",
        sort_by: "cleabs A",
        output_format: "application/json",
    },
    WfsLayer {
        identifier: "BDTOPO_V3:pylone",
        label: "Pylônes",
        sort_by: "cleabs A",
        output_format: "application/json",
    },
    WfsLayer {
        identifier: "BDTOPO_V3:poste_de_transformation",
        label: "Postes de transformation",
        sort_by: "cleabs A",
        output_format: "application/json",
    },
    WfsLayer {
        identifier: "BDTOPO_V3:canalisation",
        label: "Canalisations répertoriées",
        sort_by: "cleabs A",
        output_format: "application/json",
    },
    WfsLayer {
        identifier: "BDTOPO_V3:reservoir",
        label: "Réservoirs",
        sort_by: "cleabs A",
        output_format: "application/json",
    },
];

/// Le parcours commun est expliqué dans vector.rs ; seules les couches sont propres à BD TOPO.
pub async fn run(options: DownloadOptions) -> Result<()> {
    vector::run(options, "BD_TOPO", wfs::URL, LAYERS).await
}
