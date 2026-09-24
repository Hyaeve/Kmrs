//! `EpubMetadataProvider.kt`: EPUB OPF metadata parsing and mapping.

use crate::metadata::patch::{
    bcp47, isbn_validate, BookMetadataPatch, BookMetadataProvider, MetadataPatchTarget,
    MetadataProvider, SeriesMetadataFromBookProvider, SeriesMetadataPatch,
};
use komga_core::model::common::Author;
use komga_core::model::library::Library;
use komga_core::model::media::Media;
use komga_core::model::series::ReadingDirection;
use komga_core::task::BookMetadataPatchCapability;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use time::Date;

const EPUB_MEDIA_TYPE: &str = "application/epub+zip";
const CONTAINER_XML: &str = "META-INF/container.xml";
const OPF_NS: &str = "http://www.idpf.org/2007/opf";

/// MARC relators → komga author roles (`relators` in EpubMetadataProvider.kt)
fn relator_role(code: &str) -> Option<&'static str> {
    Some(match code {
        "aut" => "writer",
        "clr" => "colorist",
        "cov" => "cover",
        "edt" => "editor",
        "art" => "penciller",
        "ill" => "penciller",
        "trl" => "translator",
        _ => return None,
    })
}

/// Outcome of locating the OPF package document inside an EPUB archive.
enum PackageFile {
    /// The archive could not be read (e.g. a transient I/O error).
    Unreadable,
    /// The archive was read but is structurally invalid (no OPF path, bad encoding).
    Invalid,
    /// The OPF document text was read.
    Readable(String),
}

/// Locate the OPF document text via META-INF/container.xml's rootfile, reporting
/// whether the failure was a read error or a structurally invalid document.
fn read_package_file(book_path: &Path) -> PackageFile {
    let container = match crate::zip::get_entry_bytes(book_path, CONTAINER_XML) {
        Ok(container) => container,
        Err(_) => return PackageFile::Unreadable,
    };
    let container = match String::from_utf8(container) {
        Ok(container) => container,
        Err(_) => return PackageFile::Invalid,
    };
    let doc = match roxmltree::Document::parse(&container) {
        Ok(doc) => doc,
        Err(_) => return PackageFile::Invalid,
    };
    let Some(full_path) = doc
        .descendants()
        .find(|n| n.has_tag_name("rootfile"))
        .and_then(|n| n.attribute("full-path"))
    else {
        return PackageFile::Invalid;
    };
    let bytes = match crate::zip::get_entry_bytes(book_path, full_path) {
        Ok(bytes) => bytes,
        Err(_) => return PackageFile::Unreadable,
    };
    match String::from_utf8(bytes) {
        Ok(package) => PackageFile::Readable(package),
        Err(_) => PackageFile::Invalid,
    }
}

/// `getPackageFileContent`: the OPF document text located via META-INF/container.xml's rootfile.
fn get_package_file_content(book_path: &Path) -> Option<String> {
    match read_package_file(book_path) {
        PackageFile::Readable(package) => Some(package),
        _ => None,
    }
}

/// jsoup `Node.text()` collapses whitespace runs into single spaces and trims.
fn jsoup_text(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `Jsoup.clean(html, Safelist.none())`: strip all tags, keep normalized text.
fn clean_html(html: &str) -> String {
    let fragment = scraper::Html::parse_fragment(html);
    jsoup_text(&fragment.root_element().text().collect::<Vec<_>>().join(" "))
}

/// `LocalDate.parse` with ISO_DATE / ISO_LOCAL_DATE / ISO_DATE_TIME fallbacks.
fn parse_date(raw: &str) -> Option<Date> {
    let raw = raw.trim();
    let date_format = time::macros::format_description!("[year]-[month]-[day]");
    Date::parse(raw, date_format)
        .ok()
        .or_else(|| {
            time::PrimitiveDateTime::parse(
                raw,
                time::macros::format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]"),
            )
            .ok()
            .map(|dt| dt.date())
        })
        .or_else(|| {
            time::OffsetDateTime::parse(
                raw,
                &time::format_description::well_known::Iso8601::DEFAULT,
            )
            .ok()
            .map(|dt| dt.date())
        })
}

/// Namespace-agnostic view of the OPF document (jsoup's `*|x` selectors match e.g. `dc:` and
/// `mydc:` alike).
struct Opf<'a> {
    doc: roxmltree::Document<'a>,
}

impl<'a> Opf<'a> {
    fn parse(opf: &'a str) -> Option<Opf<'a>> {
        let options = roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        };
        let doc = roxmltree::Document::parse_with_options(opf, options).ok()?;
        Some(Opf { doc })
    }

    fn metadata_children<'s>(&'s self) -> Vec<roxmltree::Node<'s, 'a>>
    where
        'a: 's,
    {
        self.doc
            .descendants()
            .find(|n| n.has_tag_name("metadata"))
            .map(|m| m.children().filter(roxmltree::Node::is_element).collect())
            .unwrap_or_default()
    }

    fn children_named<'s>(
        &'s self,
        local: &'s str,
    ) -> impl Iterator<Item = roxmltree::Node<'s, 'a>> + 's
    where
        'a: 's,
    {
        self.metadata_children()
            .into_iter()
            .filter(move |n| n.has_tag_name(local))
    }

    fn first_text(&self, local: &str) -> Option<String> {
        self.children_named(local)
            .next()
            .and_then(|n| n.text())
            .map(jsoup_text)
            .filter(|s| !s.is_empty())
    }

    fn meta_with<'s>(
        &'s self,
        property: &'s str,
        scheme: Option<&'s str>,
    ) -> impl Iterator<Item = roxmltree::Node<'s, 'a>> + 's
    where
        'a: 's,
    {
        self.metadata_children().into_iter().filter(move |n| {
            n.has_tag_name("meta")
                && n.attribute("property") == Some(property)
                && scheme.is_none_or(|s| n.attribute("scheme") == Some(s))
        })
    }

    fn spine_progression(&'a self) -> Option<&'a str> {
        self.doc
            .descendants()
            .find(|n| n.has_tag_name("spine"))
            .and_then(|n| n.attribute("page-progression-direction"))
    }
}

pub struct EpubMetadataProvider;

impl EpubMetadataProvider {
    fn capabilities_set() -> &'static BTreeSet<BookMetadataPatchCapability> {
        use std::sync::OnceLock;
        static CAPABILITIES: OnceLock<BTreeSet<BookMetadataPatchCapability>> = OnceLock::new();
        CAPABILITIES.get_or_init(|| {
            [
                BookMetadataPatchCapability::Title,
                BookMetadataPatchCapability::Summary,
                BookMetadataPatchCapability::ReleaseDate,
                BookMetadataPatchCapability::Authors,
                BookMetadataPatchCapability::Isbn,
            ]
            .into_iter()
            .collect()
        })
    }
}

impl BookMetadataProvider for EpubMetadataProvider {
    fn capabilities(&self) -> &BTreeSet<BookMetadataPatchCapability> {
        Self::capabilities_set()
    }

    /// `getBookMetadataFromBook`: title/summary/releaseDate/authors/isbn/number/numberSort
    /// from the OPF. Only EPUB books are handled.
    fn get_book_metadata_from_book(
        &self,
        book_path: &Path,
        media: &Media,
    ) -> Option<BookMetadataPatch> {
        if media.media_type.as_deref() != Some(EPUB_MEDIA_TYPE) {
            return None;
        }
        let package_file = get_package_file_content(book_path)?;
        book_patch_from_package(&package_file)
    }

    fn get_book_metadata_from_book_with_sources(
        &self,
        book_path: &Path,
        media: &Media,
        sources: Option<&crate::CapturedMetadataSources>,
    ) -> Option<BookMetadataPatch> {
        if media.media_type.as_deref() != Some(EPUB_MEDIA_TYPE) {
            return None;
        }
        // reuse the OPF document captured during analysis when available, so the refresh
        // does not re-open the book; fall back to the file otherwise
        let package_file = match sources.and_then(|s| s.epub_opf.as_deref()) {
            Some(bytes) => std::borrow::Cow::Borrowed(std::str::from_utf8(bytes).ok()?),
            None => std::borrow::Cow::Owned(get_package_file_content(book_path)?),
        };
        book_patch_from_package(&package_file)
    }
}

fn book_patch_from_package(package_file: &str) -> Option<BookMetadataPatch> {
    let opf = Opf::parse(package_file)?;

    let title = opf.first_text("title");
    let description = opf
        .children_named("description")
        .next()
        .and_then(|n| n.text())
        .map(clean_html)
        .filter(|s| !s.is_empty());
    let date = opf
        .children_named("date")
        .next()
        .and_then(|n| n.text())
        .and_then(parse_date);

    // refines-id → MARC role for every `<meta property="role" scheme="marc:relators">`
    let author_roles: HashMap<String, String> = opf
        .meta_with("role", Some("marc:relators"))
        .map(|n| {
            (
                n.attribute("refines")
                    .unwrap_or("")
                    .trim_start_matches('#')
                    .to_string(),
                n.text().map(jsoup_text).unwrap_or_default(),
            )
        })
        .collect();

    let creators: Vec<Author> = opf
        .children_named("creator")
        .filter_map(|el| {
            let name = el.text().map(jsoup_text)?;
            if name.is_empty() {
                return None;
            }
            let opf_role = el.attribute((OPF_NS, "role")).filter(|r| !r.is_empty());
            let id = el.attribute("id").filter(|i| !i.is_empty());
            let refine_role = id
                .and_then(|i| author_roles.get(i))
                .filter(|r| !r.is_empty());
            let role = opf_role
                .or(refine_role.map(String::as_str))
                .and_then(relator_role)
                .unwrap_or("writer");
            Some(Author::new(&name, role))
        })
        .collect();
    let authors = if creators.is_empty() {
        None
    } else {
        Some(creators)
    };

    // identifiers are lowercased and stripped of a leading "isbn:"; only the first one is
    // `firstNotNullOfOrNull { isbnValidator.validate(it) }`: scan all identifiers and keep
    // the first one the ISBN validator accepts (invalid ones are just skipped, never fatal)
    let isbn = opf
        .children_named("identifier")
        .filter_map(|identifier| {
            let text = identifier.text()?.to_lowercase();
            let text = text.strip_prefix("isbn:").unwrap_or(&text);
            isbn_validate(text)
        })
        .next();

    let series_index = opf
        .meta_with("belongs-to-collection", None)
        .next()
        .and_then(|n| n.attribute("id"))
        .and_then(|id| {
            opf.meta_with("group-position", None)
                .find(|n| n.attribute("refines") == Some(format!("#{id}").as_str()))
        })
        .and_then(|n| n.text())
        .map(jsoup_text);

    Some(BookMetadataPatch {
        title,
        summary: description,
        number: series_index.clone().filter(|s| !s.is_empty()),
        number_sort: series_index.and_then(|s| s.parse::<f32>().ok()),
        release_date: date,
        authors,
        isbn,
        links: None,
        tags: None,
        read_lists: vec![],
    })
}

impl SeriesMetadataFromBookProvider for EpubMetadataProvider {
    /// `supportsAppendVolume = false`
    fn supports_append_volume(&self) -> bool {
        false
    }

    /// `getSeriesMetadataFromBook`: series/publisher/language/genres/readingDirection from the OPF.
    fn get_series_metadata_from_book(
        &self,
        book_path: &Path,
        media: &Media,
        _append_volume_to_title: bool,
    ) -> Option<SeriesMetadataPatch> {
        if media.media_type.as_deref() != Some(EPUB_MEDIA_TYPE) {
            return None;
        }
        let package_file = get_package_file_content(book_path)?;
        series_patch_from_package(&package_file)
    }

    fn get_series_metadata_from_book_with_sources(
        &self,
        book_path: &Path,
        media: &Media,
        _append_volume_to_title: bool,
        sources: Option<&crate::CapturedMetadataSources>,
    ) -> Option<SeriesMetadataPatch> {
        if media.media_type.as_deref() != Some(EPUB_MEDIA_TYPE) {
            return None;
        }
        // reuse the OPF document captured during analysis when available, so the refresh
        // does not re-open the book; fall back to the file otherwise
        let package_file = match sources.and_then(|s| s.epub_opf.as_deref()) {
            Some(bytes) => std::borrow::Cow::Borrowed(std::str::from_utf8(bytes).ok()?),
            None => std::borrow::Cow::Owned(get_package_file_content(book_path)?),
        };
        series_patch_from_package(&package_file)
    }
}

/// Outcome of reading an EPUB's OPF package document.
pub enum EpubPackageRead {
    /// The archive could not be read (e.g. a transient I/O error); the caller should
    /// not persist a contribution so the next refresh retries.
    Unreadable,
    /// The document was read but is structurally invalid (no OPF path, bad encoding,
    /// unparsable OPF).
    Invalid,
    /// The OPF was read and parsed.
    Parsed(SeriesMetadataPatch),
}

/// Like `get_series_metadata_from_book_with_sources`, but reports why the package is
/// unavailable so the caller can distinguish a transient read failure from an invalid
/// document.
pub fn read_epub_series_patch(
    book_path: &Path,
    media: &Media,
    sources: Option<&crate::CapturedMetadataSources>,
) -> EpubPackageRead {
    if media.media_type.as_deref() != Some(EPUB_MEDIA_TYPE) {
        return EpubPackageRead::Invalid;
    }
    let package_file: std::borrow::Cow<'_, str> = match sources.and_then(|s| s.epub_opf.as_deref())
    {
        Some(bytes) => match std::str::from_utf8(bytes) {
            Ok(package) => std::borrow::Cow::Borrowed(package),
            Err(_) => return EpubPackageRead::Invalid,
        },
        None => match read_package_file(book_path) {
            PackageFile::Unreadable => return EpubPackageRead::Unreadable,
            PackageFile::Invalid => return EpubPackageRead::Invalid,
            PackageFile::Readable(package) => std::borrow::Cow::Owned(package),
        },
    };
    match series_patch_from_package(&package_file) {
        Some(patch) => EpubPackageRead::Parsed(patch),
        None => EpubPackageRead::Invalid,
    }
}

fn series_patch_from_package(package_file: &str) -> Option<SeriesMetadataPatch> {
    let opf = Opf::parse(package_file)?;

    let series = opf
        .meta_with("belongs-to-collection", None)
        .next()
        .and_then(|n| n.text())
        .map(jsoup_text)
        .filter(|s| !s.is_empty());
    let publisher = opf.first_text("publisher");
    let language = opf.first_text("language").and_then(|l| {
        if bcp47::is_valid(&l) {
            Some(bcp47::normalize(&l))
        } else {
            None
        }
    });
    let genres: BTreeSet<String> = opf
        .children_named("subject")
        .filter_map(|n| n.text())
        .map(jsoup_text)
        .filter(|s| !s.is_empty())
        .collect();
    let direction = opf.spine_progression().and_then(|ppd| match ppd {
        "rtl" => Some(ReadingDirection::RightToLeft),
        "ltr" => Some(ReadingDirection::LeftToRight),
        _ => None,
    });

    Some(SeriesMetadataPatch {
        title: series.clone(),
        title_sort: series,
        status: None,
        summary: None,
        reading_direction: direction,
        publisher,
        age_rating: None,
        language,
        genres: if genres.is_empty() {
            None
        } else {
            Some(genres)
        },
        total_book_count: None,
        collections: BTreeSet::new(),
    })
}

impl MetadataProvider for EpubMetadataProvider {
    /// BOOK → importEpubBook, SERIES → importEpubSeries; everything else is rejected.
    fn should_library_handle_patch(&self, library: &Library, target: MetadataPatchTarget) -> bool {
        match target {
            MetadataPatchTarget::Book => library.import_epub_book,
            MetadataPatchTarget::Series => library.import_epub_series,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use komga_core::model::media::MediaStatus;
    use komga_core::time_codec::now_utc;
    use std::io::Write;

    fn media() -> Media {
        Media {
            book_id: "b1".into(),
            status: MediaStatus::Ready,
            media_type: Some(EPUB_MEDIA_TYPE.to_string()),
            comment: None,
            page_count: 0,
            pages: vec![],
            files: vec![],
            extension_class: None,
            extension_value: None,
            epub_divina_compatible: false,
            epub_is_kepub: false,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    fn write_epub(dir: &Path, name: &str, opf: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("mimetype", options).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();
        zip.start_file(CONTAINER_XML, options).unwrap();
        zip.write_all(
            br##"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"##,
        )
        .unwrap();
        zip.start_file("content.opf", options).unwrap();
        zip.write_all(opf.as_bytes()).unwrap();
        zip.finish().unwrap();
        path
    }

    fn opf(body: &str) -> String {
        format!(
            r##"<?xml version='1.0' encoding='utf-8'?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="uuid_id" version="3.0">
  <metadata xmlns:opf="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/">
    {body}
  </metadata>
  <manifest/>
  <spine/>
</package>"##
        )
    }

    fn provider() -> EpubMetadataProvider {
        EpubMetadataProvider
    }

    #[test]
    fn full_book_patch() {
        let dir = std::env::temp_dir().join("kmrs-epub-1");
        std::fs::create_dir_all(&dir).unwrap();
        let book = write_epub(
            &dir,
            "book.epub",
            &opf(r##"
    <dc:title>The Incomplete Theft</dc:title>
    <dc:creator id="id-1" opf:role="edt">Ralph Burke</dc:creator>
    <dc:identifier>isbn:9783440077894</dc:identifier>
    <dc:identifier>URI:http://www.gutenberg.org/65659</dc:identifier>
    <dc:language>en</dc:language>
    <dc:date>2021-06-20T16:00:00+00:00</dc:date>
    <dc:description>&lt;p&gt;A &lt;b&gt;great&lt;/b&gt; book&lt;/p&gt;</dc:description>
    <dc:publisher>Franckh-Kosmos Verlag</dc:publisher>
    <dc:subject> Mystery </dc:subject>
    <dc:subject>Crime</dc:subject>
    <opf:meta property="belongs-to-collection" id="col-1">The Series</opf:meta>
    <opf:meta refines="#col-1" property="group-position">2.5</opf:meta>
            "##),
        );
        let patch = provider()
            .get_book_metadata_from_book(&book, &media())
            .unwrap();
        assert_eq!(patch.title.as_deref(), Some("The Incomplete Theft"));
        assert_eq!(patch.summary.as_deref(), Some("A great book"));
        assert_eq!(
            patch.release_date,
            Date::from_calendar_date(2021, time::Month::June, 20).ok()
        );
        let authors = patch.authors.unwrap();
        assert_eq!(authors.len(), 1);
        // opf:role wins over the default
        assert_eq!(authors[0].name, "Ralph Burke");
        assert_eq!(authors[0].role, "editor");
        assert_eq!(patch.isbn.as_deref(), Some("9783440077894"));
        assert_eq!(patch.number.as_deref(), Some("2.5"));
        assert_eq!(patch.number_sort, Some(2.5));
    }

    #[test]
    fn refines_role_used_when_no_opf_role() {
        let dir = std::env::temp_dir().join("kmrs-epub-2");
        std::fs::create_dir_all(&dir).unwrap();
        let book = write_epub(
            &dir,
            "book.epub",
            &opf(r##"
    <dc:creator id="id-1">Ulf Blanck</dc:creator>
    <dc:creator id="id-3">The Editor</dc:creator>
    <opf:meta refines="#id-1" property="role" scheme="marc:relators">aut</opf:meta>
    <opf:meta refines="#id-3" property="role" scheme="marc:relators">edt</opf:meta>
            "##),
        );
        let patch = provider()
            .get_book_metadata_from_book(&book, &media())
            .unwrap();
        let authors = patch.authors.unwrap();
        assert_eq!(authors[0].role, "writer");
        assert_eq!(authors[1].role, "editor");
    }

    #[test]
    fn default_role_is_writer() {
        let dir = std::env::temp_dir().join("kmrs-epub-3");
        std::fs::create_dir_all(&dir).unwrap();
        let book = write_epub(
            &dir,
            "book.epub",
            &opf(r##"<dc:creator>Someone</dc:creator>"##),
        );
        let patch = provider()
            .get_book_metadata_from_book(&book, &media())
            .unwrap();
        assert_eq!(patch.authors.unwrap()[0].role, "writer");
    }

    #[test]
    fn first_valid_identifier_wins() {
        let dir = std::env::temp_dir().join("kmrs-epub-4");
        std::fs::create_dir_all(&dir).unwrap();
        // same identifier layout as the Panik im Paradies fixture: the first one is not an ISBN
        let book = write_epub(
            &dir,
            "book.epub",
            &opf(r##"
    <dc:title>Panik im Paradies</dc:title>
    <dc:identifier>goodreads:222735</dc:identifier>
    <dc:identifier>isbn:9783440077894</dc:identifier>
            "##),
        );
        let patch = provider()
            .get_book_metadata_from_book(&book, &media())
            .expect("patch is kept: invalid identifiers are skipped, not fatal");
        assert_eq!(patch.isbn.as_deref(), Some("9783440077894"));
        assert_eq!(patch.title.as_deref(), Some("Panik im Paradies"));
    }

    #[test]
    fn no_identifier_keeps_patch() {
        let dir = std::env::temp_dir().join("kmrs-epub-5");
        std::fs::create_dir_all(&dir).unwrap();
        let book = write_epub(
            &dir,
            "book.epub",
            &opf(r##"<dc:title>No Identifiers</dc:title>"##),
        );
        let patch = provider()
            .get_book_metadata_from_book(&book, &media())
            .unwrap();
        assert_eq!(patch.title.as_deref(), Some("No Identifiers"));
        assert!(patch.isbn.is_none());
    }

    #[test]
    fn non_epub_is_skipped() {
        let dir = std::env::temp_dir().join("kmrs-epub-6");
        std::fs::create_dir_all(&dir).unwrap();
        let book = write_epub(&dir, "book.epub", &opf(r##"<dc:title>X</dc:title>"##));
        let mut m = media();
        m.media_type = Some("application/zip".to_string());
        assert!(provider().get_book_metadata_from_book(&book, &m).is_none());
        m.media_type = None;
        assert!(provider().get_book_metadata_from_book(&book, &m).is_none());
    }

    #[test]
    fn series_patch() {
        let dir = std::env::temp_dir().join("kmrs-epub-7");
        std::fs::create_dir_all(&dir).unwrap();
        let book = write_epub(
            &dir,
            "book.epub",
            &opf(r##"
    <dc:publisher>Kosmos</dc:publisher>
    <dc:language>de</dc:language>
    <dc:subject>Kinder- und Jugendbücher</dc:subject>
    <dc:subject>Krimi</dc:subject>
    <opf:meta property="belongs-to-collection" id="col-1">Die drei ??? Kids</opf:meta>
    <opf:meta refines="#col-1" property="group-position">1.5</opf:meta>
    <opf:spine page-progression-direction="rtl"/>
            "##),
        );
        let patch = provider()
            .get_series_metadata_from_book(&book, &media(), true)
            .unwrap();
        assert_eq!(patch.title.as_deref(), Some("Die drei ??? Kids"));
        assert_eq!(patch.publisher.as_deref(), Some("Kosmos"));
        assert_eq!(patch.language.as_deref(), Some("de"));
        assert_eq!(
            patch.genres,
            Some(BTreeSet::from([
                "Kinder- und Jugendbücher".to_string(),
                "Krimi".to_string(),
            ]))
        );
        assert_eq!(patch.reading_direction, Some(ReadingDirection::RightToLeft));
        assert!(patch.collections.is_empty() && patch.status.is_none());
    }

    #[test]
    fn isbn_validation() {
        assert_eq!(
            isbn_validate("9783440077894").as_deref(),
            Some("9783440077894")
        );
        assert_eq!(
            isbn_validate("978-3-16-148410-0").as_deref(),
            Some("9783161484100")
        );
        assert_eq!(
            isbn_validate("0-306-40615-2").as_deref(),
            Some("0306406152")
        );
        assert!(isbn_validate("9783440077895").is_none(), "bad check digit");
        assert!(isbn_validate("123").is_none());
        assert!(
            isbn_validate("1113440077891").is_none(),
            "no 978/979 prefix"
        );
    }
}
