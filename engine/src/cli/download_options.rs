//! Contexte : ce fichier regroupe les options communes aux téléchargements.
//! Usage : clap remplit les champs ; app.rs, vector.rs ou lidar/mod.rs valide avant de travailler.
//! Les commentaires `///` sur les champs apparaissent aussi dans l'aide du terminal.

use anyhow::{Result, bail};
use clap::Args;
use std::path::PathBuf;

// Args génère la lecture du même ensemble d'options pour chaque produit.
#[derive(Args, Debug, Clone)]
pub struct DownloadOptions {
    /// Centre : latitude longitude pour des coordonnées GPS, sinon X Y.
    /// Avec --crs lonlat uniquement : longitude latitude.
    // Vec<f64> est une liste de nombres à virgule flottante sur 64 bits.
    // Clap lit exactement deux valeurs ; les nombres négatifs sont acceptés.
    #[arg(long, num_args = 2, value_names = ["X", "Y"], allow_hyphen_values = true)]
    pub center: Vec<f64>,

    /// Coordonnées GPS : latlon (défaut), lonlat, wgs84 ; sinon lambert93 ou EPSG:xxxx.
    // `long` crée --crs ; default_value donne la valeur si l'option est absente.
    #[arg(long, default_value = "latlon")]
    pub crs: String,

    /// Longueur du côté du carré, en mètres.
    #[arg(long)]
    pub side: f64,

    /// Répertoire racine. Chaque produit y conserve son sous-dossier, y compris avec ALL.
    // PathBuf stocke un chemin ; « . » désigne le répertoire courant du terminal.
    #[arg(long, default_value = ".")]
    pub root: PathBuf,

    /// Nombre maximal de téléchargements simultanés.
    // usize est un entier adapté aux tailles de collections en mémoire.
    #[arg(long, default_value_t = 6)]
    pub concurrency: usize,

    /// Retélécharge les fichiers déjà présents, y compris les pages BD TOPO.
    // Pour un bool, --force signifie true ; l'absence de l'option signifie false.
    #[arg(long)]
    pub force: bool,

    /// Prépare la sélection sans télécharger les fichiers de données.
    // Clap transforme le nom Rust dry_run en option --dry-run.
    #[arg(long)]
    pub dry_run: bool,
}

impl DownloadOptions {
    /// Refuse les valeurs qui empêcheraient le traitement de la commande.
    // `&self` lit les options sans les déplacer ; Result<()> signifie :
    // succès sans valeur utile (Ok(())), ou erreur accompagnée d'un message.
    pub fn validate(&self) -> Result<()> {
        if self.center.len() != 2 {
            // bail! quitte immédiatement la fonction en renvoyant une erreur.
            bail!("--center attend exactement 2 valeurs");
        }
        // is_finite exclut aussi l'infini et NaN, pas seulement les côtés négatifs.
        if !self.side.is_finite() || self.side <= 0.0 {
            bail!("--side doit être strictement positif");
        }
        if self.concurrency == 0 {
            bail!("--concurrency doit être >= 1");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn options() -> DownloadOptions {
        DownloadOptions {
            center: vec![48.0, 4.0],
            crs: "latlon".into(),
            side: 3000.0,
            root: ".".into(),
            concurrency: 6,
            force: false,
            dry_run: false,
        }
    }
    #[test]
    fn valid_options_are_accepted() {
        assert!(options().validate().is_ok());
    }
    #[test]
    fn center_requires_two_values() {
        for center in [vec![], vec![48.0], vec![48.0, 4.0, 5.0]] {
            let mut input = options();
            input.center = center;
            assert!(input.validate().is_err());
        }
    }
    #[test]
    fn zero_and_negative_sides_are_rejected() {
        for side in [0.0, -1.0] {
            let mut input = options();
            input.side = side;
            assert!(input.validate().is_err());
        }
    }
    #[test]
    fn non_finite_sides_are_rejected() {
        for side in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut input = options();
            input.side = side;
            assert!(input.validate().is_err());
        }
    }
    #[test]
    fn zero_concurrency_is_rejected() {
        let mut input = options();
        input.concurrency = 0;
        assert!(input.validate().is_err());
    }
}
