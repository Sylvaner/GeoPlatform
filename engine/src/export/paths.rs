//! Contexte : le cache, le TOML et le résultat peuvent être sur des disques différents.
//! Usage : normaliser les chemins à la frontière avec Blender, sans copier le cache.
//! Sous Windows, Rust accepte les préfixes étendus \\?\ ; Blender attend des chemins usuels.

use std::path::{Path, PathBuf};

/// Conserve le disque ou le serveur, en retirant seulement le préfixe technique Windows.
/// Exemples : \\?\D:\cache devient D:\cache ; \\?\UNC\nas\data devient \\nas\data.
/// Les antislashs d'un chemin réseau sont essentiels : Blender réserve « // » aux
/// chemins relatifs au .blend. On ne transforme donc jamais un UNC en //nas/data.
pub fn for_blender(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let mut components = path.components();
        let prefix = match components.next() {
            Some(Component::Prefix(prefix)) => prefix.kind(),
            _ => return path.to_path_buf(),
        };
        let mut result = match prefix {
            Prefix::VerbatimDisk(drive) => PathBuf::from(format!("{}:\\", char::from(drive))),
            Prefix::VerbatimUNC(server, share) | Prefix::UNC(server, share) => {
                // push accepte les noms Unicode sans les convertir en texte avec pertes.
                let mut root = std::ffi::OsString::from(r"\\");
                root.push(server);
                root.push(r"\");
                root.push(share);
                PathBuf::from(root)
            }
            _ => return path.to_path_buf(),
        };
        // Un chemin non étendu peut mélanger / et \\. Reconstruire ses composants
        // produit un UNC Windows, même si la saisie était //nas/partage/cache.
        for component in components {
            if !matches!(component, Component::RootDir) {
                result.push(component.as_os_str());
            }
        }
        result
    }
    #[cfg(not(windows))]
    path.to_path_buf()
}

/// Les champs du plan JSON sont du texte ; les arguments du processus restent des PathBuf.
pub fn text(path: &Path) -> String {
    for_blender(path).to_string_lossy().into_owned()
}
