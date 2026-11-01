//! Contexte : des rectangles indépendants laissent des fentes aux virages et aux carrefours.
//! Usage : élargir ensemble les axes avec des raccords arrondis, puis trianguler leur union.
//! La chaussée est ensuite échantillonnée sur le relief ; aucun tronçon n'est raccourci.

use super::{
    geometry,
    models::{ExportConfig, LayerPlan, ObjectPlan, TerrainGrid},
};
use anyhow::{Result, bail};
use geo::{BooleanOps, Buffer, Coord, LineString, MultiLineString, TriangulateEarcut};

pub fn join(layer: &mut LayerPlan, t: &TerrainGrid, c: &ExportConfig) -> Result<()> {
    if layer.name != "bd_topo.troncon_de_route" || layer.objects.is_empty() {
        return Ok(());
    }
    let lines = MultiLineString(
        layer
            .objects
            .iter()
            .flat_map(|o| &o.lines)
            .map(|line| {
                LineString::new(
                    line.iter()
                        .map(|p| Coord {
                            x: p[0] + t.origin[0],
                            y: p[1] + t.origin[1],
                        })
                        .collect(),
                )
            })
            .collect(),
    );
    // MultiLineString.buffer réalise aussi l'union des zones qui se recouvrent.
    // Les bouts arrondis couvrent les raccords des tronçons partageant un même nœud.
    let surface = lines
        .buffer(layer.style.line_width_m / 2.)
        .intersection(&geometry::bounds(t));
    let mut out = ObjectPlan {
        name: "Chaussées raccordées".into(),
        kind: "mesh".into(),
        vertices: Vec::new(),
        faces: Vec::new(),
        roof_faces: Vec::new(),
        uv: Vec::new(),
        lines: Vec::new(),
        properties: serde_json::json!({"methode":"union des chaussées à raccords arrondis",
            "sources":layer.objects.iter().map(|o|serde_json::json!({"id":o.name,"attributs":o.properties})).collect::<Vec<_>>()}),
    };
    for p in surface.0 {
        let raw = p.earcut_triangles_raw();
        let mut pending: Vec<[[f64; 2]; 3]> = raw
            .triangle_indices
            .chunks_exact(3)
            .map(|ids| std::array::from_fn(|i| raw.vertices[ids[i]]))
            .collect();
        while let Some(triangle) = pending.pop() {
            // Couper le côté le plus long est simple et évite de gros triangles
            // flottant au-dessus d'une vallée. Tous les sommets relisent le terrain.
            let edges: [f64; 3] = std::array::from_fn(|i| {
                let a = triangle[i];
                let b = triangle[(i + 1) % 3];
                (b[0] - a[0]).hypot(b[1] - a[1])
            });
            let edge = (0..3)
                .max_by(|a, b| edges[*a].total_cmp(&edges[*b]))
                .unwrap();
            if edges[edge] > c.vectors.line_sample_step_m {
                let a = triangle[edge];
                let b = triangle[(edge + 1) % 3];
                let d = triangle[(edge + 2) % 3];
                let middle = [(a[0] + b[0]) / 2., (a[1] + b[1]) / 2.];
                pending.push([a, middle, d]);
                pending.push([middle, b, d]);
            } else {
                if out.vertices.len() + 3 > c.vectors.max_road_vertices {
                    bail!(
                        "chaussée trop dense : augmenter vectors.max_road_vertices ou line_sample_step_m"
                    );
                }
                let start = out.vertices.len();
                for [x, y] in triangle {
                    out.vertices
                        .push(t.local(x, y, t.height(x, y)? + layer.style.z_offset_m));
                }
                out.faces.push(vec![start, start + 1, start + 2]);
            }
        }
    }
    layer.objects = if out.faces.is_empty() {
        Vec::new()
    } else {
        vec![out]
    };
    Ok(())
}
