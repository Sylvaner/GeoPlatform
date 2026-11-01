//! Contexte : une extrusion BD TOPO donne un volume simple ; un LiDAR dense décrit le toit.
//! Usage : garder le contour BD TOPO, comparer couverture et densité des points de classe 6,
//! puis remplacer le toit plat par une surface mesurée si les seuils sont satisfaits.
//! La source et les critères restent dans les attributs de chaque bâtiment exporté.

use super::{
    geometry,
    models::{ExportConfig, LidarSamples, ObjectPlan, RoofSurface, TerrainGrid},
};
use anyhow::{Context, Result, bail};
use geo::{
    Area, BooleanOps, BoundingRect, Contains, Coord, Point, Polygon, Rect, TriangulateEarcut,
};
use serde_json::{Value, json};

/// Les mesures sont sélectionnées DANS ce contour, pour éviter le toit du voisin.
fn select(
    p: &Polygon<f64>,
    t: &TerrainGrid,
    c: &ExportConfig,
    s: &LidarSamples,
) -> Result<(Option<RoofSurface>, Value)> {
    let b = &c.buildings;
    let size = b.cell_size_m;
    let rect = p.bounding_rect().context("contour de bâtiment vide")?;
    let xmin = (rect.min().x / size).floor() as i64;
    let xmax = (rect.max().x / size).floor() as i64;
    let ymin = (rect.min().y / size).floor() as i64;
    let ymax = (rect.max().y / size).floor() as i64;
    if (xmax as f64 - xmin as f64 + 1.) * (ymax as f64 - ymin as f64 + 1.)
        > b.max_cells_per_building as f64
    {
        bail!("bâtiment trop étendu pour buildings.max_cells_per_building");
    }
    let mut cells = std::collections::BTreeMap::new();
    let mut expected = 0;
    let mut covered = 0;
    let mut points = 0;
    for x in xmin..=xmax {
        for y in ymin..=ymax {
            let inside = p.contains(&Point::new(
                (x as f64 + 0.5) * size,
                (y as f64 + 0.5) * size,
            ));
            if inside {
                expected += 1;
            }
            if let Some(cell) = s.roofs.get(&(x, y)) {
                let mean = cell.mean();
                let height = mean[2] - t.height(mean[0], mean[1])?;
                if p.contains(&Point::new(mean[0], mean[1]))
                    && height >= b.min_height_m
                    && height <= b.max_height_m
                    && cell.max_z - cell.min_z <= b.max_cell_spread_m
                {
                    cells.insert((x, y), mean);
                    points += cell.count;
                    if inside {
                        covered += 1;
                    }
                }
            }
        }
    }
    let coverage = if expected > 0 {
        covered as f64 / expected as f64
    } else {
        0.
    };
    let density = points as f64 / p.unsigned_area().max(1e-9);
    let accepted =
        points >= b.min_points && coverage >= b.min_coverage && density >= b.min_density_per_m2;
    let report = json!({"source":if accepted {"LiDAR HD classe 6"} else {"BD TOPO / hauteur de secours"},
        "points":points,"couverture":coverage,"densite_points_m2":density,
        "regle":"seuils de couverture, densité, hauteur et cohérence locale"});
    Ok((
        if accepted {
            Some(RoofSurface {
                cells,
                cell_size: size,
                radius: b.fill_radius_cells,
            })
        } else {
            None
        },
        report,
    ))
}

/// Construit d'abord dans un objet temporaire : un manque n'abîme pas le modèle de secours.
fn measured(
    p: &Polygon<f64>,
    surface: &RoofSurface,
    t: &TerrainGrid,
    offset: f64,
    out: &mut ObjectPlan,
) -> Result<bool> {
    let rect = p.bounding_rect().context("toit sans emprise")?;
    let size = surface.cell_size;
    let xmin = (rect.min().x / size).floor() as i64;
    let xmax = (rect.max().x / size).floor() as i64;
    let ymin = (rect.min().y / size).floor() as i64;
    let ymax = (rect.max().y / size).floor() as i64;
    for x in xmin..=xmax {
        for y in ymin..=ymax {
            let cell = Rect::new(
                Coord {
                    x: x as f64 * size,
                    y: y as f64 * size,
                },
                Coord {
                    x: (x + 1) as f64 * size,
                    y: (y + 1) as f64 * size,
                },
            )
            .to_polygon();
            for piece in p.intersection(&cell).0 {
                let raw = piece.earcut_triangles_raw();
                let start = out.vertices.len();
                for xy in raw.vertices {
                    let Some(z) = surface.height(xy[0], xy[1]) else {
                        return Ok(false);
                    };
                    out.vertices.push(t.local(xy[0], xy[1], z + offset));
                }
                for ids in raw.triangle_indices.chunks_exact(3) {
                    out.roof_faces.push(out.faces.len());
                    out.faces.push(ids.iter().map(|i| start + i).collect());
                }
            }
        }
    }
    // Densifier les murs suit le bord du toit mesuré, y compris autour des cours intérieures.
    for ring in std::iter::once(p.exterior()).chain(p.interiors()) {
        for pair in ring.0.windows(2) {
            let dx = pair[1].x - pair[0].x;
            let dy = pair[1].y - pair[0].y;
            let n = (dx.hypot(dy) / size).ceil().max(1.) as usize;
            for i in 0..n {
                let a = Point::new(
                    pair[0].x + dx * i as f64 / n as f64,
                    pair[0].y + dy * i as f64 / n as f64,
                );
                let d = Point::new(
                    pair[0].x + dx * (i + 1) as f64 / n as f64,
                    pair[0].y + dy * (i + 1) as f64 / n as f64,
                );
                let (Some(za), Some(zd)) =
                    (surface.height(a.x(), a.y()), surface.height(d.x(), d.y()))
                else {
                    return Ok(false);
                };
                let start = out.vertices.len();
                for (q, z) in [
                    (a, t.height(a.x(), a.y())?),
                    (d, t.height(d.x(), d.y())?),
                    (d, zd),
                    (a, za),
                ] {
                    out.vertices.push(t.local(q.x(), q.y(), z + offset));
                }
                out.faces.push(vec![start, start + 1, start + 2, start + 3]);
            }
        }
    }
    Ok(!out.roof_faces.is_empty())
}

pub fn improve(
    feature: &Value,
    out: &mut ObjectPlan,
    t: &TerrainGrid,
    c: &ExportConfig,
    s: &LidarSamples,
) -> Result<()> {
    let source_3d = super::building_source::apply(feature, out, t, c)?;
    let fallback = if source_3d {
        "BD TOPO 3D"
    } else {
        "BD TOPO / hauteur de secours"
    };
    if c.buildings.source == "bd_topo" {
        if let Some(properties) = out.properties.as_object_mut() {
            properties.insert(
                "export_toit".into(),
                json!({"source":fallback,"raison":"source BD TOPO imposée"}),
            );
        }
        return Ok(());
    }
    let g = &feature["geometry"];
    let v = &g["coordinates"];
    let polygons = if g["type"] == "Polygon" {
        vec![geometry::polygon(v)?]
    } else if g["type"] == "MultiPolygon" {
        v.as_array()
            .context("multipolygone invalide")?
            .iter()
            .map(geometry::polygon)
            .collect::<Result<Vec<_>>>()?
    } else {
        return Ok(());
    };
    let style = &c.layers["bd_topo.batiment"];
    let mut reports = Vec::new();
    let mut vertices = Vec::new();
    let mut faces = Vec::new();
    let mut roofs = Vec::new();
    for polygon in polygons {
        for p in polygon.intersection(&geometry::bounds(t)).0 {
            let (selected, mut report) = select(&p, t, c, s)?;
            let mut piece = ObjectPlan {
                name: out.name.clone(),
                kind: "mesh".into(),
                vertices: Vec::new(),
                faces: Vec::new(),
                roof_faces: Vec::new(),
                uv: Vec::new(),
                lines: Vec::new(),
                properties: Value::Null,
            };
            let accepted = if let Some(surface) = selected {
                measured(&p, &surface, t, style.z_offset_m, &mut piece)?
            } else {
                false
            };
            if !accepted {
                if c.buildings.source == "lidar" {
                    bail!(
                        "{} : toit LiDAR insuffisant selon les seuils configurés",
                        out.name
                    );
                }
                // Le modèle initial garde précisément la règle des champs de hauteur BD TOPO.
                // Si une partie est insuffisante, on conserve l'objet entier, sans mélange caché.
                report["source"] = json!(fallback);
                report["raison"] = json!("seuils LiDAR insuffisants ou manque non interpolable");
                if let Some(properties) = out.properties.as_object_mut() {
                    properties.insert("export_toit".into(), report);
                }
                return Ok(());
            }
            let base = vertices.len();
            let base_face = faces.len();
            vertices.extend(piece.vertices);
            faces.extend(
                piece
                    .faces
                    .into_iter()
                    .map(|f| f.into_iter().map(|i| i + base).collect()),
            );
            roofs.extend(piece.roof_faces.into_iter().map(|i| i + base_face));
            reports.push(report);
        }
    }
    if !faces.is_empty() {
        out.vertices = vertices;
        out.faces = faces;
        out.roof_faces = roofs;
    }
    if let Some(properties) = out.properties.as_object_mut() {
        properties.insert("export_toit".into(), json!(reports));
    }
    Ok(())
}
