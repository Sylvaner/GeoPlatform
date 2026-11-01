//! Contexte : les GeoJSON peuvent contenir des polygones troués, des lignes ou des points.
//! Usage : découper d'abord à l'emprise, puis poser la géométrie sur le terrain local.
//! geo conserve les trous et peut produire plusieurs morceaux après découpe.

use super::models::{ExportConfig, LayerConfig, ObjectPlan, TerrainGrid};
use anyhow::{Context, Result, bail};
use geo::{BooleanOps, Coord, LineString, MultiLineString, Polygon, Rect, TriangulateEarcut};
use serde_json::Value;

fn coordinates(value: &Value) -> Result<Vec<Coord<f64>>> {
    value
        .as_array()
        .context("liste de coordonnées absente")?
        .iter()
        .map(|p| {
            let x = p[0].as_f64().context("coordonnée X invalide")?;
            let y = p[1].as_f64().context("coordonnée Y invalide")?;
            if !x.is_finite() || !y.is_finite() {
                bail!("coordonnée non finie");
            }
            Ok(Coord { x, y })
        })
        .collect()
}

pub fn bounds(t: &TerrainGrid) -> Polygon<f64> {
    Rect::new(
        Coord {
            x: t.xmin,
            y: t.ymin,
        },
        Coord {
            x: t.xmin + t.side,
            y: t.ymin + t.side,
        },
    )
    .to_polygon()
}

pub fn polygon(value: &Value) -> Result<Polygon<f64>> {
    let rings = value.as_array().context("anneaux de polygone absents")?;
    if rings.is_empty() {
        bail!("polygone sans contour");
    }
    Ok(Polygon::new(
        LineString::new(coordinates(&rings[0])?),
        rings[1..]
            .iter()
            .map(|ring| coordinates(ring).map(LineString::new))
            .collect::<Result<_>>()?,
    ))
}

/// Ajoute une surface triangulée, et des murs pour une extrusion de bâtiment.
pub fn add_polygon(
    out: &mut ObjectPlan,
    p: &Polygon<f64>,
    t: &TerrainGrid,
    offset: f64,
    height: f64,
) -> Result<()> {
    let raw = p.earcut_triangles_raw();
    let start = out.vertices.len();
    let center = p.exterior().0.first().context("contour vide")?;
    let floor = t.height(center.x, center.y)?;
    for point in &raw.vertices {
        let z = if height > 0. {
            floor + height
        } else {
            t.height(point[0], point[1])?
        };
        out.vertices.push(t.local(point[0], point[1], z + offset));
    }
    for triangle in raw.triangle_indices.chunks_exact(3) {
        if height > 0. {
            out.roof_faces.push(out.faces.len());
        }
        out.faces.push(triangle.iter().map(|i| start + i).collect());
    }
    if height > 0. {
        for ring in std::iter::once(p.exterior()).chain(p.interiors()) {
            for pair in ring.0.windows(2) {
                let i = out.vertices.len();
                for (point, z) in [
                    (pair[0], floor),
                    (pair[1], floor),
                    (pair[1], floor + height),
                    (pair[0], floor + height),
                ] {
                    out.vertices.push(t.local(point.x, point.y, z + offset));
                }
                out.faces.push(vec![i, i + 1, i + 2, i + 3]);
            }
        }
    }
    Ok(())
}

fn add_lines(
    out: &mut ObjectPlan,
    lines: &MultiLineString<f64>,
    t: &TerrainGrid,
    c: &ExportConfig,
    offset: f64,
) -> Result<()> {
    for line in &lines.0 {
        let mut points = Vec::new();
        for pair in line.0.windows(2) {
            let dx = pair[1].x - pair[0].x;
            let dy = pair[1].y - pair[0].y;
            let count = (dx.hypot(dy) / c.vectors.line_sample_step_m).ceil().max(1.) as usize;
            for step in 0..count {
                let f = step as f64 / count as f64;
                let x = pair[0].x + dx * f;
                let y = pair[0].y + dy * f;
                points.push(t.local(x, y, t.height(x, y)? + offset));
            }
        }
        if let Some(p) = line.0.last() {
            points.push(t.local(p.x, p.y, t.height(p.x, p.y)? + offset));
        }
        if points.windows(2).any(|p| p[0] != p[1]) {
            out.lines.push(points);
        }
    }
    Ok(())
}

pub fn prepare(
    feature: &Value,
    key: &str,
    style: &LayerConfig,
    t: &TerrainGrid,
    c: &ExportConfig,
) -> Result<Option<ObjectPlan>> {
    let geometry = &feature["geometry"];
    if geometry.is_null() {
        return Ok(None);
    }
    let kind = geometry["type"]
        .as_str()
        .context("type géométrique absent")?;
    let value = &geometry["coordinates"];
    if value
        .as_array()
        .is_some_and(|coordinates| coordinates.is_empty())
    {
        return Ok(None);
    }
    let clip = bounds(t);
    let building = key == "bd_topo.batiment";
    let height = if building {
        c.vectors
            .building_height_fields
            .iter()
            .find_map(|field| {
                feature["properties"][field].as_f64().or_else(|| {
                    feature["properties"][field]
                        .as_str()
                        .and_then(|v| v.parse().ok())
                })
            })
            .filter(|v| v.is_finite() && *v > 0.)
            .unwrap_or(style.fallback_height_m)
    } else {
        0.
    };
    let mut out = ObjectPlan {
        name: feature["id"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{}", feature["properties"]["gid"])),
        kind: "mesh".into(),
        vertices: Vec::new(),
        faces: Vec::new(),
        roof_faces: Vec::new(),
        uv: Vec::new(),
        lines: Vec::new(),
        properties: feature["properties"].clone(),
    };
    if matches!(kind, "Polygon" | "MultiPolygon") {
        let polygons = if kind == "Polygon" {
            vec![polygon(value)?]
        } else {
            value
                .as_array()
                .context("multipolygone invalide")?
                .iter()
                .map(polygon)
                .collect::<Result<Vec<_>>>()?
        };
        let outline = key.starts_with("cadastre.") || key.contains("Bassin");
        for p in polygons {
            for p in p.intersection(&clip).0 {
                if outline {
                    let lines = MultiLineString(
                        std::iter::once(p.exterior().clone())
                            .chain(p.interiors().iter().cloned())
                            .collect(),
                    );
                    add_lines(&mut out, &lines, t, c, style.z_offset_m)?;
                } else if super::water::is_water(key) {
                    super::water::add_surface(&mut out, &p, t, style, c)?;
                } else {
                    add_polygon(&mut out, &p, t, style.z_offset_m, height)?;
                }
            }
        }
    } else if matches!(kind, "LineString" | "MultiLineString") {
        let lines = if kind == "LineString" {
            vec![LineString::new(coordinates(value)?)]
        } else {
            value
                .as_array()
                .context("multiligne invalide")?
                .iter()
                .map(|v| coordinates(v).map(LineString::new))
                .collect::<Result<_>>()?
        };
        let extra = if key.ends_with("ligne_electrique") {
            style.fallback_height_m
        } else {
            0.
        };
        add_lines(
            &mut out,
            &clip.clip(&MultiLineString(lines), false),
            t,
            c,
            style.z_offset_m + extra,
        )?;
    } else if matches!(kind, "Point" | "MultiPoint") {
        let points = if kind == "Point" {
            vec![value.clone()]
        } else {
            value.as_array().context("multipoint invalide")?.clone()
        };
        for point in points {
            let x = point[0].as_f64().context("X de point invalide")?;
            let y = point[1].as_f64().context("Y de point invalide")?;
            if x >= t.xmin && x <= t.xmin + t.side && y >= t.ymin && y <= t.ymin + t.side {
                out.vertices
                    .push(t.local(x, y, t.height(x, y)? + style.z_offset_m));
            }
        }
        out.kind = "points".into();
    } else {
        bail!("géométrie non prise en charge : {kind}");
    }
    // Des sommets isolés ne forment pas une surface. Un polygone aplati ou vide
    // ne doit pas fabriquer un objet vide, puis une collection inutile dans Blender.
    let has_geometry = (out.kind == "points" && !out.vertices.is_empty())
        || !out.faces.is_empty()
        || !out.lines.is_empty();
    if !has_geometry {
        Ok(None)
    } else {
        Ok(Some(out))
    }
}
