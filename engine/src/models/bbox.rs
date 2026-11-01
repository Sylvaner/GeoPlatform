//! Contexte : une bbox (bounding box) est un rectangle défini par ses quatre limites.
//! Usage : `Bbox::square(x, y, side)` crée la zone demandée en mètres dans app.rs.
//! coordinates.rs convertit ensuite ses limites dans le CRS de la grille WMTS.
//! Une Bbox ne porte pas son CRS : l'appelant doit connaître l'unité de ses nombres.
//! Ce fichier contient la structure Bbox et les méthodes qui construisent cette valeur.

/// Limites d'une surface, en coordonnées X/Y du système utilisé par l'appelant.
// pub rend la structure et ses champs accessibles aux autres modules.
// f64 est un nombre à virgule flottante sur 64 bits.
// `derive` demande au compilateur de générer des comportements standards :
// - Debug : autorise l'affichage de diagnostic avec `{:?}` ;
// - Clone : autorise une copie explicite avec `.clone()` ;
// - Copy : la valeur se copie automatiquement lorsqu'on la passe à une fonction.
// Ces copies conviennent ici : la structure contient seulement quatre nombres.
#[derive(Debug, Clone, Copy)]
pub struct Bbox {
    /// Bord ouest : plus petite coordonnée X.
    pub xmin: f64,
    /// Bord sud : plus petite coordonnée Y.
    pub ymin: f64,
    /// Bord est : plus grande coordonnée X.
    pub xmax: f64,
    /// Bord nord : plus grande coordonnée Y.
    pub ymax: f64,
}

// `impl Bbox` regroupe les fonctions associées à cette structure.
impl Bbox {
    /// Construit un carré à partir de son centre et de son côté, en mètres.
    /// Le côté doit avoir été validé comme strictement positif par l'appelant.
    // Sans paramètre `self`, cette fonction s'appelle `Bbox::square(...)`.
    // `Self` désigne le type du bloc impl, donc Bbox.
    pub fn square(center_x: f64, center_y: f64, side: f64) -> Self {
        let half_side = side / 2.0;

        // Une expression finale sans point-virgule est la valeur de retour.
        Self {
            xmin: center_x - half_side,
            ymin: center_y - half_side,
            xmax: center_x + half_side,
            ymax: center_y + half_side,
        }
    }
}
