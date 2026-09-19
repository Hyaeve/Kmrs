//! Media pipeline: type sniffing, archive/EPUB/PDF extraction, hashing, thumbnails, metadata.

pub mod container;
pub mod detect;
pub mod error;
pub mod hash;
pub mod image;
pub mod pdf;
pub mod rar;
pub mod zip;

pub use container::PageContent;
pub use error::{MediaError, Result};
