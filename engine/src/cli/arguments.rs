//! Contexte : Cli représente la commande complète saisie par l'utilisateur.
//! Usage : `main.rs` appelle `Cli::parse()` ; clap remplit alors cette structure.
//! Exemple : `cargo run -- RGE_ALTI --center 48.2618 4.7387 --side 3000`.

use clap::Parser;

use super::Command;

// `Parser` génère la lecture des arguments et l'aide `--help` à partir des champs.
// `Debug` permet d'afficher le résultat avec `{:?}` pendant le développement.
#[derive(Parser, Debug)]
#[command(
    name = "geo-cache",
    version,
    about = "Cache local de données géographiques IGN/Géoplateforme et Sandre"
)]
pub struct Cli {
    /// Produit à télécharger, ou ALL pour les six produits disponibles.
    // Cet attribut indique que clap doit lire une sous-commande, pas une option.
    #[command(subcommand)]
    pub command: Command,
}

// Vérifie la convention de saisie par défaut à partir d'une commande de terminal.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_and_its_lowercase_alias_select_the_combined_download() {
        for name in ["ALL", "all"] {
            let cli = Cli::try_parse_from([
                "geo-cache",
                name,
                "--center",
                "48.0",
                "4.0",
                "--side",
                "100",
                "--root",
                "cache local",
                "--concurrency",
                "2",
                "--force",
                "--dry-run",
            ])
            .unwrap();
            let Command::All(options) = cli.command else {
                panic!("commande ALL attendue")
            };
            assert_eq!(options.root, std::path::PathBuf::from("cache local"));
            assert_eq!(options.concurrency, 2);
            assert!(options.force);
            assert!(options.dry_run);
        }
    }

    #[test]
    fn parcel_aliases_select_the_cadastre_product() {
        for name in ["CADASTRE", "PARCELLES", "cadastre", "parcelles"] {
            let cli = Cli::try_parse_from([
                "geo-cache",
                name,
                "--center",
                "48.0",
                "4.0",
                "--side",
                "100",
            ])
            .unwrap();
            assert!(matches!(cli.command, Command::Cadastre(_)));
        }
    }
    #[test]
    fn topage_aliases_select_hydrography_and_not_bdtopo_buildings() {
        for name in ["BD_TOPAGE", "TOPAGE", "bd-topage", "bd_topage"] {
            let cli = Cli::try_parse_from([
                "geo-cache",
                name,
                "--center",
                "48.0",
                "4.0",
                "--side",
                "100",
            ])
            .unwrap();
            assert!(matches!(cli.command, Command::BdTopage(_)));
        }
    }

    #[test]
    fn negative_geographic_coordinates_are_parsed_as_numbers() {
        let cli = Cli::try_parse_from([
            "geo-cache",
            "BD_TOPO",
            "--center",
            "48.0",
            "-4.0",
            "--side",
            "100",
        ])
        .unwrap();
        let Command::BdTopo(options) = cli.command else {
            panic!("commande inattendue")
        };
        assert_eq!(options.center, vec![48.0, -4.0]);
    }
    #[test]
    fn incomplete_center_is_rejected() {
        assert!(
            Cli::try_parse_from(["geo-cache", "BD_TOPO", "--center", "48.0", "--side", "100"])
                .is_err()
        );
    }
    #[test]
    fn missing_side_is_rejected() {
        assert!(Cli::try_parse_from(["geo-cache", "BD_TOPO", "--center", "48.0", "4.0"]).is_err());
    }
    #[test]
    fn unknown_product_is_rejected() {
        assert!(
            Cli::try_parse_from([
                "geo-cache",
                "UNKNOWN",
                "--center",
                "48.0",
                "4.0",
                "--side",
                "100"
            ])
            .is_err()
        );
    }

    #[test]
    fn gps_input_defaults_to_latitude_longitude() {
        let cli = Cli::try_parse_from([
            "geo-cache",
            "RGE_ALTI",
            "--center",
            "48.2618",
            "4.7387",
            "--side",
            "3000",
        ])
        .unwrap();
        let Command::RgeAlti(options) = cli.command else {
            panic!("commande inattendue")
        };
        assert_eq!(options.crs, "latlon");
        assert_eq!(options.center, vec![48.2618, 4.7387]);
    }

    #[test]
    fn all_products_share_the_same_download_options() {
        for product in [
            "ALL",
            "all",
            "RGE_ALTI",
            "BD_ORTHO",
            "ORTHO",
            "LIDAR_HD",
            "LIDAR",
            "BD_TOPO",
            "bd-topo",
            "bd_topo",
            "CADASTRE",
            "PARCELLES",
            "cadastre",
            "parcelles",
            "BD_TOPAGE",
            "TOPAGE",
            "bd-topage",
            "bd_topage",
        ] {
            let cli = Cli::try_parse_from([
                "geo-cache",
                product,
                "--center",
                "48.2618",
                "4.7387",
                "--side",
                "3000",
                "--dry-run",
            ])
            .unwrap();
            let options = match cli.command {
                Command::All(options)
                | Command::RgeAlti(options)
                | Command::BdOrtho(options)
                | Command::LidarHd(options)
                | Command::BdTopo(options)
                | Command::Cadastre(options)
                | Command::BdTopage(options) => options,
            };
            assert_eq!(options.crs, "latlon");
            assert_eq!(options.side, 3000.0);
            assert!(options.dry_run);
        }
    }
}
