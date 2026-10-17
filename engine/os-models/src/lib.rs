//! Model catalog, local store and downloader.
//!
//! This crate holds the engine's only network code path: [`download`]. Every
//! host it can reach is listed in `docs/network.md`.

mod catalog;
mod download;
mod store;

pub use catalog::{Catalog, CatalogEntry, FileSpec, Layout, Slot, Source};
pub use download::{DownloadError, Progress, download};
pub use store::{ModelStore, Verify};
