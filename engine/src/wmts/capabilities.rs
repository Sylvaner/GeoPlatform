//! Contexte : le catalogue WMTS décrit les couches, leurs grilles et leurs niveaux.
//! Usage : app.rs appelle parse_capabilities pour choisir la couche et le niveau le
//! plus fin réellement autorisé. Le nom du produit ne garantit pas le pas des tuiles.
//! Le serveur annonce actuellement WGS84G_6_14 pour RGE ALTI, pas 2154_1M_MNT.
//! Lire SupportedCRS et TileMatrixSetLimits évite de confondre mètres et degrés
//! et de demander un niveau ou des tuiles qui ne sont pas servis pour cette couche.

use anyhow::{Context, Result, anyhow, bail};
use roxmltree::{Document, Node};

use crate::models::{TileMatrix, TileRange, WmtsLayer, WmtsProduct};

// Le standard WMTS définit un pixel conventionnel de 0,28 mm.
const OGC_PIXEL_SIZE_M: f64 = 0.00028;
// Pour un CRS géographique, l'échelle WMTS utilise la longueur équatoriale d'un degré.
// Ce facteur donne le pas en degrés ; il ne donne pas la largeur locale en mètres.
const METERS_PER_DEGREE: f64 = 111319.49079327358;

/// Retrouve la couche du produit et choisit son niveau le plus fin disponible.
pub fn parse_capabilities(xml: &str, product: WmtsProduct) -> Result<(WmtsLayer, TileMatrix)> {
    let layer_id = product.layer_id();
    let document = Document::parse(xml).context("XML GetCapabilities invalide")?;
    // Le XML est un arbre. descendants parcourt ses éléments ; find prend la couche.
    // *node lit le Node derrière la référence fournie par l'itérateur.
    let layer_node = document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "Layer")
        .find(|node| child_text(*node, "Identifier").as_deref() == Some(layer_id))
        .ok_or_else(|| anyhow!("couche WMTS introuvable : {layer_id}"))?;

    let layer = read_layer(layer_node, product)?;
    let matrix = read_matrix(&document, layer_node, layer_id)?;
    Ok((layer, matrix))
}

/// Choisit les paramètres de contenu, indépendamment de la grille géographique.
fn read_layer(layer: Node<'_, '_>, product: WmtsProduct) -> Result<WmtsLayer> {
    let layer_id = product.layer_id();
    // filter_map garde les éléments ayant du texte ; collect construit la liste.
    let formats: Vec<String> = layer
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "Format")
        .filter_map(|node| node.text().map(str::trim).map(str::to_owned))
        .collect();
    let format = choose_format(&formats, product.preferred_format())
        .ok_or_else(|| anyhow!("aucun format WMTS trouvé pour {layer_id}"))?;
    Ok(WmtsLayer {
        identifier: product.layer_id().to_owned(),
        style: read_style(layer),
        format,
    })
}

/// Utilise le style par défaut, sinon le premier, sinon le nom « normal ».
fn read_style(layer: Node<'_, '_>) -> String {
    // Node emprunte le document et son texte ; '_ laisse Rust déduire ces durées de vie.
    let styles: Vec<_> = layer
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "Style")
        .collect();
    let default_style = styles.iter().find(|node| {
        node.attribute("isDefault")
            .is_some_and(|value| value.eq_ignore_ascii_case("true"))
    });
    // Ces étapes manipulent Option : Some(valeur) ou None si rien n'est trouvé.
    // or_else essaie un repli ; and_then lit le nom ; unwrap_or_else crée le secours.
    default_style
        .or_else(|| styles.first())
        .and_then(|node| child_text(*node, "Identifier"))
        .unwrap_or_else(|| "normal".to_owned())
}

/// Préfère le format du produit (BIL ou JPEG), puis TIFF, puis le premier format. None signifie qu'il n'y en a aucun.
fn choose_format(formats: &[String], preferred: &str) -> Option<String> {
    for keyword in [preferred, "tif"] {
        for format in formats {
            if format.to_ascii_lowercase().contains(keyword) {
                // clone rend une chaîne indépendante de la liste empruntée.
                return Some(format.clone());
            }
        }
    }
    formats.first().cloned()
}

/// Examine seulement les jeux de grilles liés à la couche, jamais un nom supposé.
fn read_matrix(document: &Document<'_>, layer: Node<'_, '_>, layer_id: &str) -> Result<TileMatrix> {
    let mut matrices = Vec::new();
    let mut advertised = Vec::new();

    for link in layer
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "TileMatrixSetLink")
    {
        let identifier = required_child_text(link, "TileMatrixSet")?;
        advertised.push(identifier.clone());
        // Il existe aussi des éléments TileMatrixSet dans les liens. Celui qui décrit
        // la grille possède un enfant Identifier ; c'est celui que nous cherchons.
        let grid = document
            .descendants()
            .filter(|node| node.is_element() && node.tag_name().name() == "TileMatrixSet")
            .find(|node| child_text(*node, "Identifier").as_deref() == Some(identifier.as_str()))
            .ok_or_else(|| anyhow!("grille annoncée mais introuvable : {identifier}"))?;
        matrices.extend(read_grid_matrices(grid, link, &identifier)?);
    }

    // Plus l'échelle est petite, plus le niveau est fin. On ne fabrique pas un
    // niveau à 1 m si le serveur ne l'annonce pas : on garde sa résolution réelle.
    matrices
        .into_iter()
        .min_by(|a, b| {
            a.nominal_resolution_m_per_px
                .total_cmp(&b.nominal_resolution_m_per_px)
        })
        .ok_or_else(|| {
            anyhow!(
                "aucune grille exploitable pour {layer_id}. Grilles annoncées : {}. \
             CRS pris en charge : IGNF:WGS84G, CRS84, EPSG:4326, EPSG:2154 et EPSG:3857",
                advertised.join(", ")
            )
        })
}

/// Lit les niveaux d'une grille, avec ses unités, son ordre d'axes et ses limites.
fn read_grid_matrices(
    grid: Node<'_, '_>,
    link: Node<'_, '_>,
    identifier: &str,
) -> Result<Vec<TileMatrix>> {
    let supported_crs = required_child_text(grid, "SupportedCRS")?.to_ascii_uppercase();
    // Les coordonnées internes sont toujours X/Y. Pour EPSG:4326, le standard
    // encode TopLeftCorner en latitude/longitude ; IGNF:WGS84G et CRS84 utilisent
    // longitude/latitude. Cette règle XML est indépendante de la saisie du terminal.
    let (crs, reverse_axes) = if supported_crs == "IGNF:WGS84G"
        || supported_crs.contains("CRS84")
        || supported_crs == "CRS:84"
    {
        ("EPSG:4326", false)
    } else if supported_crs.contains("EPSG") {
        match supported_crs.rsplit(':').next().unwrap_or("") {
            "4326" => ("EPSG:4326", true),
            "2154" => ("EPSG:2154", false),
            "3857" => ("EPSG:3857", false),
            _ => return Ok(Vec::new()),
        }
    } else {
        return Ok(Vec::new());
    };

    let limits_node = grid_child(link, "TileMatrixSetLimits");
    let mut matrices = Vec::new();
    for node in grid
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "TileMatrix")
    {
        let mut matrix = read_tile_matrix(node, identifier, crs, reverse_axes)?;
        if let Some(limits_node) = limits_node {
            // Une grille mondiale peut avoir davantage de niveaux que ceux de la
            // couche. Si une liste de limites est fournie, n'utiliser que ses niveaux.
            let level_limits = limits_node.children().find(|limit| {
                limit.is_element()
                    && limit.tag_name().name() == "TileMatrixLimits"
                    && child_text(*limit, "TileMatrix").as_deref()
                        == Some(matrix.identifier.as_str())
            });
            let Some(level_limits) = level_limits else {
                continue;
            };
            matrix.limits = TileRange {
                col_min: required_child_text(level_limits, "MinTileCol")?.parse()?,
                col_max: required_child_text(level_limits, "MaxTileCol")?.parse()?,
                row_min: required_child_text(level_limits, "MinTileRow")?.parse()?,
                row_max: required_child_text(level_limits, "MaxTileRow")?.parse()?,
            };
            if matrix.limits.col_min > matrix.limits.col_max
                || matrix.limits.row_min > matrix.limits.row_max
                || matrix.limits.col_max >= matrix.matrix_width
                || matrix.limits.row_max >= matrix.matrix_height
            {
                bail!(
                    "limites de tuiles invalides pour {identifier}/{}",
                    matrix.identifier
                );
            }
        }
        matrices.push(matrix);
    }
    Ok(matrices)
}

/// Transforme un élément TileMatrix en nombres utilisables par les calculs.
fn read_tile_matrix(
    node: Node<'_, '_>,
    matrix_set: &str,
    crs: &str,
    reverse_axes: bool,
) -> Result<TileMatrix> {
    // parse convertit du texte en nombre ; ? propage l'erreur si le texte est invalide.
    let scale_denominator: f64 = required_child_text(node, "ScaleDenominator")?.parse()?;
    if !scale_denominator.is_finite() || scale_denominator <= 0.0 {
        bail!("échelle WMTS invalide");
    }
    let top_left = required_child_text(node, "TopLeftCorner")?;
    let mut coordinates = top_left.split_whitespace();
    let first: f64 = coordinates
        .next()
        .ok_or_else(|| anyhow!("origine X absente"))?
        .parse()?;
    let second: f64 = coordinates
        .next()
        .ok_or_else(|| anyhow!("origine Y absente"))?
        .parse()?;
    let (top_left_x, top_left_y) = if reverse_axes {
        (second, first)
    } else {
        (first, second)
    };
    let tile_width = required_child_text(node, "TileWidth")?.parse()?;
    let tile_height = required_child_text(node, "TileHeight")?.parse()?;
    let matrix_width: u64 = required_child_text(node, "MatrixWidth")?.parse()?;
    let matrix_height: u64 = required_child_text(node, "MatrixHeight")?.parse()?;
    if tile_width == 0
        || tile_height == 0
        || matrix_width == 0
        || matrix_height == 0
        || matrix_width > i64::MAX as u64
        || matrix_height > i64::MAX as u64
        || !top_left_x.is_finite()
        || !top_left_y.is_finite()
    {
        bail!("dimensions ou origine de grille invalides");
    }

    let nominal_resolution_m_per_px = scale_denominator * OGC_PIXEL_SIZE_M;
    let resolution_units_per_px = if crs == "EPSG:4326" {
        // Diviser les mètres conventionnels par les mètres par degré donne des degrés.
        nominal_resolution_m_per_px / METERS_PER_DEGREE
    } else {
        nominal_resolution_m_per_px
    };

    Ok(TileMatrix {
        matrix_set: matrix_set.to_owned(),
        crs: crs.to_owned(),
        identifier: required_child_text(node, "Identifier")?,
        scale_denominator,
        resolution_units_per_px,
        nominal_resolution_m_per_px,
        top_left_x,
        top_left_y,
        tile_width,
        tile_height,
        matrix_width,
        matrix_height,
        // Sans restrictions annoncées, toute la grille est autorisée.
        limits: TileRange {
            col_min: 0,
            col_max: matrix_width - 1,
            row_min: 0,
            row_max: matrix_height - 1,
        },
    })
}

/// Cherche un enfant direct par son nom, sans dépendre du préfixe XML.
fn grid_child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    // Les durées de vie nommées indiquent que le Node renvoyé emprunte le même
    // document et le même texte XML que le Node reçu, sans en faire une copie.
    node.children()
        .find(|child| child.is_element() && child.tag_name().name() == name)
}

/// Lit le texte non vide d'un enfant direct, ou None s'il est absent.
fn child_text(node: Node<'_, '_>, name: &str) -> Option<String> {
    grid_child(node, name)
        .and_then(|child| child.text())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// Même lecture, mais un champ absent devient une erreur explicite.
fn required_child_text(node: Node<'_, '_>, name: &str) -> Result<String> {
    child_text(node, name).ok_or_else(|| anyhow!("élément XML manquant : {name}"))
}

// Les tests utilisent un extrait local représentatif du catalogue IGN, sans réseau.
#[cfg(test)]
mod tests {
    use super::*;
    fn parse_altitude(xml: &str) -> Result<(WmtsLayer, TileMatrix)> {
        super::parse_capabilities(xml, WmtsProduct::RgeAlti)
    }
    const CATALOG: &str = include_str!("../../tests/fixtures/capabilities.xml");

    #[test]
    fn malformed_xml_is_rejected() {
        assert!(parse_altitude("<broken>").is_err());
    }
    #[test]
    fn missing_product_layer_is_rejected() {
        assert!(
            parse_altitude(&CATALOG.replace("ELEVATION.ELEVATIONGRIDCOVERAGE.HIGHRES", "OTHER"))
                .is_err()
        );
    }
    #[test]
    fn zero_tile_width_is_rejected() {
        assert!(
            parse_altitude(
                &CATALOG.replace("<TileWidth>256</TileWidth>", "<TileWidth>0</TileWidth>")
            )
            .is_err()
        );
    }
    #[test]
    fn non_finite_scale_is_rejected() {
        assert!(parse_altitude(&CATALOG.replace("17061.8366707982518644", "NaN")).is_err());
    }
    #[test]
    fn reversed_layer_limits_are_rejected() {
        assert!(
            parse_altitude(&CATALOG.replace(
                "<MinTileCol>10653</MinTileCol>",
                "<MinTileCol>22000</MinTileCol>"
            ))
            .is_err()
        );
    }
    #[test]
    fn layer_limits_outside_matrix_are_rejected() {
        assert!(
            parse_altitude(&CATALOG.replace(
                "<MaxTileCol>21663</MaxTileCol>",
                "<MaxTileCol>99999</MaxTileCol>"
            ))
            .is_err()
        );
    }
    #[test]
    fn geographic_crs84_does_not_reverse_origin_axes() {
        let (_, matrix) =
            parse_altitude(&CATALOG.replace("IGNF:WGS84G", "urn:ogc:def:crs:OGC::CRS84")).unwrap();
        assert_eq!((matrix.top_left_x, matrix.top_left_y), (-180.0, 90.0));
    }

    #[test]
    fn catalog_selects_finest_authorized_level_and_degree_units() {
        let (layer, matrix) = parse_altitude(CATALOG).unwrap();
        assert_eq!(layer.style, "normal");
        assert_eq!(layer.format, "image/x-bil;bits=32");
        assert_eq!(matrix.matrix_set, "WGS84G_6_14");
        assert_eq!(matrix.identifier, "14");
        assert_eq!(matrix.crs, "EPSG:4326");
        assert_eq!((matrix.top_left_x, matrix.top_left_y), (-180.0, 90.0));
        assert!((matrix.resolution_units_per_px - 360.0 / (32768.0 * 256.0)).abs() < 1e-12);
        assert!((matrix.nominal_resolution_m_per_px - 4.7773142678).abs() < 1e-8);
        assert_eq!(matrix.limits.row_min, 3540);
        assert_eq!(matrix.limits.col_max, 21663);
    }

    #[test]
    fn catalog_falls_back_to_first_style_and_tiff() {
        let xml = CATALOG
            .replace("isDefault=\"true\"", "isDefault=\"false\"")
            .replace("<Format>image/x-bil;bits=32</Format>", "");
        let (layer, _) = parse_altitude(&xml).unwrap();
        assert_eq!(layer.style, "alternative");
        assert_eq!(layer.format, "image/tiff");
    }

    #[test]
    fn epsg_4326_xml_axis_order_is_normalized() {
        let xml = CATALOG
            .replace("IGNF:WGS84G", "urn:ogc:def:crs:EPSG::4326")
            .replace(
                "<TopLeftCorner>-180 90</TopLeftCorner>",
                "<TopLeftCorner>90 -180</TopLeftCorner>",
            );
        let (_, matrix) = parse_altitude(&xml).unwrap();
        assert_eq!((matrix.top_left_x, matrix.top_left_y), (-180.0, 90.0));
    }

    #[test]
    fn unlinked_or_unsupported_grid_is_rejected() {
        let xml = CATALOG.replace(
            "<TileMatrixSet>WGS84G_6_14</TileMatrixSet>",
            "<TileMatrixSet>missing</TileMatrixSet>",
        );
        assert!(parse_altitude(&xml).is_err());
        assert!(parse_altitude(&CATALOG.replace("IGNF:WGS84G", "EPSG:9999")).is_err());
    }

    #[test]
    fn finer_global_level_without_layer_limits_is_not_selected() {
        // Le niveau 15 existe dans la grille de test mais pas dans les limites de couche.
        let (_, matrix) = parse_altitude(CATALOG).unwrap();
        assert_eq!(matrix.identifier, "14");
    }

    #[test]
    fn projected_grid_keeps_meter_units() {
        let xml = CATALOG.replace("IGNF:WGS84G", "EPSG:2154");
        let (_, matrix) = parse_altitude(&xml).unwrap();
        assert_eq!(matrix.crs, "EPSG:2154");
        assert_eq!(
            matrix.resolution_units_per_px,
            matrix.nominal_resolution_m_per_px
        );
    }

    #[test]
    fn ortho_uses_its_own_layer_jpeg_format_and_mercator_grid() {
        let (layer, matrix) = super::parse_capabilities(
            include_str!("../../tests/fixtures/ortho-capabilities.xml"),
            WmtsProduct::BdOrtho,
        )
        .unwrap();
        assert_eq!(layer.identifier, "HR.ORTHOIMAGERY.ORTHOPHOTOS");
        assert_eq!(layer.format, "image/jpeg");
        assert_eq!(matrix.matrix_set, "PM_6_19");
        assert_eq!(matrix.crs, "EPSG:3857");
        assert_eq!(matrix.identifier, "19");
        assert!((matrix.resolution_units_per_px - 0.2985821417).abs() < 1e-8);
    }
}
