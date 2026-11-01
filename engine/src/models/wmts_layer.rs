//! Contexte : une couche WMTS peut proposer plusieurs styles et formats de données.
//! Usage : `wmts/capabilities.rs` choisit ces valeurs ; `wmts/mod.rs` les transmet
//! au serveur lors du téléchargement. `cache.rs` les conserve dans les métadonnées.

/// Paramètres retenus pour demander les tuiles d'un produit WMTS.
pub struct WmtsLayer {
    /// Identifiant de couche : altitude ou photographies aériennes BD ORTHO.
    pub identifier: String,
    /// Identifiant du style, généralement « normal ».
    pub style: String,
    /// Type de contenu demandé au serveur, par exemple « image/x-bil;bits=32 ».
    pub format: String,
}
