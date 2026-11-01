//! Contexte : un grand triangle reliant seulement les rives peut traverser le relief du lac.
//! Usage : découper l'eau sur les mêmes triangles que le terrain et l'orthophoto.
//! Chaque morceau est ensuite légèrement surélevé ; aucun pixel de texture n'est modifié.

use super::models::{ExportConfig, LayerConfig, ObjectPlan, TerrainGrid};
use anyhow::{Result, bail};
use geo::{BooleanOps, BoundingRect, Coord, MultiPolygon, Polygon, TriangulateEarcut};
use serde_json::{Value, json};

/// Un plan d'eau décrit la surface physique prioritaire ; les surfaces élémentaires
/// complètent ensuite les zones absentes. Deux sources ne doivent pas dessiner deux
/// faces au même endroit : cela produit du z-fighting même au-dessus de la photo.
pub fn priority(key: &str) -> u8 {
    match key {
        "bd_topage.PlanEau" => 0,
        "bd_topage.SurfaceElementaire" => 1,
        "bd_topage.CoursEau" => 2,
        "bd_topage.TronconHydrographique" => 3,
        _ => 4,
    }
}

/// Conserver les morceaux non représentés, sans modifier le GeoJSON du cache.
/// Les trous restent des trous : une île n'est jamais ajoutée à la couverture d'eau.
pub fn uncovered(
    feature: &Value,
    covered: &mut MultiPolygon<f64>,
    t: &TerrainGrid,
) -> Result<Option<Value>> {
    let geometry = &feature["geometry"];
    let coordinates = &geometry["coordinates"];
    if coordinates.as_array().is_some_and(|items| items.is_empty()) {
        return Ok(None);
    }
    let polygons = match geometry["type"].as_str() {
        Some("Polygon") => vec![super::geometry::polygon(coordinates)?],
        Some("MultiPolygon") => coordinates
            .as_array()
            .into_iter()
            .flatten()
            .map(super::geometry::polygon)
            .collect::<Result<Vec<_>>>()?,
        // Les axes et les points ne sont pas des surfaces à retrancher.
        _ => return Ok(Some(feature.clone())),
    };
    let bounds = super::geometry::bounds(t);
    let mut source = MultiPolygon(Vec::new());
    for polygon in polygons {
        source = source.union(&polygon.intersection(&bounds));
    }
    let remaining = source.difference(covered);
    *covered = covered.union(&source);
    if remaining.0.is_empty() {
        return Ok(None);
    }
    let mut result = feature.clone();
    result["geometry"] = json!({"type":"MultiPolygon", "coordinates": remaining.0.iter().map(|polygon| {
        std::iter::once(polygon.exterior()).chain(polygon.interiors())
            .map(|ring|ring.0.iter().map(|p|[p.x,p.y]).collect::<Vec<_>>()).collect::<Vec<_>>()
    }).collect::<Vec<_>>()});
    Ok(Some(result))
}

pub fn is_water(key: &str) -> bool {
    matches!(
        key,
        "bd_topage.PlanEau"
            | "bd_topage.SurfaceElementaire"
            | "bd_topage.CoursEau"
            | "bd_topage.TronconHydrographique"
    )
}

pub fn add_surface(
    out: &mut ObjectPlan,
    polygon: &Polygon<f64>,
    t: &TerrainGrid,
    style: &LayerConfig,
    c: &ExportConfig,
) -> Result<()> {
    let Some(bounds) = polygon.bounding_rect() else {
        return Ok(());
    };
    let cell = |value: f64, origin: f64| {
        (((value - origin) / t.step()).floor().max(0.) as usize).min(t.count - 2)
    };
    // Les décalages des couches terrain/orthophoto sont appliqués dans Blender,
    // après l'exagération verticale. La garde doit donc être calculée dans ce repère.
    let mut offset = style.z_offset_m * t.exaggeration;
    for name in ["terrain", "orthophoto"] {
        if c.layers[name].enabled {
            offset = offset.max(c.layers[name].z_offset_m + c.water.surface_clearance_m);
        }
    }
    for row in cell(bounds.min().y, t.ymin)..=cell(bounds.max().y, t.ymin) {
        for col in cell(bounds.min().x, t.xmin)..=cell(bounds.max().x, t.xmin) {
            let x = t.xmin + col as f64 * t.step();
            let y = t.ymin + row as f64 * t.step();
            let sw = Coord { x, y };
            let se = Coord { x: x + t.step(), y };
            let ne = Coord {
                x: x + t.step(),
                y: y + t.step(),
            };
            let nw = Coord { x, y: y + t.step() };
            for corners in [[sw, se, ne, sw], [sw, ne, nw, sw]] {
                let triangle = Polygon::new(corners.to_vec().into(), Vec::new());
                for part in polygon.intersection(&triangle).0 {
                    let raw = part.earcut_triangles_raw();
                    for indices in raw.triangle_indices.chunks_exact(3) {
                        if out.vertices.len() + 3 > c.water.max_vertices {
                            bail!(
                                "surface d'eau trop dense : augmenter water.max_vertices ou terrain.grid_step_m"
                            );
                        }
                        let start = out.vertices.len();
                        for &index in indices {
                            let [x, y] = raw.vertices[index];
                            let mut point = t.local(x, y, t.mesh_height(x, y));
                            point[2] += offset;
                            out.vertices.push(point);
                        }
                        // Normales toujours vers le haut : l'épaisseur Blender
                        // se développe vers le bas et ne déplace jamais la surface.
                        let [a, b, d] = [
                            out.vertices[start],
                            out.vertices[start + 1],
                            out.vertices[start + 2],
                        ];
                        let upward =
                            (b[0] - a[0]) * (d[1] - a[1]) - (b[1] - a[1]) * (d[0] - a[0]) > 0.;
                        out.faces.push(if upward {
                            vec![start, start + 1, start + 2]
                        } else {
                            vec![start, start + 2, start + 1]
                        });
                    }
                }
            }
        }
    }
    out.properties["export_eau"] = serde_json::json!({
        "methode": "découpe sur les triangles du terrain",
        "terrain_offset_m": offset,
        "thickness_m": c.water.thickness_m,
    });
    Ok(())
}
