//! `ComicInfoProvider.kt` and `ReadListProvider.kt`: ComicInfo.xml parsing and mapping.

use crate::container;
use crate::metadata::patch::{
    bcp47, isbn_validate, BookMetadataPatch, BookMetadataProvider, MetadataPatchTarget,
    MetadataProvider, ReadListEntry, SeriesMetadataFromBookProvider, SeriesMetadataPatch,
};
use komga_core::error::{codes, CodedError};
use komga_core::model::common::{Author, WebLink};
use komga_core::model::library::Library;
use komga_core::model::media::Media;
use komga_core::model::series::ReadingDirection;
use komga_core::task::BookMetadataPatchCapability;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::LazyLock;

const COMIC_INFO: &str = "ComicInfo.xml";

// region dto (`dto/ComicInfo.kt`)

/// The XML shape of `ComicInfo.xml`; unknown elements are ignored.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ComicInfo {
    #[serde(rename = "Title")]
    pub title: Option<String>,
    #[serde(rename = "Series")]
    pub series: Option<String>,
    #[serde(rename = "Number")]
    pub number: Option<String>,
    #[serde(rename = "Count")]
    pub count: Option<i32>,
    #[serde(rename = "Volume")]
    pub volume: Option<i32>,
    #[serde(rename = "AlternateSeries")]
    pub alternate_series: Option<String>,
    #[serde(rename = "AlternateNumber")]
    pub alternate_number: Option<String>,
    #[serde(rename = "AlternateCount")]
    pub alternate_count: Option<i32>,
    #[serde(rename = "Summary")]
    pub summary: Option<String>,
    #[serde(rename = "Notes")]
    pub notes: Option<String>,
    #[serde(rename = "Year")]
    pub year: Option<i32>,
    #[serde(rename = "Month")]
    pub month: Option<i32>,
    #[serde(rename = "Day")]
    pub day: Option<i32>,
    #[serde(rename = "Writer")]
    pub writer: Option<String>,
    #[serde(rename = "Penciller")]
    pub penciller: Option<String>,
    #[serde(rename = "Inker")]
    pub inker: Option<String>,
    #[serde(rename = "Colorist")]
    pub colorist: Option<String>,
    #[serde(rename = "Letterer")]
    pub letterer: Option<String>,
    #[serde(rename = "CoverArtist")]
    pub cover_artist: Option<String>,
    #[serde(rename = "Editor")]
    pub editor: Option<String>,
    #[serde(rename = "Translator")]
    pub translator: Option<String>,
    #[serde(rename = "Publisher")]
    pub publisher: Option<String>,
    #[serde(rename = "Imprint")]
    pub imprint: Option<String>,
    #[serde(rename = "Genre")]
    pub genre: Option<String>,
    #[serde(rename = "Tags")]
    pub tags: Option<String>,
    #[serde(rename = "Web")]
    pub web: Option<String>,
    #[serde(rename = "PageCount")]
    pub page_count: Option<i32>,
    #[serde(rename = "LanguageISO")]
    pub language_iso: Option<String>,
    #[serde(rename = "Format")]
    pub format: Option<String>,
    #[serde(rename = "BlackAndWhite")]
    pub black_and_white: Option<YesNo>,
    #[serde(rename = "Manga")]
    pub manga: Option<Manga>,
    #[serde(rename = "Characters")]
    pub characters: Option<String>,
    #[serde(rename = "Teams")]
    pub teams: Option<String>,
    #[serde(rename = "Locations")]
    pub locations: Option<String>,
    #[serde(rename = "ScanInformation")]
    pub scan_information: Option<String>,
    #[serde(rename = "StoryArc")]
    pub story_arc: Option<String>,
    #[serde(rename = "StoryArcNumber")]
    pub story_arc_number: Option<String>,
    #[serde(rename = "SeriesGroup")]
    pub series_group: Option<String>,
    #[serde(rename = "AgeRating")]
    pub age_rating: Option<AgeRating>,
    #[serde(rename = "GTIN")]
    pub gtin: Option<String>,
}

/// `dto/AgeRating.kt`: the integer age mapped to each rating (UNKNOWN/RATING_PENDING map to none).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgeRating {
    Unknown,
    AdultsOnly18,
    EarlyChildhood,
    Everyone,
    Everyone10,
    G,
    KidsToAdults,
    M,
    Ma15,
    Mature17,
    Pg,
    R18,
    RatingPending,
    Teen,
    X18,
}

impl AgeRating {
    pub fn age_rating(self) -> Option<i32> {
        match self {
            AgeRating::Unknown | AgeRating::RatingPending => None,
            AgeRating::AdultsOnly18 | AgeRating::R18 | AgeRating::X18 => Some(18),
            AgeRating::EarlyChildhood => Some(3),
            AgeRating::Everyone | AgeRating::G => Some(0),
            AgeRating::Everyone10 => Some(10),
            AgeRating::KidsToAdults => Some(6),
            AgeRating::M | AgeRating::Mature17 => Some(17),
            AgeRating::Ma15 => Some(15),
            AgeRating::Pg => Some(8),
            AgeRating::Teen => Some(13),
        }
    }

    /// `fromValue`: case-insensitive, spaces ignored; unknown values yield None (never an error).
    pub fn from_value(value: &str) -> Option<AgeRating> {
        let key = value.to_lowercase().replace(' ', "");
        Some(match key.as_str() {
            "unknown" => AgeRating::Unknown,
            "adultsonly18+" => AgeRating::AdultsOnly18,
            "earlychildhood" => AgeRating::EarlyChildhood,
            "everyone" => AgeRating::Everyone,
            "everyone10+" => AgeRating::Everyone10,
            "g" => AgeRating::G,
            "kidstoadults" => AgeRating::KidsToAdults,
            "m" => AgeRating::M,
            "ma15+" => AgeRating::Ma15,
            "mature17+" => AgeRating::Mature17,
            "pg" => AgeRating::Pg,
            "r18+" => AgeRating::R18,
            "ratingpending" => AgeRating::RatingPending,
            "teen" => AgeRating::Teen,
            "x18+" => AgeRating::X18,
            _ => return None,
        })
    }
}

impl<'de> Deserialize<'de> for AgeRating {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        AgeRating::from_value(&value)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown age rating: {value}")))
    }
}

/// `dto/Manga.kt`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Manga {
    Unknown,
    No,
    Yes,
    YesAndRightToLeft,
}

impl Manga {
    pub fn from_value(value: &str) -> Option<Manga> {
        Some(match value {
            "Unknown" => Manga::Unknown,
            "No" => Manga::No,
            "Yes" => Manga::Yes,
            "YesAndRightToLeft" => Manga::YesAndRightToLeft,
            _ => return None,
        })
    }
}

impl<'de> Deserialize<'de> for Manga {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Manga::from_value(&value)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown manga value: {value}")))
    }
}

/// `dto/YesNo.kt`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YesNo {
    Unknown,
    No,
    Yes,
}

impl<'de> Deserialize<'de> for YesNo {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "Unknown" => Ok(YesNo::Unknown),
            "No" => Ok(YesNo::No),
            "Yes" => Ok(YesNo::Yes),
            _ => Err(serde::de::Error::custom(format!(
                "unknown yes/no value: {value}"
            ))),
        }
    }
}

// endregion

static CAPABILITIES: LazyLock<BTreeSet<BookMetadataPatchCapability>> = LazyLock::new(|| {
    [
        BookMetadataPatchCapability::Title,
        BookMetadataPatchCapability::Summary,
        BookMetadataPatchCapability::Number,
        BookMetadataPatchCapability::NumberSort,
        BookMetadataPatchCapability::ReleaseDate,
        BookMetadataPatchCapability::Authors,
        BookMetadataPatchCapability::ReadLists,
        BookMetadataPatchCapability::Links,
    ]
    .into_iter()
    .collect()
});

pub struct ComicInfoProvider;

impl ComicInfoProvider {
    /// `getComicInfo`: the file must be listed in media.files; parse failures yield None.
    fn get_comic_info(book_path: &Path, media: &Media) -> Option<ComicInfo> {
        if !media.files.iter().any(|f| f.file_name == COMIC_INFO) {
            tracing::debug!("Book does not contain any {COMIC_INFO} file");
            return None;
        }
        let content = container::get_file_content(book_path, media, COMIC_INFO).ok()?;
        match quick_xml::de::from_reader::<_, ComicInfo>(content.as_slice()) {
            Ok(comic_info) => Some(comic_info),
            Err(e) => {
                tracing::error!("Error while retrieving metadata from {COMIC_INFO}: {e}");
                None
            }
        }
    }
}

impl MetadataProvider for ComicInfoProvider {
    fn should_library_handle_patch(&self, library: &Library, target: MetadataPatchTarget) -> bool {
        match target {
            MetadataPatchTarget::Book => library.import_comicinfo_book,
            MetadataPatchTarget::Series => library.import_comicinfo_series,
            MetadataPatchTarget::ReadList => library.import_comicinfo_readlist,
            MetadataPatchTarget::Collection => library.import_comicinfo_collection,
        }
    }
}

impl BookMetadataProvider for ComicInfoProvider {
    fn capabilities(&self) -> &BTreeSet<BookMetadataPatchCapability> {
        &CAPABILITIES
    }

    fn get_book_metadata_from_book(
        &self,
        book_path: &Path,
        media: &Media,
    ) -> Option<BookMetadataPatch> {
        let comic_info = Self::get_comic_info(book_path, media)?;

        // an invalid date aborts the whole patch, like LocalDate.of throwing out of the provider
        let release_date = match comic_info.year {
            Some(year) => {
                let month = time::Month::try_from(comic_info.month.unwrap_or(1) as u8).ok()?;
                Some(
                    time::Date::from_calendar_date(year, month, comic_info.day.unwrap_or(1) as u8)
                        .ok()?,
                )
            }
            None => None,
        };

        let mut authors = vec![];
        for (value, role) in [
            (&comic_info.writer, "writer"),
            (&comic_info.penciller, "penciller"),
            (&comic_info.inker, "inker"),
            (&comic_info.colorist, "colorist"),
            (&comic_info.letterer, "letterer"),
            (&comic_info.cover_artist, "cover"),
            (&comic_info.editor, "editor"),
            (&comic_info.translator, "translator"),
        ] {
            if let Some(list) = split_with_role(value.as_deref(), role) {
                authors.extend(list);
            }
        }

        let mut read_lists = vec![];
        if let Some(alternate_series) = non_blank(comic_info.alternate_series.as_deref()) {
            read_lists.push(ReadListEntry::new(
                alternate_series,
                comic_info
                    .alternate_number
                    .as_deref()
                    .and_then(|n| n.parse::<i32>().ok()),
            ));
        }
        if let Some(story_arc) = &comic_info.story_arc {
            let arcs: Vec<Option<String>> = story_arc
                .split(',')
                .map(|s| non_blank(Some(s.trim())).map(str::to_string))
                .collect();
            let numbers: Option<Vec<Option<i32>>> = comic_info
                .story_arc_number
                .as_deref()
                .map(|n| n.split(',').map(|s| s.trim().parse::<i32>().ok()).collect());
            if let Some(numbers) = numbers.filter(|n| !n.is_empty()) {
                for (arc, number) in arcs.iter().zip(numbers.iter()) {
                    if let (Some(arc), Some(number)) = (arc, number) {
                        read_lists.push(ReadListEntry::new(arc, Some(*number)));
                    }
                }
            } else {
                read_lists.extend(
                    arcs.into_iter()
                        .flatten()
                        .map(|arc| ReadListEntry::new(arc, None)),
                );
            }
        }

        let links: Option<Vec<WebLink>> = comic_info.web.as_deref().and_then(|web| {
            let links: Vec<WebLink> = web
                .split(' ')
                .filter(|s| !s.is_empty())
                .filter_map(|s| {
                    let trimmed = s.trim();
                    host_of(trimmed).map(|host| WebLink {
                        label: host.to_string(),
                        url: trimmed.to_string(),
                    })
                })
                .collect();
            if links.is_empty() {
                None
            } else {
                Some(links)
            }
        });

        let tags: Option<Vec<String>> = comic_info.tags.as_deref().and_then(|tags_str| {
            let mut tags: Vec<String> = vec![];
            for tag in tags_str.split(',') {
                if let Some(tag) = non_blank(Some(tag.trim().to_lowercase().as_str())) {
                    if !tags.iter().any(|t| t == tag) {
                        tags.push(tag.to_string());
                    }
                }
            }
            if tags.is_empty() {
                None
            } else {
                Some(tags)
            }
        });

        let isbn = comic_info.gtin.as_deref().and_then(isbn_validate);

        Some(BookMetadataPatch {
            title: non_blank(comic_info.title.as_deref()).map(str::to_string),
            summary: non_blank(comic_info.summary.as_deref()).map(str::to_string),
            number: non_blank(comic_info.number.as_deref()).map(str::to_string),
            number_sort: comic_info
                .number
                .as_deref()
                .and_then(|n| n.parse::<f32>().ok()),
            release_date,
            authors: if authors.is_empty() {
                None
            } else {
                Some(authors)
            },
            isbn,
            links,
            tags,
            read_lists,
        })
    }
}

impl SeriesMetadataFromBookProvider for ComicInfoProvider {
    fn supports_append_volume(&self) -> bool {
        true
    }

    fn get_series_metadata_from_book(
        &self,
        book_path: &Path,
        media: &Media,
        append_volume_to_title: bool,
    ) -> Option<SeriesMetadataPatch> {
        let comic_info = Self::get_comic_info(book_path, media)?;

        let reading_direction = match comic_info.manga {
            Some(Manga::No) => Some(ReadingDirection::LeftToRight),
            Some(Manga::YesAndRightToLeft) => Some(ReadingDirection::RightToLeft),
            _ => None,
        };

        let genres: Option<BTreeSet<String>> = comic_info.genre.as_deref().and_then(|genre| {
            let genres: BTreeSet<String> = genre
                .split(',')
                .filter_map(|g| non_blank(Some(g.trim())).map(str::to_string))
                .collect();
            if genres.is_empty() {
                None
            } else {
                Some(genres)
            }
        });

        let series = if append_volume_to_title {
            compute_series_from_series_and_volume(comic_info.series.as_deref(), comic_info.volume)
        } else {
            comic_info.series.clone()
        };

        let language = comic_info.language_iso.as_deref().and_then(|iso| {
            if bcp47::is_valid(iso) {
                Some(bcp47::normalize(iso))
            } else {
                None
            }
        });

        let collections: BTreeSet<String> = comic_info
            .series_group
            .as_deref()
            .map(|group| {
                group
                    .split(',')
                    .filter_map(|g| non_blank(Some(g.trim())).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        Some(SeriesMetadataPatch {
            title: series.clone(),
            title_sort: series,
            status: None,
            summary: None,
            reading_direction,
            publisher: non_blank(comic_info.publisher.as_deref()).map(str::to_string),
            age_rating: comic_info.age_rating.and_then(AgeRating::age_rating),
            language,
            genres,
            total_book_count: comic_info.count,
            collections,
        })
    }
}

/// `splitWithRole`: `split(',')` trimmed, blanks dropped; None when nothing remains.
fn split_with_role(value: Option<&str>, role: &str) -> Option<Vec<Author>> {
    let list: Vec<&str> = value
        .map(|v| {
            v.split(',')
                .filter_map(|s| non_blank(Some(s.trim())))
                .collect()
        })
        .unwrap_or_default();
    if list.is_empty() {
        return None;
    }
    Some(
        list.into_iter()
            .map(|name| Author::new(name, role))
            .collect(),
    )
}

/// Kotlin `String?.ifBlank { null }` for `Option<&str>` (trims nothing — checks blank only).
fn non_blank(value: Option<&str>) -> Option<&str> {
    value.filter(|s| !s.trim().is_empty())
}

/// `URI(it).host` for the web links: requires a scheme; the host is the authority part
/// (userinfo and port stripped). Unparseable values are skipped, like the Kotlin catch-all.
fn host_of(uri: &str) -> Option<&str> {
    let after_scheme = uri.split_once("://")?.1;
    let authority = after_scheme.split('/').next()?;
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let host = authority.split(':').next()?;
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

/// `computeSeriesFromSeriesAndVolume`: volume is appended as ` (n)` only when > 1.
pub fn compute_series_from_series_and_volume(
    series: Option<&str>,
    volume: Option<i32>,
) -> Option<String> {
    let series = non_blank(series)?;
    let suffix = match volume {
        Some(v) if v != 1 => format!(" ({v})"),
        _ => String::new(),
    };
    Some(format!("{series}{suffix}"))
}

// region CBL import (`ReadListProvider.kt` + `dto/ReadingList.kt`)

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct ReadingList {
    #[serde(rename = "Name")]
    name: Option<String>,
    #[serde(rename = "Books")]
    books: Option<ReadingListBooks>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct ReadingListBooks {
    #[serde(rename = "Book")]
    book: Vec<CblBook>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct CblBook {
    #[serde(rename = "Series")]
    series: Option<String>,
    #[serde(rename = "Number")]
    number: Option<String>,
    #[serde(rename = "Volume")]
    volume: Option<i32>,
    #[serde(rename = "Year")]
    year: Option<i32>,
    #[serde(rename = "FileName")]
    file_name: Option<String>,
}

/// `domain/model/ReadListRequest.kt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadListRequest {
    pub name: String,
    pub books: Vec<ReadListRequestBook>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadListRequestBook {
    pub series: BTreeSet<String>,
    pub number: String,
}

/// `ReadListProvider.importFromCbl`.
pub fn import_from_cbl(cbl: &[u8]) -> Result<ReadListRequest, CodedError> {
    let reading_list: ReadingList = match quick_xml::de::from_reader(cbl) {
        Ok(list) => list,
        Err(e) => {
            tracing::error!("Error while trying to parse ComicRack ReadingList: {e}");
            return Err(CodedError(codes::ERR_1015));
        }
    };

    let name = non_blank(reading_list.name.as_deref())
        .ok_or(CodedError(codes::ERR_1030))?
        .to_string();

    let books = reading_list.books.map(|b| b.book).unwrap_or_default();
    if books.is_empty() {
        return Err(CodedError(codes::ERR_1029));
    }

    let books = books
        .into_iter()
        .map(|book| {
            let series = non_blank(book.series.as_deref());
            let number = book.number.as_deref();
            let (Some(series), Some(number)) = (series, number) else {
                return Err(CodedError(codes::ERR_1031));
            };
            let mut names = BTreeSet::new();
            if let Some(computed) = compute_series_from_series_and_volume(Some(series), book.volume)
            {
                names.insert(computed);
            }
            if let Some(plain) = non_blank(Some(series)) {
                names.insert(plain.to_string());
            }
            Ok(ReadListRequestBook {
                series: names,
                number: number.trim().to_string(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ReadListRequest { name, books })
}

// endregion

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::patch::apply_series_patch;
    use komga_core::model::media::{BookPage, MediaFile, MediaStatus};
    use komga_core::time_codec::now_utc;
    use std::io::Write;

    fn book_with_comic_info(xml: &str) -> (std::path::PathBuf, Media) {
        let dir = std::env::temp_dir().join(format!("kmrs-comicinfo-{}", uuid_like()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("book.cbz");
        {
            let file = std::fs::File::create(&path).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            writer
                .start_file("ComicInfo.xml", zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(xml.as_bytes()).unwrap();
            writer
                .start_file("page1.png", zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"\x89PNG\r\n\x1a\n").unwrap();
            writer.finish().unwrap();
        }
        let media = Media {
            book_id: "b1".into(),
            status: MediaStatus::Ready,
            media_type: Some("application/zip".into()),
            comment: None,
            page_count: 1,
            pages: vec![BookPage {
                file_name: "page1.png".into(),
                media_type: "image/png".into(),
                width: None,
                height: None,
                file_hash: String::new(),
                file_size: None,
            }],
            files: vec![
                MediaFile {
                    file_name: "ComicInfo.xml".into(),
                    media_type: Some("application/xml".into()),
                    sub_type: None,
                    file_size: Some(xml.len() as i64),
                },
                MediaFile {
                    file_name: "page1.png".into(),
                    media_type: Some("image/png".into()),
                    sub_type: None,
                    file_size: None,
                },
            ],
            extension_class: None,
            extension_value: None,
            epub_divina_compatible: false,
            epub_is_kepub: false,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        };
        (path, media)
    }

    fn uuid_like() -> String {
        format!(
            "{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
    }

    fn library() -> Library {
        komga_core::model::library::Library {
            id: "l1".into(),
            name: "lib".into(),
            root: "file:/data/".into(),
            import_comicinfo_book: true,
            import_comicinfo_series: true,
            import_comicinfo_collection: true,
            import_comicinfo_readlist: false,
            import_comicinfo_series_append_volume: true,
            import_epub_book: true,
            import_epub_series: true,
            import_mylar_series: true,
            import_local_artwork: true,
            import_barcode_isbn: true,
            scan_force_modified_time: false,
            scan_on_startup: false,
            scan_interval: komga_core::model::library::ScanInterval::Every6H,
            scan_cbx: true,
            scan_pdf: true,
            scan_epub: true,
            scan_directory_exclusions: vec![],
            repair_extensions: false,
            convert_to_cbz: false,
            empty_trash_after_scan: false,
            series_cover: komga_core::model::library::SeriesCover::First,
            hash_files: true,
            hash_pages: false,
            hash_koreader: false,
            analyze_dimensions: true,
            oneshots_directory: None,
            unavailable_date: None,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    const FULL_XML: &str = r#"<?xml version="1.0"?>
<ComicInfo>
  <Title>v01</Title>
  <Series>Sandman</Series>
  <Number>010</Number>
  <Count>10</Count>
  <Volume>2020</Volume>
  <AlternateSeries>story arc</AlternateSeries>
  <AlternateNumber>5</AlternateNumber>
  <Summary>sum</Summary>
  <Year>2020</Year>
  <Month>2</Month>
  <Writer>w1, w2</Writer>
  <Penciller>p1</Penciller>
  <Inker>i1</Inker>
  <Colorist>c1</Colorist>
  <Letterer>l1</Letterer>
  <CoverArtist>ca1</CoverArtist>
  <Editor>e1</Editor>
  <Translator>t1</Translator>
  <Publisher>DC</Publisher>
  <Genre>Action, Adventure</Genre>
  <Tags>dark, Occult</Tags>
  <Web>   https://www.comixology.com/Sandman/digital-comic/727888    https://www.comics.com/x/727889   </Web>
  <PageCount>237</PageCount>
  <LanguageISO>en</LanguageISO>
  <Manga>YesAndRightToLeft</Manga>
  <StoryArc>one, two, three</StoryArc>
  <StoryArcNumber>6, 7, 8</StoryArcNumber>
  <SeriesGroup>multiple,collections</SeriesGroup>
  <AgeRating>MA15+</AgeRating>
  <GTIN>9783440077894</GTIN>
</ComicInfo>"#;

    #[test]
    fn book_metadata_full_mapping() {
        let (path, media) = book_with_comic_info(FULL_XML);
        let provider = ComicInfoProvider;
        let patch = provider.get_book_metadata_from_book(&path, &media).unwrap();

        assert_eq!(patch.title.as_deref(), Some("v01"));
        assert_eq!(patch.summary.as_deref(), Some("sum"));
        assert_eq!(patch.number.as_deref(), Some("010"));
        assert_eq!(patch.number_sort, Some(10.0));
        assert_eq!(
            patch.release_date,
            komga_core::time_codec::parse_date("2020-02-01")
        );
        assert_eq!(patch.isbn.as_deref(), Some("9783440077894"));

        let authors = patch.authors.unwrap();
        assert_eq!(authors.len(), 9);
        assert!(authors.contains(&Author::new("w1", "writer")));
        assert!(authors.contains(&Author::new("w2", "writer")));
        assert!(authors.contains(&Author::new("ca1", "cover")));

        assert_eq!(patch.read_lists.len(), 4);
        assert!(patch
            .read_lists
            .contains(&ReadListEntry::new("story arc", Some(5))));
        assert!(patch
            .read_lists
            .contains(&ReadListEntry::new("one", Some(6))));
        assert!(patch
            .read_lists
            .contains(&ReadListEntry::new("three", Some(8))));

        let links = patch.links.unwrap();
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].label, "www.comixology.com");
        assert_eq!(
            links[0].url,
            "https://www.comixology.com/Sandman/digital-comic/727888"
        );
        assert_eq!(links[1].label, "www.comics.com");

        let tags = patch.tags.unwrap();
        assert_eq!(tags, vec!["dark".to_string(), "occult".to_string()]);
    }

    #[test]
    fn series_metadata_full_mapping() {
        let (path, media) = book_with_comic_info(FULL_XML);
        let provider = ComicInfoProvider;
        let patch = provider
            .get_series_metadata_from_book(&path, &media, true)
            .unwrap();

        assert_eq!(patch.title.as_deref(), Some("Sandman (2020)"));
        assert_eq!(patch.title_sort.as_deref(), Some("Sandman (2020)"));
        assert_eq!(patch.reading_direction, Some(ReadingDirection::RightToLeft));
        assert_eq!(patch.publisher.as_deref(), Some("DC"));
        assert_eq!(patch.age_rating, Some(15));
        assert_eq!(patch.language.as_deref(), Some("en"));
        assert_eq!(
            patch.genres,
            Some(
                ["Action".to_string(), "Adventure".to_string()]
                    .into_iter()
                    .collect()
            )
        );
        assert_eq!(patch.total_book_count, Some(10));
        assert_eq!(
            patch.collections,
            ["collections".to_string(), "multiple".to_string()]
                .into_iter()
                .collect()
        );

        let no_append = provider
            .get_series_metadata_from_book(&path, &media, false)
            .unwrap();
        assert_eq!(no_append.title.as_deref(), Some("Sandman"));
    }

    #[test]
    fn blank_values_are_omitted() {
        let xml = r#"<?xml version="1.0"?>
<ComicInfo>
  <Title></Title><Summary></Summary><Number></Number>
  <AlternateSeries></AlternateSeries><StoryArc></StoryArc>
  <Penciller></Penciller><GTIN></GTIN><Web></Web>
  <Genre></Genre><LanguageISO></LanguageISO><Publisher></Publisher><SeriesGroup></SeriesGroup>
</ComicInfo>"#;
        let (path, media) = book_with_comic_info(xml);
        let provider = ComicInfoProvider;
        let patch = provider.get_book_metadata_from_book(&path, &media).unwrap();
        assert!(patch.title.is_none());
        assert!(patch.summary.is_none());
        assert!(patch.number.is_none());
        assert!(patch.number_sort.is_none());
        assert!(patch.authors.is_none());
        assert!(patch.read_lists.is_empty());
        assert!(patch.isbn.is_none());
        assert!(patch.links.is_none());

        let series = provider
            .get_series_metadata_from_book(&path, &media, true)
            .unwrap();
        assert!(series.genres.is_none());
        assert!(series.language.is_none());
        assert!(series.publisher.is_none());
        assert!(series.collections.is_empty());
    }

    #[test]
    fn story_arc_zip_semantics() {
        let provider = ComicInfoProvider;
        let read_lists = |arc: &str, numbers: &str| {
            let xml = format!(
                r#"<?xml version="1.0"?><ComicInfo><StoryArc>{arc}</StoryArc><StoryArcNumber>{numbers}</StoryArcNumber></ComicInfo>"#
            );
            let (path, media) = book_with_comic_info(&xml);
            provider
                .get_book_metadata_from_book(&path, &media)
                .unwrap()
                .read_lists
        };
        // more numbers than arcs: zip truncates
        assert_eq!(
            read_lists("one, two", "6, 7, 8"),
            vec![
                ReadListEntry::new("one", Some(6)),
                ReadListEntry::new("two", Some(7))
            ]
        );
        // fewer numbers than arcs: zip truncates
        assert_eq!(
            read_lists("one, two, three", "6, 7"),
            vec![
                ReadListEntry::new("one", Some(6)),
                ReadListEntry::new("two", Some(7))
            ]
        );
        // invalid pairs are omitted
        assert_eq!(
            read_lists("one, two, three", "6, x, 8"),
            vec![
                ReadListEntry::new("one", Some(6)),
                ReadListEntry::new("three", Some(8))
            ]
        );
        // blank arcs are omitted
        assert_eq!(
            read_lists("one, , three", "6, 7, 8"),
            vec![
                ReadListEntry::new("one", Some(6)),
                ReadListEntry::new("three", Some(8))
            ]
        );
        // no numbers: names only
        let xml =
            r#"<?xml version="1.0"?><ComicInfo><StoryArc>one, two, three</StoryArc></ComicInfo>"#;
        let (path, media) = book_with_comic_info(xml);
        let lists = provider
            .get_book_metadata_from_book(&path, &media)
            .unwrap()
            .read_lists;
        assert_eq!(
            lists,
            vec![
                ReadListEntry::new("one", None),
                ReadListEntry::new("two", None),
                ReadListEntry::new("three", None)
            ]
        );
    }

    #[test]
    fn release_date_rules() {
        let provider = ComicInfoProvider;
        let release = |xml: &str| {
            let (path, media) = book_with_comic_info(xml);
            provider
                .get_book_metadata_from_book(&path, &media)
                .unwrap()
                .release_date
        };
        assert!(
            release(r#"<?xml version="1.0"?><ComicInfo><Month>2</Month></ComicInfo>"#).is_none()
        );
        assert_eq!(
            release(r#"<?xml version="1.0"?><ComicInfo><Year>2020</Year></ComicInfo>"#),
            komga_core::time_codec::parse_date("2020-01-01")
        );
        assert_eq!(
            release(
                r#"<?xml version="1.0"?><ComicInfo><Year>2020</Year><Month>2</Month></ComicInfo>"#
            ),
            komga_core::time_codec::parse_date("2020-02-01")
        );
    }

    #[test]
    fn no_comic_info_file_returns_none() {
        let dir = std::env::temp_dir().join(format!("kmrs-comicinfo-empty-{}", uuid_like()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("book.cbz");
        {
            let file = std::fs::File::create(&path).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            writer
                .start_file("page1.png", zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"\x89PNG\r\n\x1a\n").unwrap();
            writer.finish().unwrap();
        }
        let mut media = Media {
            book_id: "b1".into(),
            status: MediaStatus::Ready,
            media_type: Some("application/zip".into()),
            comment: None,
            page_count: 1,
            pages: vec![],
            files: vec![],
            extension_class: None,
            extension_value: None,
            epub_divina_compatible: false,
            epub_is_kepub: false,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        };
        let provider = ComicInfoProvider;
        assert!(provider
            .get_book_metadata_from_book(&path, &media)
            .is_none());
        media.files.push(MediaFile {
            file_name: "ComicInfo.xml".into(),
            media_type: None,
            sub_type: None,
            file_size: None,
        });
        // file listed but unreadable content (missing from the archive): also None
        assert!(provider
            .get_book_metadata_from_book(&path, &media)
            .is_none());
    }

    #[test]
    fn enum_mappings() {
        assert_eq!(AgeRating::from_value("MA15+"), Some(AgeRating::Ma15));
        assert_eq!(
            AgeRating::from_value("Mature 17+"),
            Some(AgeRating::Mature17)
        );
        assert_eq!(
            AgeRating::from_value("adults only 18+"),
            Some(AgeRating::AdultsOnly18)
        );
        assert_eq!(AgeRating::from_value("Non existent"), None);
        assert_eq!(AgeRating::Mature17.age_rating(), Some(17));
        assert_eq!(AgeRating::Unknown.age_rating(), None);
        assert_eq!(AgeRating::RatingPending.age_rating(), None);
        assert_eq!(
            Manga::from_value("YesAndRightToLeft"),
            Some(Manga::YesAndRightToLeft)
        );
        assert_eq!(Manga::from_value("non existent"), None);
    }

    #[test]
    fn compute_series_rules() {
        assert_eq!(compute_series_from_series_and_volume(None, None), None);
        assert_eq!(compute_series_from_series_and_volume(Some(""), None), None);
        assert_eq!(
            compute_series_from_series_and_volume(Some("Series"), None).as_deref(),
            Some("Series")
        );
        assert_eq!(
            compute_series_from_series_and_volume(Some("Series"), Some(1)).as_deref(),
            Some("Series")
        );
        assert_eq!(
            compute_series_from_series_and_volume(Some("Series"), Some(10)).as_deref(),
            Some("Series (10)")
        );
        assert_eq!(
            compute_series_from_series_and_volume(Some("Series"), Some(2005)).as_deref(),
            Some("Series (2005)")
        );
    }

    #[test]
    fn library_gate() {
        let provider = ComicInfoProvider;
        let lib = library();
        assert!(provider.should_library_handle_patch(&lib, MetadataPatchTarget::Book));
        assert!(provider.should_library_handle_patch(&lib, MetadataPatchTarget::Series));
        assert!(!provider.should_library_handle_patch(&lib, MetadataPatchTarget::ReadList));
        assert!(provider.should_library_handle_patch(&lib, MetadataPatchTarget::Collection));
    }

    #[test]
    fn cbl_import_happy_path() {
        let cbl = br#"<?xml version="1.0"?>
<ReadingList>
  <Name>my read list</Name>
  <Books>
    <Book><Series>series 1</Series><Number> 4 </Number><Volume>2005</Volume></Book>
    <Book><Series>series 2</Series><Number>1</Number></Book>
  </Books>
</ReadingList>"#;
        let request = import_from_cbl(cbl).unwrap();
        assert_eq!(request.name, "my read list");
        assert_eq!(request.books.len(), 2);
        assert_eq!(
            request.books[0].series,
            ["series 1".to_string(), "series 1 (2005)".to_string()]
                .into_iter()
                .collect()
        );
        assert_eq!(request.books[0].number, "4");
        assert_eq!(
            request.books[1].series,
            ["series 2".to_string()].into_iter().collect()
        );
        assert_eq!(request.books[1].number, "1");
    }

    #[test]
    fn cbl_import_error_codes() {
        // parse failure
        assert_eq!(import_from_cbl(b"not xml").unwrap_err().0, codes::ERR_1015);
        // missing name
        let no_name = br#"<?xml version="1.0"?><ReadingList><Books><Book><Series>s</Series><Number>1</Number></Book></Books></ReadingList>"#;
        assert_eq!(import_from_cbl(no_name).unwrap_err().0, codes::ERR_1030);
        // no books
        let no_books = br#"<?xml version="1.0"?><ReadingList><Name>x</Name></ReadingList>"#;
        assert_eq!(import_from_cbl(no_books).unwrap_err().0, codes::ERR_1029);
        // book missing series
        let no_series = br#"<?xml version="1.0"?><ReadingList><Name>x</Name><Books><Book><Number>1</Number></Book></Books></ReadingList>"#;
        assert_eq!(import_from_cbl(no_series).unwrap_err().0, codes::ERR_1031);
        // book missing number
        let no_number = br#"<?xml version="1.0"?><ReadingList><Name>x</Name><Books><Book><Series>s</Series></Book></Books></ReadingList>"#;
        assert_eq!(import_from_cbl(no_number).unwrap_err().0, codes::ERR_1031);
        // blank series
        let blank_series = br#"<?xml version="1.0"?><ReadingList><Name>x</Name><Books><Book><Series> </Series><Number>1</Number></Book></Books></ReadingList>"#;
        assert_eq!(
            import_from_cbl(blank_series).unwrap_err().0,
            codes::ERR_1031
        );
    }

    #[test]
    fn apply_series_patch_smoke() {
        // the patch from a full ComicInfo applies cleanly onto an empty metadata
        let (path, media) = book_with_comic_info(FULL_XML);
        let patch = ComicInfoProvider
            .get_series_metadata_from_book(&path, &media, true)
            .unwrap();
        let metadata = komga_core::model::series::SeriesMetadata {
            series_id: "s1".into(),
            status: komga_core::model::series::SeriesStatus::Ongoing,
            title: "Sandman".into(),
            title_sort: "Sandman".into(),
            summary: String::new(),
            reading_direction: None,
            publisher: String::new(),
            age_rating: None,
            language: String::new(),
            genres: BTreeSet::new(),
            tags: BTreeSet::new(),
            total_book_count: None,
            sharing_labels: BTreeSet::new(),
            links: vec![],
            alternate_titles: vec![],
            status_lock: false,
            title_lock: false,
            title_sort_lock: false,
            summary_lock: false,
            reading_direction_lock: false,
            publisher_lock: false,
            age_rating_lock: false,
            language_lock: false,
            genres_lock: false,
            tags_lock: false,
            total_book_count_lock: false,
            sharing_labels_lock: false,
            links_lock: false,
            alternate_titles_lock: false,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        };
        let applied = apply_series_patch(&patch, &metadata);
        assert_eq!(applied.title, "Sandman (2020)");
        assert_eq!(applied.publisher, "DC");
        assert_eq!(applied.age_rating, Some(15));
        assert_eq!(applied.total_book_count, Some(10));
    }
}
