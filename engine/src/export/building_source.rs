//! Contexte : les Z des contours BD TOPO sont parfois plus précis qu'une simple hauteur.
//! Usage : les conserver si toutes les altitudes de toit nécessaires sont cohérentes.
//! Le LiDAR peut ensuite remplacer cette surface lorsqu'il apporte des mesures assez denses.

use super::{
    geometry,
    models::{ExportConfig, ObjectPlan, SourceRoofTriangle, TerrainGrid},
};
use anyhow::Result;
use geo::TriangulateEarcut;
use serde_json::Value;
use std::collections::BTreeSet;

pub fn apply(
    feature: &Value,
    out: &mut ObjectPlan,
    t: &TerrainGrid,
    c: &ExportConfig,
) -> Result<bool> {
    if !c.buildings.use_bd_topo_altitudes {
        return Ok(false);
    }
    let g = &feature["geometry"];
    let coords = &g["coordinates"];
    let polygons: Vec<&Value> = if g["type"] == "Polygon" {
        vec![coords]
    } else {
        coords
            .as_array()
            .map(|a| a.iter().collect())
            .unwrap_or_default()
    };
    let mut triangles = Vec::new();
    for polygon in polygons {
        let p = geometry::polygon(polygon)?;
        let mut points = Vec::new();
        for ring in polygon.as_array().into_iter().flatten() {
            for point in ring.as_array().into_iter().flatten() {
                let (Some(x), Some(y), Some(z)) =
                    (point[0].as_f64(), point[1].as_f64(), point[2].as_f64())
                else {
                    return Ok(false);
                };
                let h = z - t.height(x, y)?;
                if !z.is_finite() || h < c.buildings.min_height_m || h > c.buildings.max_height_m {
                    return Ok(false);
                }
                points.push([x, y, z]);
            }
        }
        let raw = p.earcut_triangles_raw();
        for ids in raw.triangle_indices.chunks_exact(3) {
            let mut values = [[0.; 3]; 3];
            for i in 0..3 {
                let xy = raw.vertices[ids[i]];
                let Some(point) = points
                    .iter()
                    .find(|p| (p[0] - xy[0]).hypot(p[1] - xy[1]) < 1e-6)
                else {
                    return Ok(false);
                };
                values[i] = *point;
            }
            triangles.push(SourceRoofTriangle { points: values });
        }
    }
    let mut top = BTreeSet::new();
    let roof_faces: BTreeSet<_> = out.roof_faces.iter().copied().collect();
    for (index, face) in out.faces.iter().enumerate() {
        if roof_faces.contains(&index) {
            top.extend(face.iter().copied());
        } else if face.len() == 4 {
            top.extend(face[2..].iter().copied());
        }
    }
    let mut replacements = Vec::new();
    for index in top {
        let point = out.vertices[index];
        let x = point[0] + t.origin[0];
        let y = point[1] + t.origin[1];
        let Some(z) = triangles.iter().find_map(|triangle| triangle.height(x, y)) else {
            return Ok(false);
        };
        replacements.push((
            index,
            t.local(x, y, z + c.layers["bd_topo.batiment"].z_offset_m),
        ));
    }
    let applied = !replacements.is_empty();
    for (index, point) in replacements {
        out.vertices[index] = point;
    }
    Ok(applied)
}
