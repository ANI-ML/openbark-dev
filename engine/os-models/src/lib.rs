//! Model catalog, local store and downloader.
//!
//! This crate holds the engine's only network code paths: [`download`] and
//! [`hub`] (model search). Every host they can reach is listed in
//! `docs/network.md`.

mod catalog;
mod download;
pub mod gpu_pack;
pub mod hub;
pub mod local;
pub mod locations;
pub mod mlx_pack;
pub mod platform;
pub mod remove;
mod store;

pub use catalog::LlmEngine;
pub use catalog::{Catalog, CatalogEntry, FileSpec, Layout, Slot, Source};
pub use download::{DownloadError, Progress, download};
pub use store::{
    FileHashes, UnsafePath, bundled_models_dir, dir_name, hash_file, hashes_match, safe_rel_path,
};
pub use store::{ModelStore, Verify};
