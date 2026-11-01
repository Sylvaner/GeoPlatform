//! Contexte : les chemins Windows étendus et réseau ne sont pas des chemins relatifs Blender.
//! Usage : cargo test --lib export::path_tests ; aucune connexion à un NAS n'est nécessaire.
//! Les tests UNC vérifient les noms ; un petit cache réel vérifie la lecture hors du dossier TOML.

use super::{blender, config, models::ExportArguments, paths};
use crate::test_support as support;
use std::path::{Path, PathBuf};

fn path_text_eq(actual: &str, expected: &str) -> bool {
    #[cfg(windows)]
    {
        actual.eq_ignore_ascii_case(expected)
    }
    #[cfg(not(windows))]
    {
        actual == expected
    }
}

#[cfg(windows)]
#[test]
fn windows_extended_and_unc_paths_keep_their_disk_or_server() {
    for (source, expected) in [
        (r"\\?\D:\Données\cache", r"D:\Données\cache"),
        (
            r"\\?\UNC\nas\partage\Données\cache",
            r"\\nas\partage\Données\cache",
        ),
        (r"\\nas\partage\cache", r"\\nas\partage\cache"),
        ("//nas/partage/cache", r"\\nas\partage\cache"),
        (r"Z:\Données\cache", r"Z:\Données\cache"),
        (r"cache\relatif", r"cache\relatif"),
    ] {
        // Comparer le texte, pas Path : Windows considère certains séparateurs
        // équivalents, tandis que Blender réserve le préfixe // à un autre usage.
        assert_eq!(paths::text(Path::new(source)), expected);
    }
}

#[cfg(windows)]
#[test]
fn another_drive_or_unc_share_is_not_joined_to_the_configuration_folder() {
    let base = Path::new(r"C:\projet\configuration");
    for absolute in [r"D:\cache", r"Z:\cache", r"\\nas\partage\cache"] {
        assert_eq!(config::resolve(base, absolute), PathBuf::from(absolute));
    }
    assert_eq!(
        paths::text(&config::resolve(Path::new(r"\\?\UNC\nas\projet"), "cache")),
        r"\\nas\projet\cache"
    );
}

#[test]
fn toml_literal_strings_preserve_windows_backslashes() {
    let c = config::parse(
        r#"
[paths]
cache_root = '\\nas\Données partagées\cache'
output_file = 'D:\Résultats\scene.blend'
work_directory = 'C:\Travail export'
"#,
    )
    .unwrap();
    assert_eq!(c.paths.cache_root, r"\\nas\Données partagées\cache");
    assert_eq!(c.paths.output_file, r"D:\Résultats\scene.blend");
    assert_eq!(c.paths.work_directory, r"C:\Travail export");
}

#[test]
fn raster_selection_stops_at_each_matrix_and_keeps_the_finest_resolution() {
    let root = support::directory();
    for (name, resolution) in [("coarse", 5.), ("fine", 0.2)] {
        let folder = root.join(name).join("0");
        std::fs::create_dir_all(folder.join("0")).unwrap();
        std::fs::write(
            folder.join("matrix.json"),
            serde_json::json!({
                "matrix":{"nominal_resolution_m_per_px":resolution}
            })
            .to_string(),
        )
        .unwrap();
        // Les fichiers sous une ligne de tuiles sont des données, pas d'autres
        // grilles candidates. Même un nom ressemblant à une métadonnée est ignoré.
        std::fs::write(folder.join("0/matrix.json"), "ceci n'est pas une grille").unwrap();
    }
    assert_eq!(
        super::files::raster(&root).unwrap(),
        Some(root.join("fine/0/matrix.json"))
    );
    support::cleanup(&root);
}

#[test]
fn blender_command_does_not_depend_on_the_terminal_directory() {
    let root = support::directory();
    let folder = root.join("travail séparé avec espaces");
    let script = blender::write_scripts(&folder).unwrap();
    let plan = folder.join("scene-plan.json");
    std::fs::write(&plan, "{}").unwrap();
    // Le fichier n'est pas exécuté : on inspecte les arguments du futur processus.
    let executable = root.join("Blender local.exe");
    std::fs::write(&executable, "test").unwrap();
    let command =
        blender::command(&executable, &script, &plan, &config::parse("").unwrap()).unwrap();
    assert_eq!(command.get_current_dir(), Some(script.parent().unwrap()));
    let arguments: Vec<_> = command.get_args().collect();
    let index = arguments.iter().position(|arg| *arg == "--python").unwrap();
    assert!(path_text_eq(
        &arguments[index + 1].to_string_lossy(),
        &script.to_string_lossy()
    ));
    assert!(path_text_eq(
        &arguments.last().unwrap().to_string_lossy(),
        &plan.to_string_lossy()
    ));
    assert!(path_text_eq(
        &command.get_program().to_string_lossy(),
        &executable.to_string_lossy()
    ));
    support::cleanup(&root);
}

/// Sépare les ressources, la configuration, le travail et le résultat final.
/// Le nuage LAZ et les quatre JPEG sont synthétiques ; aucun cache personnel n'est lu.
fn fixture(root: &Path, prepare_only: bool) -> (PathBuf, PathBuf, PathBuf) {
    let cache = root.join("ressources séparées avec espaces");
    super::tests::native_ortho_cache(&cache);
    let points: Vec<_> = [0., 10., 20.]
        .into_iter()
        .flat_map(|y| {
            [0., 10., 20.].into_iter().map(move |x| {
                (
                    x,
                    y,
                    if x == 10. && y == 10. { 80. } else { 100. },
                    2,
                    false,
                )
            })
        })
        .collect();
    super::tests::write_laz(&cache, "edition", &points, true);
    for (folder, id) in [
        ("BD_TOPAGE/2026/test", "sa:PlanEau_FXX_Topage2026"),
        (
            "BD_TOPAGE/2026/duplicate",
            "sa:SurfaceElementaire_FXX_Topage2026",
        ),
        (
            "CADASTRE/test",
            "CADASTRALPARCELS.PARCELLAIRE_EXPRESS:parcelle",
        ),
    ] {
        let directory = cache.join(folder);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("complete.json"),
            serde_json::json!({
                "source":"fixture", "layer":id, "bbox":[0,0,20,20],
                "sort_by":"id A", "pages":[0], "features":1
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            directory.join("page-000000.geojson"),
            serde_json::json!({
                "type":"FeatureCollection", "features":[{"type":"Feature", "id":"test.1",
                "properties":{}, "geometry":{"type":"Polygon", "coordinates":[
                    [[1,1],[19,1],[19,19],[1,19],[1,1]]
                ]}}]
            })
            .to_string(),
        )
        .unwrap();
    }
    let folder = root.join("configuration à part");
    std::fs::create_dir_all(&folder).unwrap();
    let mut c = config::parse("").unwrap();
    c.area.center = [10., 10.];
    c.area.crs = "EPSG:2154".into();
    c.area.side_m = 20.;
    c.terrain.source = "lidar".into();
    c.terrain.grid_step_m = 10.;
    for (name, layer) in &mut c.layers {
        layer.enabled = matches!(
            name.as_str(),
            "terrain"
                | "orthophoto"
                | "bd_topage.PlanEau"
                | "bd_topage.SurfaceElementaire"
                | "cadastre.parcelle"
        );
    }
    c.paths.cache_root = cache.to_string_lossy().into_owned();
    c.paths.work_directory = root.join("travail séparé").to_string_lossy().into_owned();
    c.paths.output_file = "résultat/scene.blend".into();
    c.paths.prepare_only = prepare_only;
    c.render.preview = false;
    if let Some(executable) = std::env::var_os("GEO_TEST_BLENDER") {
        c.blender.executable = executable.to_string_lossy().into_owned();
    }
    let configuration = folder.join("export.toml");
    std::fs::write(&configuration, toml::to_string_pretty(&c).unwrap()).unwrap();
    (
        configuration,
        PathBuf::from(c.paths.work_directory),
        folder.join(c.paths.output_file),
    )
}

#[test]
fn preparation_reads_resources_outside_the_configuration_directory() {
    let root = support::directory();
    let (configuration, work, output) = fixture(&root, true);
    super::run(ExportArguments {
        config: configuration,
        init_config: None,
    })
    .unwrap();
    let unique_work = std::fs::read_dir(work)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let plan: serde_json::Value =
        serde_json::from_slice(&std::fs::read(unique_work.join("scene-plan.json")).unwrap())
            .unwrap();
    let tiles = plan["ortho"]["tiles"].as_array().unwrap();
    assert!(
        !plan["layers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|layer| layer["name"] == "bd_topage.SurfaceElementaire")
    );
    assert_eq!(tiles.len(), 4);
    for tile in tiles {
        let path = tile["path"].as_str().unwrap();
        assert!(Path::new(path).is_absolute());
        assert!(Path::new(path).is_file());
        assert!(!path.starts_with(r"\\?\"));
    }
    assert!(path_text_eq(
        plan["output"].as_str().unwrap(),
        &paths::text(&output.with_extension("part.blend"))
    ));
    support::cleanup(&root);
}

#[test]
#[ignore = "nécessite Blender installé ; lancer avec --ignored --nocapture"]
fn actual_blender_export_with_separate_resources_and_unicode_paths() {
    let root = support::directory();
    let (configuration, work, output) = fixture(&root, false);
    let c = config::read(&configuration).unwrap();
    let executable = blender::executable(&c, configuration.parent().unwrap()).unwrap();
    super::run(ExportArguments {
        config: configuration,
        init_config: None,
    })
    .unwrap();
    assert!(output.is_file());
    assert!(!output.with_extension("part.blend").exists());
    assert_eq!(std::fs::read_dir(work).unwrap().count(), 0);
    // Rouvrir depuis un autre dossier, après suppression des préparations, prouve
    // aussi que les images sont intégrées et la scène reste utilisable seule.
    // Rendre les sources introuvables à leur ancien emplacement. Ce déplacement
    // reste entièrement dans le dossier synthétique que ce test vient de créer.
    std::fs::rename(
        root.join("ressources séparées avec espaces"),
        root.join("cache déplacé après export"),
    )
    .unwrap();
    let status = std::process::Command::new(executable)
        .current_dir(&root)
        .args(["--background", "--factory-startup"])
        .arg(&output)
        .args(["--python-exit-code", "1", "--python"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/blender_verify.py"))
        .arg("--")
        .arg("--report")
        .arg(root.join("verification.json"))
        .status()
        .unwrap();
    assert!(status.success());
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("verification.json")).unwrap()).unwrap();
    assert_eq!(report["packed_images"].as_array().unwrap().len(), 4);
    assert_eq!(report["water_objects"], 1);
    // Diagnostic manuel facultatif : garder la scène pour contrôler son interface
    // après réouverture. La suite automatique continue à nettoyer ses fichiers.
    if std::env::var_os("GEO_TEST_KEEP_EXPORT").is_some() {
        println!("CONTROL_BLEND {}", output.display());
        return;
    }
    support::cleanup(&root);
}
