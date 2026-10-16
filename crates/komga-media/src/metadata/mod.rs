//! Metadata providers (komga's `infrastructure/metadata` equivalents): ComicInfo.xml, EPUB OPF,
//! Mylar series.json, ISBN barcodes, and local artwork, plus the patch model they produce.

pub mod artwork;
pub mod barcode;
pub mod comicinfo;
pub mod epub;
pub mod mylar;
pub mod patch;

pub use patch::{
    aggregate, apply_book_patch, apply_series_patch, most_frequent, BookMetadataPatch,
    SeriesMetadataPatch,
};
