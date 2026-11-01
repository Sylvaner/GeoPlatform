//! Contexte : l'utilisateur fournit un centre ; le serveur attend des indices de tuiles.
//! Usage : après conversion du carré vers le CRS des tuiles, `app.rs` appelle
//! `tile_range` avec une Bbox pour trouver les lignes et colonnes à télécharger.
//! Les structures Bbox, TileRange et TileMatrix sont définies dans `models/`.
//! Ce fichier ne fait ni appel HTTP ni écriture de fichier.

use anyhow::{Result, bail};
use std::cmp::{max, min};

use crate::models::{Bbox, TileMatrix, TileRange};

/// Calcule les indices inclusifs des tuiles du carré, bornés à la grille annoncée.
pub fn tile_range(bbox: Bbox, matrix: &TileMatrix) -> Result<TileRange> {
    // bbox, origine et pas de pixel doivent avoir la même unité.
    // Exemple WGS84 : 256 pixels × 0,0000429153 degré/pixel ≈ 0,0109863 degré.
    // Ne jamais utiliser ici la résolution nominale en mètres d'une grille en degrés.
    let tile_width_units = matrix.tile_width as f64 * matrix.resolution_units_per_px;
    let tile_height_units = matrix.tile_height as f64 * matrix.resolution_units_per_px;

    if !tile_width_units.is_finite()
        || !tile_height_units.is_finite()
        || tile_width_units <= 0.0
        || tile_height_units <= 0.0
    {
        bail!("taille de tuile invalide");
    }
    if ![bbox.xmin, bbox.ymin, bbox.xmax, bbox.ymax]
        .iter()
        .all(|value| value.is_finite())
        || bbox.xmin >= bbox.xmax
        || bbox.ymin >= bbox.ymax
    {
        bail!("emprise invalide dans le système de la grille");
    }

    // Le bord droit et le bord du bas sont exclus : s'ils tombent sur une
    // frontière de tuile, on ne prend pas la tuile voisine en plus.
    // Décalage depuis l'origine / largeur d'une tuile = indice de colonne.
    // floor arrondit vers le bas. On garde un entier signé (i64) à cette étape
    // car un carré débordant à gauche ou en haut peut donner un indice négatif.
    let col_min = ((bbox.xmin - matrix.top_left_x) / tile_width_units).floor() as i64;
    // ceil - 1 exclut une frontière exacte, sans soustraire un epsilon qui
    // pourrait faire disparaître une très petite emprise pourtant non vide.
    let col_max = ((bbox.xmax - matrix.top_left_x) / tile_width_units).ceil() as i64 - 1;

    // L'origine de la grille est en haut à gauche : les lignes augmentent vers le sud.
    let row_min = ((matrix.top_left_y - bbox.ymax) / tile_height_units).floor() as i64;
    let row_max = ((matrix.top_left_y - bbox.ymin) / tile_height_units).ceil() as i64 - 1;

    // Faire l'intersection avec la couverture annoncée pour la couche.
    // Si la demande est entièrement hors couverture, les bornes s'inversent et
    // donnent une erreur. L'ancien bornage pouvait télécharger une tuile lointaine.
    let col_min = max(col_min, matrix.limits.col_min as i64);
    let col_max = min(col_max, matrix.limits.col_max as i64);
    let row_min = max(row_min, matrix.limits.row_min as i64);
    let row_max = min(row_max, matrix.limits.row_max as i64);

    if col_min > col_max || row_min > row_max {
        bail!("l'emprise ne coupe pas la matrice WMTS");
    }

    // Les indices retenus sont stockés sous forme d'entiers non signés (u64).
    Ok(TileRange {
        col_min: col_min as u64,
        col_max: col_max as u64,
        row_min: row_min as u64,
        row_max: row_max as u64,
    })
}

// Ce module est compilé pour les tests, pas pour le programme livré.
#[cfg(test)]
mod tests {
    // super désigne le module parent ; * importe ses noms dans ce module de tests.
    use super::*;

    // Petite grille artificielle, sans lecture de données IGN.
    fn matrix() -> TileMatrix {
        TileMatrix {
            matrix_set: "test-grid".to_owned(),
            crs: "EPSG:2154".to_owned(),
            identifier: "test".to_owned(),
            scale_denominator: 1.0 / 0.00028,
            resolution_units_per_px: 1.0,
            nominal_resolution_m_per_px: 1.0,
            top_left_x: 0.0,
            top_left_y: 1000.0,
            tile_width: 100,
            tile_height: 100,
            matrix_width: 10,
            matrix_height: 10,
            limits: TileRange {
                col_min: 0,
                col_max: 9,
                row_min: 0,
                row_max: 9,
            },
        }
    }

    // #[test] indique à cargo test que cette fonction est un test à exécuter.
    #[test]
    fn very_small_positive_bbox_at_tile_boundary_still_selects_a_tile() {
        let area = Bbox {
            xmin: 100.0,
            ymin: 899.999999999,
            ymax: 900.0,
            xmax: 100.000000001,
        };
        let result = tile_range(area, &matrix()).unwrap();
        assert_eq!(
            (
                result.col_min,
                result.col_max,
                result.row_min,
                result.row_max
            ),
            (1, 1, 1, 1)
        );
    }
    #[test]
    fn rectangular_tiles_use_independent_width_and_height() {
        let mut grid = matrix();
        grid.tile_height = 50;
        let result = tile_range(
            Bbox {
                xmin: 100.0,
                xmax: 200.0,
                ymin: 850.0,
                ymax: 900.0,
            },
            &grid,
        )
        .unwrap();
        assert_eq!(
            (
                result.col_min,
                result.col_max,
                result.row_min,
                result.row_max
            ),
            (1, 1, 2, 2)
        );
    }
    #[test]
    fn invalid_pixel_resolutions_are_rejected() {
        for resolution in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let mut grid = matrix();
            grid.resolution_units_per_px = resolution;
            assert!(tile_range(Bbox::square(150.0, 850.0, 100.0), &grid).is_err());
        }
    }

    // unwrap est volontaire dans les tests : un Err doit faire échouer le test.
    #[test]
    fn exact_tile_boundaries_do_not_include_neighbors() {
        let bbox = Bbox::square(150.0, 850.0, 100.0);
        let range = tile_range(bbox, &matrix()).unwrap();

        assert_eq!((range.col_min, range.col_max), (1, 1));
        assert_eq!((range.row_min, range.row_max), (1, 1));
        assert_eq!(range.count(), 1);
    }

    #[test]
    fn square_across_boundaries_includes_all_four_tiles() {
        let bbox = Bbox::square(100.0, 900.0, 100.0);
        let range = tile_range(bbox, &matrix()).unwrap();

        assert_eq!((range.col_min, range.col_max), (0, 1));
        assert_eq!((range.row_min, range.row_max), (0, 1));
        assert_eq!(range.count(), 4);
    }

    #[test]
    fn real_wgs84_grid_covers_the_user_square_with_twenty_tiles() {
        // Régression du cas fourni : une emprise en degrés ne doit jamais être
        // divisée par la résolution nominale en mètres de la grille.
        let (_, matrix) = crate::wmts::parse_capabilities(
            include_str!("../tests/fixtures/capabilities.xml"),
            crate::models::WmtsProduct::RgeAlti,
        )
        .unwrap();
        let center =
            crate::coordinates::to_lambert93(48.26183052164179, 4.738721628572549, "latlon")
                .unwrap();
        let bbox = crate::coordinates::bbox_from_lambert93(
            Bbox::square(center.0, center.1, 3000.0),
            &matrix.crs,
        )
        .unwrap();
        let range = tile_range(bbox, &matrix).unwrap();
        assert_eq!((range.col_min, range.col_max), (16813, 16817));
        assert_eq!((range.row_min, range.row_max), (3797, 3800));
        assert_eq!(range.count(), 20);
    }

    #[test]
    fn request_outside_coverage_is_rejected_instead_of_clamped_to_a_distant_tile() {
        let matrix = matrix();
        assert!(tile_range(Bbox::square(-200.0, 500.0, 100.0), &matrix).is_err());
        assert!(tile_range(Bbox::square(1200.0, 500.0, 100.0), &matrix).is_err());
        assert!(tile_range(Bbox::square(500.0, 1200.0, 100.0), &matrix).is_err());
        assert!(tile_range(Bbox::square(500.0, -200.0, 100.0), &matrix).is_err());
    }

    #[test]
    fn request_is_clipped_to_layer_limits_and_empty_bbox_is_rejected() {
        let mut matrix = matrix();
        matrix.limits = TileRange {
            col_min: 1,
            col_max: 2,
            row_min: 1,
            row_max: 2,
        };
        let range = tile_range(Bbox::square(150.0, 850.0, 300.0), &matrix).unwrap();
        assert_eq!(
            (range.col_min, range.col_max, range.row_min, range.row_max),
            (1, 2, 1, 2)
        );
        assert!(tile_range(Bbox::square(150.0, 850.0, 0.0), &matrix).is_err());
    }
}
