//! Contexte : le serveur découpe la carte en lignes et colonnes de tuiles.
//! Usage : `geometry.rs` calcule cette plage ; `cache.rs` parcourt ses indices.
//! Les quatre bornes sont incluses et doivent décrire une plage non vide.

/// Rectangle d'indices de tuiles à parcourir.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct TileRange {
    /// Première colonne incluse, comptée depuis la gauche de la grille.
    pub col_min: u64,
    /// Dernière colonne incluse.
    pub col_max: u64,
    /// Première ligne incluse, comptée depuis le haut de la grille.
    pub row_min: u64,
    /// Dernière ligne incluse.
    pub row_max: u64,
}

impl TileRange {
    /// Nombre de tuiles : nombre de colonnes multiplié par nombre de lignes.
    pub fn count(&self) -> u64 {
        // `&self` emprunte cette plage sans la modifier ni en prendre possession.
        // Le +1 tient compte des deux bornes : les indices 2 à 4 donnent 3 tuiles.
        (self.col_max - self.col_min + 1) * (self.row_max - self.row_min + 1)
    }
}
