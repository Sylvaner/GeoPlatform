//! Contexte : ces types décrivent les données manipulées par le programme.
//! Usage : importer un type avec `use crate::models::Bbox`, par exemple.
//! Chaque fichier contient une structure et, si nécessaire, son bloc `impl`.
//! Les calculs généraux, les appels réseau et les fichiers restent dans les services.

// `mod` déclare un fichier comme sous-module ; il ne copie pas son contenu ici.
mod bbox;
mod cache_metadata;
mod cached_tile;
mod dataset;
mod download_receipt;
mod lidar_tile;
mod tile_matrix;
mod tile_range;
mod wfs_layer;
mod wfs_manifest;
mod wfs_request;
mod wmts_layer;
mod wmts_product;

// `pub use` permet aux autres modules d'importer les types sans connaître
// leur fichier exact : `models::Bbox` au lieu de `models::bbox::Bbox`.
pub use bbox::Bbox;
pub use cache_metadata::CacheMetadata;
pub use cached_tile::CachedTile;
pub use dataset::Dataset;
pub use download_receipt::DownloadReceipt;
pub use lidar_tile::LidarTile;
pub use tile_matrix::TileMatrix;
pub use tile_range::TileRange;
pub use wfs_layer::WfsLayer;
pub use wfs_manifest::WfsManifest;
pub use wfs_request::WfsRequest;
pub use wmts_layer::WmtsLayer;
pub use wmts_product::WmtsProduct;
