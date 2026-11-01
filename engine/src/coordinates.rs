//! Contexte : les coordonnées d'entrée doivent être converties en Lambert-93 (mètres).
//! Usage : `app.rs` appelle `to_lambert93` avec les deux valeurs et le CRS du terminal.
//! Puis bbox_from_lambert93 convertit le contour du carré vers le CRS des tuiles.
//! La saisie GPS utilise latitude/longitude, mais les calculs internes utilisent X/Y :
//! longitude/latitude pour une grille géographique, Est/Nord pour une grille projetée.
//! Pour les coordonnées géographiques, l'ordre est latitude puis longitude par défaut.
//! L'alias explicite lonlat permet de saisir longitude puis latitude.
//! Pour un système projeté, les valeurs restent dans l'ordre X puis Y.
//! La conversion utilise proj4rs en Rust : aucune installation de PROJ/SQLite requise.

use anyhow::{Context, Result, bail};
use proj4rs::{Proj, transform::transform};

use crate::models::Bbox;
// Déclaré ici pour partager les conversions entre les deux binaires, sans réseau.
pub const TARGET_CRS: &str = "EPSG:2154";

/// Convertit un nom courant ou un code numérique en identifiant EPSG.
pub fn normalize_crs(input: &str) -> Result<String> {
    // trim retire les espaces autour du texte ; la comparaison ignore la casse.
    let name = input.trim().to_ascii_lowercase();
    match name.as_str() {
        // Ces alias décrivent le même système. L'ordre lonlat est traité plus bas.
        "latlon" | "lonlat" | "wgs84" | "gps" | "4326" | "epsg:4326" => Ok("EPSG:4326".to_owned()),
        "lambert" | "lambert93" | "lambert-93" | "l93" | "2154" | "epsg:2154" => {
            Ok(TARGET_CRS.to_owned())
        }
        _ if name.starts_with("epsg:") => Ok(name.to_ascii_uppercase()),
        // Une chaîne vide ne doit pas être considérée comme un numéro EPSG.
        _ if !name.is_empty() && name.chars().all(|c| c.is_ascii_digit()) => {
            Ok(format!("EPSG:{name}"))
        }
        _ => bail!("CRS non reconnu : {input}. Utilise latlon, lonlat, lambert93 ou EPSG:xxxx"),
    }
}

/// Renvoie le centre en Lambert-93, dans l'ordre (X, Y), en mètres.
pub fn to_lambert93(first: f64, second: f64, input_crs: &str) -> Result<(f64, f64)> {
    if !first.is_finite() || !second.is_finite() {
        bail!("les coordonnées du centre doivent être des nombres finis");
    }

    let source_crs = normalize_crs(input_crs)?;
    // Si les valeurs sont déjà en Lambert-93, aucune transformation n'est nécessaire.
    if source_crs == TARGET_CRS {
        return Ok((first, second));
    }

    // Le catalogue EPSG est embarqué dans la bibliothèque Rust.
    // Un code absent ou une projection non prise en charge donne une erreur explicite.
    let source = Proj::from_user_string(&source_crs)
        .with_context(|| format!("système non pris en charge : {source_crs}"))?;
    let target = Proj::from_user_string(TARGET_CRS).context("création du système Lambert-93")?;

    let (x, y) = if source.is_latlong() {
        geographic_xy(first, second, input_crs)?
    } else {
        (first, second)
    };
    convert_xy(&source, &target, x, y)
        .with_context(|| format!("échec transformation {source_crs} -> {TARGET_CRS}"))
}

/// Convertit un point interne X/Y ; les angles entrent et sortent en degrés.
fn convert_xy(source: &Proj, target: &Proj, x: f64, y: f64) -> Result<(f64, f64)> {
    // proj4rs transforme un point (X, Y, Z) en modifiant sa valeur.
    // Z=0 sert au calcul horizontal ; ce n'est pas une altitude extraite du MNT.
    let mut point = (x, y, 0.0);
    if source.is_latlong() {
        // Les radians restent un détail interne de cette bibliothèque.
        point.0 = x.to_radians();
        point.1 = y.to_radians();
    }
    // &mut prête le point en autorisant la bibliothèque à modifier sa valeur.
    transform(source, target, &mut point).context("conversion des coordonnées X/Y")?;
    if target.is_latlong() {
        point.0 = point.0.to_degrees();
        point.1 = point.1.to_degrees();
    }
    if !point.0.is_finite() || !point.1.is_finite() {
        bail!("le point ne peut pas être converti dans le système demandé");
    }
    Ok((point.0, point.1))
}

/// Enveloppe le carré Lambert-93 dans un rectangle exprimé dans le CRS des tuiles.
pub fn bbox_from_lambert93(bbox: Bbox, target_crs: &str) -> Result<Bbox> {
    // Valider aussi le cas sans conversion, avant toute requête réseau.
    if [bbox.xmin, bbox.ymin, bbox.xmax, bbox.ymax]
        .iter()
        .any(|value| !value.is_finite())
        || bbox.xmin >= bbox.xmax
        || bbox.ymin >= bbox.ymax
    {
        bail!("emprise Lambert-93 invalide");
    }
    if target_crs == TARGET_CRS {
        return Ok(bbox);
    }
    let source = Proj::from_user_string(TARGET_CRS)?;
    let target = Proj::from_user_string(target_crs)?;
    let mut result = Bbox {
        xmin: f64::INFINITY,
        ymin: f64::INFINITY,
        xmax: f64::NEG_INFINITY,
        ymax: f64::NEG_INFINITY,
    };

    // Une projection peut courber les bords : les quatre coins seuls ne suffisent
    // pas toujours à décrire l'enveloppe. Échantillonner chaque côté en 32 segments
    // donne une approximation du contour adaptée aux zones locales demandées ici.
    // La liste des tuiles couvre le rectangle englobant, pas un découpage exact du carré.
    for step in 0..=32 {
        let fraction = step as f64 / 32.0;
        let x = bbox.xmin + fraction * (bbox.xmax - bbox.xmin);
        let y = bbox.ymin + fraction * (bbox.ymax - bbox.ymin);
        for (x, y) in [
            (x, bbox.ymin),
            (x, bbox.ymax),
            (bbox.xmin, y),
            (bbox.xmax, y),
        ] {
            let (x, y) = convert_xy(&source, &target, x, y)?;
            result.xmin = result.xmin.min(x);
            result.xmax = result.xmax.max(x);
            result.ymin = result.ymin.min(y);
            result.ymax = result.ymax.max(y);
        }
    }
    // Éviter une requête quasi mondiale si l'enveloppe traverse le changement de date.
    if target.is_latlong() && result.xmax - result.xmin > 180.0 {
        bail!("la zone traverse l'antiméridien ; ce cas n'est pas pris en charge");
    }
    Ok(result)
}

/// Remet une saisie géographique dans l'ordre interne (longitude, latitude).
fn geographic_xy(first: f64, second: f64, input_crs: &str) -> Result<(f64, f64)> {
    // Seul lonlat demande explicitement l'ordre inverse de la convention du programme.
    let (longitude, latitude) = if input_crs.trim().eq_ignore_ascii_case("lonlat") {
        (first, second)
    } else {
        (second, first)
    };
    if !(-90.0..=90.0).contains(&latitude) {
        bail!("latitude invalide : {latitude} (attendu entre -90 et 90 degrés)");
    }
    if !(-180.0..=180.0).contains(&longitude) {
        bail!("longitude invalide : {longitude} (attendu entre -180 et 180 degrés)");
    }
    Ok((longitude, latitude))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_bbox_is_rejected_even_without_reprojection() {
        for target in ["EPSG:2154", "EPSG:4326"] {
            for area in [
                Bbox::square(700000.0, 6600000.0, 0.0),
                Bbox::square(700000.0, 6600000.0, -10.0),
                Bbox::square(f64::INFINITY, 6600000.0, 10.0),
            ] {
                assert!(bbox_from_lambert93(area, target).is_err());
            }
        }
    }
    #[test]
    fn non_finite_centers_are_rejected_for_projected_and_geographic_crs() {
        for crs in ["latlon", "lambert93"] {
            for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                assert!(to_lambert93(number, 3.0, crs).is_err());
                assert!(to_lambert93(48.0, number, crs).is_err());
            }
        }
    }

    #[test]
    fn geographic_aliases_use_latitude_then_longitude() {
        for alias in ["latlon", "gps", "wgs84", "4326", "EPSG:4326"] {
            assert_eq!(geographic_xy(48.26, 4.73, alias).unwrap(), (4.73, 48.26));
        }
        assert_eq!(
            geographic_xy(4.73, 48.26, " LONLAT ").unwrap(),
            (4.73, 48.26)
        );
    }

    #[test]
    fn lambert_origin_has_the_expected_coordinates() {
        // Point définissant l'origine Lambert-93 : 46,5° N / 3° E -> 700000 / 6600000.
        for (first, second, crs) in [(46.5, 3.0, "latlon"), (3.0, 46.5, "lonlat")] {
            let (x, y) = to_lambert93(first, second, crs).unwrap();
            assert!((x - 700000.0).abs() < 0.01);
            assert!((y - 6600000.0).abs() < 0.01);
        }
    }

    #[test]
    fn both_geographic_orders_produce_the_same_user_center() {
        let latlon = to_lambert93(48.26183052164179, 4.738721628572549, "latlon").unwrap();
        let lonlat = to_lambert93(4.738721628572549, 48.26183052164179, "lonlat").unwrap();
        assert_eq!(latlon, lonlat);
    }

    #[test]
    fn another_projected_epsg_is_converted() {
        // Construire indépendamment les coordonnées Web Mercator de l'origine L93.
        let radius = 6378137.0;
        let x = radius * 3.0_f64.to_radians();
        let y = radius
            * (std::f64::consts::FRAC_PI_4 + 46.5_f64.to_radians() / 2.0)
                .tan()
                .ln();
        let (x, y) = to_lambert93(x, y, "EPSG:3857").unwrap();
        assert!((x - 700000.0).abs() < 0.01);
        assert!((y - 6600000.0).abs() < 0.01);
    }

    #[test]
    fn lambert_coordinates_are_unchanged_and_bad_inputs_are_rejected() {
        assert_eq!(
            to_lambert93(625000.0, 6860000.0, "lambert93").unwrap(),
            (625000.0, 6860000.0)
        );
        assert!(to_lambert93(91.0, 3.0, "latlon").is_err());
        assert!(to_lambert93(48.0, 181.0, "latlon").is_err());
        assert!(to_lambert93(f64::NAN, 3.0, "latlon").is_err());
        assert!(to_lambert93(48.0, 3.0, "EPSG:999999").is_err());
        assert!(normalize_crs("").is_err());
        assert_eq!(normalize_crs("32631").unwrap(), "EPSG:32631");
    }

    #[test]
    fn transformed_bbox_contains_all_sampled_edges_in_longitude_latitude_order() {
        let bbox = Bbox::square(700000.0, 6600000.0, 3000.0);
        let result = bbox_from_lambert93(bbox, "EPSG:4326").unwrap();
        // Le centre est 3° Est / 46,5° Nord : X est bien la longitude en interne.
        assert!(result.xmin < 3.0 && result.xmax > 3.0);
        assert!(result.ymin < 46.5 && result.ymax > 46.5);
        let lambert = Proj::from_user_string(TARGET_CRS).unwrap();
        let wgs84 = Proj::from_user_string("EPSG:4326").unwrap();
        for step in 0..=16 {
            let x = bbox.xmin + (bbox.xmax - bbox.xmin) * step as f64 / 16.0;
            for y in [bbox.ymin, bbox.ymax] {
                let (longitude, latitude) = convert_xy(&lambert, &wgs84, x, y).unwrap();
                assert!(longitude >= result.xmin && longitude <= result.xmax);
                assert!(latitude >= result.ymin && latitude <= result.ymax);
            }
        }
        let unchanged = bbox_from_lambert93(bbox, TARGET_CRS).unwrap();
        assert_eq!((unchanged.xmin, unchanged.ymax), (bbox.xmin, bbox.ymax));
    }
}
