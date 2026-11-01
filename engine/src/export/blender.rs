//! Contexte : le binaire prépare ses scripts puis démarre Blender sans interface.
//! Usage : auto cherche l'exécutable local ; aucun programme n'est téléchargé ou installé.
//! Les chemins sont des arguments de processus, jamais des commandes assemblées dans un shell.

use super::models::ExportConfig;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

pub fn executable(c: &ExportConfig, base: &Path) -> Result<PathBuf> {
    if c.blender.executable != "auto" {
        let candidate = super::config::resolve(base, &c.blender.executable);
        if candidate.is_file() {
            return Ok(candidate);
        }
        bail!("Blender introuvable : {}", candidate.display());
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for folder in std::env::split_paths(&paths) {
            for name in ["blender.exe", "blender"] {
                let file = folder.join(name);
                if file.is_file() {
                    return Ok(file);
                }
            }
        }
    }
    if let Some(programs) = std::env::var_os("ProgramFiles") {
        let root = PathBuf::from(programs).join("Blender Foundation");
        if root.is_dir() {
            let mut candidates = std::fs::read_dir(root)?
                .filter_map(|e| e.ok().map(|e| e.path().join("blender.exe")))
                .filter(|p| p.is_file())
                .collect::<Vec<_>>();
            candidates.sort();
            if let Some(path) = candidates.pop() {
                return Ok(path);
            }
        }
    }
    bail!("Blender n'est pas trouvé. Renseigner blender.executable dans la configuration TOML.")
}

pub fn write_scripts(work: &Path) -> Result<PathBuf> {
    let folder = work.join("blender");
    std::fs::create_dir_all(&folder)?;
    for (name, source) in [
        (
            "scene_outliner.py",
            include_str!("../../blender/scene_outliner.py"),
        ),
        (
            "finalize_outliner.py",
            include_str!("../../blender/finalize_outliner.py"),
        ),
        (
            "scene_collections.py",
            include_str!("../../blender/scene_collections.py"),
        ),
        (
            "scene_water.py",
            include_str!("../../blender/scene_water.py"),
        ),
        ("scene_view.py", include_str!("../../blender/scene_view.py")),
        (
            "scene_texture_library.py",
            include_str!("../../blender/scene_texture_library.py"),
        ),
        (
            "scene_texture_geometry.py",
            include_str!("../../blender/scene_texture_geometry.py"),
        ),
        (
            "export_scene.py",
            include_str!("../../blender/export_scene.py"),
        ),
        (
            "scene_geometry.py",
            include_str!("../../blender/scene_geometry.py"),
        ),
        (
            "scene_materials.py",
            include_str!("../../blender/scene_materials.py"),
        ),
        (
            "scene_trees.py",
            include_str!("../../blender/scene_trees.py"),
        ),
        (
            "scene_setup.py",
            include_str!("../../blender/scene_setup.py"),
        ),
    ] {
        std::fs::write(folder.join(name), source)?;
    }
    Ok(folder.join("export_scene.py"))
}

pub fn launch(executable: &Path, script: &Path, plan: &Path, c: &ExportConfig) -> Result<()> {
    let mut command = command(executable, script, plan, c)?;
    let status = command.status().with_context(|| {
        format!(
            "démarrage de Blender : {} ; travail : {}",
            executable.display(),
            script.display()
        )
    })?;
    if !status.success() {
        bail!("Blender a échoué ({status}) ; le fichier final n'a pas été publié");
    }
    Ok(())
}

/// Construire la commande séparément permet de vérifier ses chemins sans lancer Blender.
pub(super) fn command(
    executable: &Path,
    script: &Path,
    plan: &Path,
    c: &ExportConfig,
) -> Result<std::process::Command> {
    // Tout rendre absolu AVANT de changer de dossier. Le lancement peut venir
    // d'un autre disque ou d'un partage UNC, et ne doit pas dépendre du terminal.
    let absolute = |path: &Path| -> Result<PathBuf> {
        Ok(super::paths::for_blender(
            &std::fs::canonicalize(path).with_context(|| {
                format!(
                    "fichier nécessaire à Blender inaccessible : {}",
                    path.display()
                )
            })?,
        ))
    };
    let executable = absolute(executable)?;
    let script = absolute(script)?;
    let plan = absolute(plan)?;
    let folder = script
        .parent()
        .context("script Blender sans dossier parent")?;
    let mut command = std::process::Command::new(&executable);
    command
        .current_dir(folder)
        .args([
            "--background",
            "--factory-startup",
            "--disable-autoexec",
            "--threads",
        ])
        .arg(c.blender.threads.to_string())
        .args(["--python-exit-code", "1", "--python"])
        .arg(&script)
        .arg("--")
        .arg(&plan);
    Ok(command)
}
