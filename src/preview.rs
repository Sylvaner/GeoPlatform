//! Contexte : aperçu léger d'un STL sans dépendre de Blender ni d'un moteur 3D.
//! Usage : projeter un échantillon de triangles dans un SVG affichable directement par le front.

use std::path::Path;

use crate::error::{AppError, AppResult};

/// Nombre maximal de triangles dessinés pour garder un aperçu léger.
const MAX_TRIANGLES: usize = 4_000;

/// Crée un dessin isométrique SVG à partir d'un STL binaire.
pub async fn create_stl_svg(input: &Path, output: &Path) -> AppResult<()> {
    let bytes = tokio::fs::read(input).await?;
    let triangles = parse_binary_stl(&bytes)?;
    let svg = render_svg(&triangles);
    tokio::fs::write(output, svg).await?;
    Ok(())
}

/// Lit uniquement les sommets nécessaires dans la structure STL binaire standard.
fn parse_binary_stl(bytes: &[u8]) -> AppResult<Vec<[[f32; 3]; 3]>> {
    if bytes.len() < 84 {
        return Err(AppError::engine("STL trop court pour produire un aperçu"));
    }
    let announced = u32::from_le_bytes(bytes[80..84].try_into().expect("quatre octets")) as usize;
    let available = (bytes.len() - 84) / 50;
    if available < announced {
        return Err(AppError::engine("STL binaire tronqué"));
    }
    let stride = announced.div_ceil(MAX_TRIANGLES).max(1);
    let mut triangles = Vec::with_capacity(announced.min(MAX_TRIANGLES));
    for index in (0..announced).step_by(stride) {
        let start = 84 + index * 50 + 12;
        let mut triangle = [[0.0; 3]; 3];
        for (vertex_index, vertex) in triangle.iter_mut().enumerate() {
            let offset = start + vertex_index * 12;
            for (axis, coordinate) in vertex.iter_mut().enumerate() {
                let value = offset + axis * 4;
                *coordinate = f32::from_le_bytes(
                    bytes[value..value + 4]
                        .try_into()
                        .expect("quatre octets contrôlés"),
                );
            }
        }
        triangles.push(triangle);
    }
    Ok(triangles)
}

/// Projette les points en isométrie et ajuste automatiquement leur emprise au canevas.
fn render_svg(triangles: &[[[f32; 3]; 3]]) -> String {
    let projected: Vec<[[f32; 2]; 3]> = triangles
        .iter()
        .map(|triangle| triangle.map(project))
        .collect();
    let mut minimum = [f32::INFINITY; 2];
    let mut maximum = [f32::NEG_INFINITY; 2];
    for point in projected.iter().flat_map(|triangle| triangle.iter()) {
        minimum[0] = minimum[0].min(point[0]);
        minimum[1] = minimum[1].min(point[1]);
        maximum[0] = maximum[0].max(point[0]);
        maximum[1] = maximum[1].max(point[1]);
    }
    if projected.is_empty() {
        minimum = [0.0, 0.0];
        maximum = [1.0, 1.0];
    }
    let width = (maximum[0] - minimum[0]).max(1.0);
    let height = (maximum[1] - minimum[1]).max(1.0);
    let scale = (760.0 / width).min(560.0 / height);
    let mut paths = String::new();
    for triangle in projected {
        let points: Vec<String> = triangle
            .iter()
            .map(|point| {
                let x = 20.0 + (point[0] - minimum[0]) * scale;
                let y = 20.0 + (maximum[1] - point[1]) * scale;
                format!("{x:.1},{y:.1}")
            })
            .collect();
        paths.push_str(&format!("<polygon points=\"{}\"/>\n", points.join(" ")));
    }
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 800 600\" role=\"img\" aria-label=\"Aperçu STL\"><rect width=\"800\" height=\"600\" fill=\"#f4f1e8\"/><g fill=\"#b7c8a4\" fill-opacity=\".45\" stroke=\"#31473a\" stroke-width=\".35\">{paths}</g></svg>"
    )
}

/// Projection isométrique stable, suffisante pour contrôler visuellement le relief.
fn project(point: [f32; 3]) -> [f32; 2] {
    [
        (point[0] - point[1]) * 0.866_025_4,
        (point[0] + point[1]) * 0.5 - point[2],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vérifie la lecture minimale d'un triangle STL binaire.
    #[test]
    fn reads_one_binary_triangle() {
        let mut bytes = vec![0_u8; 84 + 50];
        bytes[80..84].copy_from_slice(&1_u32.to_le_bytes());
        bytes[84 + 12..84 + 16].copy_from_slice(&1.0_f32.to_le_bytes());
        let triangles = parse_binary_stl(&bytes).expect("STL valide");
        assert_eq!(triangles.len(), 1);
        assert_eq!(triangles[0][0][0], 1.0);
    }
}
