//! PDF page rendering and extraction via pdfium (`pdfium-render`), ported from `PdfExtractor.kt`.
//!
//! libpdfium is a runtime dependency: it is looked up in `KOMGA_PDFIUM_PATH`, next to the
//! executable, then through the system library search. When unavailable, every operation
//! returns `MediaError::Unsupported`.

use crate::error::{MediaError, Result};
use crate::image;
use pdfium_render::prelude::*;
use std::path::{Path, PathBuf};

const RESOLUTION: f32 = 3200.0;

fn library_paths() -> Vec<PathBuf> {
    let mut paths = vec![];
    if let Ok(custom) = std::env::var("KOMGA_PDFIUM_PATH") {
        paths.push(PathBuf::from(custom));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            paths.push(dir.join(Pdfium::pdfium_platform_library_name()));
        }
    }
    paths
}

fn bind() -> Result<Pdfium> {
    let mut last_err = None;
    for path in library_paths() {
        if path.exists() {
            match Pdfium::bind_to_library(&path) {
                Ok(bindings) => return Ok(Pdfium::new(bindings)),
                Err(e) => last_err = Some(e),
            }
        }
    }
    match Pdfium::bind_to_system_library() {
        Ok(bindings) => Ok(Pdfium::new(bindings)),
        Err(e) => {
            let detail = last_err
                .map(|e| e.to_string())
                .unwrap_or_else(|| e.to_string());
            Err(MediaError::unsupported(format!(
                "libpdfium is not available: {detail}"
            )))
        }
    }
}

/// Whether a usable libpdfium was found; tests skip PDF assertions when false.
pub fn pdf_available() -> bool {
    bind().is_ok()
}

fn load<'a>(pdfium: &'a Pdfium, path: &Path) -> Result<PdfDocument<'a>> {
    pdfium.load_pdf_from_file(path, None).map_err(|e| {
        if matches!(e, PdfiumError::IoError(_)) && !path.exists() {
            MediaError::NoSuchFile(path.display().to_string())
        } else {
            MediaError::unsupported(format!("could not open pdf document: {e}"))
        }
    })
}

/// `PdfExtractor.getPageContentAsImage`: render the page at `3200 / min(cropBox.w, cropBox.h)`
/// and encode as JPEG.
pub fn get_page_content_as_image(path: &Path, page_number: usize) -> Result<Vec<u8>> {
    let pdfium = bind()?;
    let document = load(&pdfium, path)?;
    let page = document
        .pages()
        .get((page_number - 1) as i32)
        .map_err(|_| MediaError::PageOutOfBounds(page_number))?;

    let (width, height) = crop_box_size(&page);
    let scale = RESOLUTION / width.min(height);

    let bitmap = page
        .render_with_config(&PdfRenderConfig::new().scale_page_by_factor(scale))
        .map_err(|e| {
            MediaError::Conversion(format!("could not render pdf page {page_number}: {e}"))
        })?;
    let image = bitmap.as_image().map_err(|e| {
        MediaError::Conversion(format!("could not convert pdf page {page_number}: {e}"))
    })?;

    image::encode_jpeg(&image)
}

/// `PdfExtractor.getPageContentAsPdf`: a new document containing only the requested page.
pub fn get_page_content_as_pdf(path: &Path, page_number: usize) -> Result<Vec<u8>> {
    let pdfium = bind()?;
    let source = load(&pdfium, path)?;
    let mut extracted = pdfium
        .create_new_pdf()
        .map_err(|e| MediaError::unsupported(format!("could not create pdf document: {e}")))?;
    extracted
        .pages_mut()
        .copy_pages_from_document(&source, &page_number.to_string(), 0)
        .map_err(|_| MediaError::PageOutOfBounds(page_number))?;
    extracted
        .save_to_bytes()
        .map_err(|e| MediaError::unsupported(format!("could not save pdf document: {e}")))
}

/// `PdfExtractor.scaleDimension`: dimensions after applying the render scale
pub fn scale_dimension(width: i32, height: i32) -> (i32, i32) {
    let scale = RESOLUTION / (width.min(height) as f32);
    (
        (width as f32 * scale).round() as i32,
        (height as f32 * scale).round() as i32,
    )
}

/// PDFBox's `page.cropBox` falls back to the media box when no crop box is set
fn crop_box_size(page: &PdfPage<'_>) -> (f32, f32) {
    let boundaries = page.boundaries();
    match boundaries.crop().or_else(|_| boundaries.media()) {
        Ok(b) => (b.bounds.width().value, b.bounds.height().value),
        Err(_) => (page.width().value, page.height().value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/resources/pdf/komga.pdf")
    }

    #[test]
    fn render_first_page_as_jpeg() {
        if !pdf_available() {
            eprintln!("libpdfium not available, skipping");
            return;
        }
        let bytes = get_page_content_as_image(&fixture(), 1).unwrap();
        assert_eq!(&bytes[0..3], b"\xFF\xD8\xFF");
        let (w, h) = image::get_dimension(&bytes).unwrap();
        assert_eq!(w.min(h), 3200);
    }

    #[test]
    fn extract_single_page_pdf() {
        if !pdf_available() {
            eprintln!("libpdfium not available, skipping");
            return;
        }
        let bytes = get_page_content_as_pdf(&fixture(), 1).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
    }

    #[test]
    fn out_of_bounds_page() {
        if !pdf_available() {
            eprintln!("libpdfium not available, skipping");
            return;
        }
        assert!(matches!(
            get_page_content_as_image(&fixture(), 999),
            Err(MediaError::PageOutOfBounds(999)) | Err(MediaError::Unsupported { .. })
        ));
    }

    #[test]
    fn scale_dimension_math() {
        assert_eq!(scale_dimension(100, 200), (3200, 6400));
        assert_eq!(scale_dimension(3200, 3200), (3200, 3200));
    }
}
