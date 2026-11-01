//! Contexte : représentation géographique légère pour la carte du futur front.
//! Usage : renvoyer l'emprise et les couches de chaque job sous forme de GeoJSON standard.

use serde_json::{Value, json};

use crate::{
    domain::{Area, Job, JobKind},
    error::{AppError, AppResult},
};

/// Transforme une liste de jobs GPS en collection de polygones GeoJSON.
pub fn jobs_geojson(jobs: &[Job]) -> AppResult<Value> {
    let features: AppResult<Vec<_>> = jobs.iter().map(job_feature).collect();
    Ok(json!({
        "type": "FeatureCollection",
        "features": features?,
    }))
}

/// Produit une emprise avec l'état et les couches utiles dans ses propriétés.
fn job_feature(job: &Job) -> AppResult<Value> {
    let (area, layers, kind, zone_name) = match &job.kind {
        JobKind::Download(request) => (
            &request.area,
            &request.layers,
            "download",
            request.name.as_deref(),
        ),
        JobKind::Export(request) => (&request.area, &request.layers, "export", None),
    };
    let coordinates = square_coordinates(area)?;
    Ok(json!({
        "type": "Feature",
        "id": job.id,
        "geometry": {
            "type": "Polygon",
            "coordinates": [coordinates],
        },
        "properties": {
            "job_id": job.id,
            "job_type": kind,
            "status": job.status,
            "center": area.center,
            "layers": layers,
            "size_m": area.size_m,
            "cache_hit": job.cache_hit,
            "zone_name": zone_name,
        }
    }))
}

/// Approxime le carré métrique en degrés autour d'un centre WGS84.
fn square_coordinates(area: &Area) -> AppResult<Vec<[f64; 2]>> {
    if !area.is_wgs84() {
        return Err(AppError::validation(
            "la carte GeoJSON nécessite pour l'instant une zone GPS EPSG:4326",
        ));
    }
    let latitude = area.center[0];
    let longitude = area.center[1];
    let half = area.size_m / 2.0;
    let latitude_delta = half / 111_320.0;
    let longitude_scale = latitude.to_radians().cos().abs().max(0.01);
    let longitude_delta = half / (111_320.0 * longitude_scale);
    Ok(vec![
        [longitude - longitude_delta, latitude - latitude_delta],
        [longitude + longitude_delta, latitude - latitude_delta],
        [longitude + longitude_delta, latitude + latitude_delta],
        [longitude - longitude_delta, latitude + latitude_delta],
        [longitude - longitude_delta, latitude - latitude_delta],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vérifie que la boucle GeoJSON est fermée dans l'ordre longitude-latitude.
    #[test]
    fn gps_square_is_closed() {
        let area = Area {
            center: [48.0, 4.0],
            size_m: 1_000.0,
            advanced: None,
        };
        let coordinates = square_coordinates(&area).expect("carré");
        assert_eq!(coordinates.len(), 5);
        assert_eq!(coordinates.first(), coordinates.last());
    }
}
