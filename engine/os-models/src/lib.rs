//! Model catalog, local store and downloader.
//!
//! This crate holds the engine's only network code paths: [`download`] and
//! [`hub`] (model search). Every host they can reach is listed in
//! `docs/network.md`.

mod catalog;
mod download;
pub mod hub;
mod store;

pub use catalog::{Catalog, CatalogEntry, FileSpec, Layout, Slot, Source};
pub use download::{DownloadError, Progress, download};
pub use store::{ModelStore, Verify};
