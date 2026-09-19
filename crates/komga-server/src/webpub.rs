//! WebPub manifest generation, ported from `WebPubGenerator.kt` and `OpdsGenerator.kt`, with the
//! DTO shapes of `WepPub.kt` (NON_EMPTY / NON_NULL per class).
//!
//! Link URLs are absolute: the caller passes the request's base URL (`http::base_url::base_url`)
//! plus the path segments that identify the API surface (`api/v1` for REST, `opds/v2` for OPDS).

use komga_core::dto::book::{BookDto, MediaDto};
use komga_core::model::media::{Media, MediaFileSubType};
use komga_core::model::series::{ReadingDirection, SeriesMetadata};
use komga_core::search::MediaProfile;
use komga_media::detect;
use komga_media::image::ImageType;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use time::{Date, OffsetDateTime};

pub const MEDIATYPE_OPDS_JSON: &str = "application/opds+json";
#[allow(dead_code)] // used by the OPDS v2 endpoints
pub const MEDIATYPE_OPDS_PUBLICATION_JSON: &str = "application/opds-publication+json";
#[allow(dead_code)] // used by the OPDS v2 endpoints
pub const MEDIATYPE_OPDS_AUTHENTICATION_JSON: &str = "application/opds-authentication+json";
pub const MEDIATYPE_DIVINA_JSON: &str = "application/divina+json";
pub const MEDIATYPE_WEBPUB_JSON: &str = "application/webpub+json";
#[allow(dead_code)] // used by the OPDS v2 endpoints
pub const MEDIATYPE_PROGRESSION_JSON: &str = "application/vnd.readium.progression+json";

pub const PROFILE_DIVINA: &str = "https://readium.org/webpub-manifest/profiles/divina";
pub const PROFILE_EPUB: &str = "https://readium.org/webpub-manifest/profiles/epub";
pub const PROFILE_PDF: &str = "https://readium.org/webpub-manifest/profiles/pdf";

#[allow(dead_code)] // used by the OPDS v2 endpoints
pub const REL_PROGRESSION_API: &str = "http://www.cantook.com/api/progression";

const CONTEXT_WEBPUB: &str = "https://readium.org/webpub-manifest/context.jsonld";

/// `MediaType.exportType` per media type
fn export_type(media_type: &str) -> &str {
    match media_type {
        detect::APPLICATION_ZIP => "application/vnd.comicbook+zip",
        "application/x-rar-compressed" | detect::APPLICATION_RAR_4 | detect::APPLICATION_RAR_5 => {
            "application/vnd.comicbook-rar"
        }
        other => other,
    }
}

fn profile_of(media: &MediaDto) -> Option<MediaProfile> {
    komga_media::container::media_profile(Some(media.media_type.as_str()))
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct WPLinkDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub templated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternate: Vec<WPLinkDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<WPLinkDto>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, serde_json::Value>,
}

impl WPLinkDto {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn href_type(href: String, type_: &str) -> Self {
        Self {
            href: Some(href),
            type_: Some(type_.to_string()),
            ..Self::new()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WPPublicationDto {
    pub metadata: WPMetadataDto,
    pub links: Vec<WPLinkDto>,
    #[serde(rename = "@context", skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    // NON_NULL class-level inclusion in Jackson: list fields are always present, even empty
    pub images: Vec<WPLinkDto>,
    #[serde(rename = "readingOrder")]
    pub reading_order: Vec<WPLinkDto>,
    pub resources: Vec<WPLinkDto>,
    pub toc: Vec<WPLinkDto>,
    pub landmarks: Vec<WPLinkDto>,
    #[serde(rename = "pageList")]
    pub page_list: Vec<WPLinkDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WPMetadataDto {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    #[serde(rename = "conformsTo", skip_serializing_if = "Option::is_none")]
    pub conforms_to: Option<String>,
    #[serde(rename = "sortAs", skip_serializing_if = "Option::is_none")]
    pub sort_as: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(with = "zoned_date_time_opt", skip_serializing_if = "Option::is_none")]
    pub modified: Option<OffsetDateTime>,
    #[serde(with = "date_opt", skip_serializing_if = "Option::is_none")]
    pub published: Option<Date>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub author: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub translator: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub editor: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artist: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub illustrator: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub letterer: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub penciler: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub colorist: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inker: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contributor: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub publisher: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subject: Vec<String>,
    #[serde(rename = "readingProgression", skip_serializing_if = "Option::is_none")]
    pub reading_progression: Option<WPReadingProgressionDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "numberOfPages", skip_serializing_if = "Option::is_none")]
    pub number_of_pages: Option<i32>,
    #[serde(rename = "belongsTo", skip_serializing_if = "Option::is_none")]
    pub belongs_to: Option<WPBelongsToDto>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub rendition: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct WPBelongsToDto {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub series: Vec<WPContributorDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collection: Vec<WPContributorDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WPContributorDto {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<WPLinkDto>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WPReadingProgressionDto {
    #[serde(rename = "rtl")]
    Rtl,
    #[serde(rename = "ltr")]
    Ltr,
    #[serde(rename = "ttb")]
    Ttb,
    #[serde(rename = "btt")]
    Btt,
    #[serde(rename = "auto")]
    Auto,
}

/// Optional `zoned_date_time` for `modified`
mod zoned_date_time_opt {
    use serde::{Deserializer, Serializer};
    use time::OffsetDateTime;

    pub fn serialize<S: Serializer>(
        dt: &Option<OffsetDateTime>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match dt {
            // `toZonedDateTime()`: rendered in the system zone, like Jackson's ZonedDateTime
            Some(dt) => serializer.serialize_str(&komga_core::time_codec::format_offset_date_time(
                komga_core::time_codec::to_zoned_date_time(*dt),
            )),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<OffsetDateTime>, D::Error> {
        Ok(Some(time::serde::iso8601::deserialize(deserializer)?))
    }
}

/// `yyyy-MM-dd` for `Option<Date>` fields
mod date_opt {
    use serde::{Deserializer, Serializer};
    use time::Date;

    pub fn serialize<S: Serializer>(date: &Option<Date>, serializer: S) -> Result<S::Ok, S::Error> {
        match date {
            Some(d) => serializer.serialize_str(&super::format_date(*d)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Date>, D::Error> {
        let s = String::deserialize(deserializer)?;
        komga_core::time_codec::parse_date(&s)
            .map(Some)
            .ok_or_else(|| serde::de::Error::custom(format!("invalid date: {s}")))
    }

    use serde::Deserialize;
}

fn format_date(d: Date) -> String {
    komga_core::time_codec::format_date(d)
}

/// `EpubTocEntry` as needed by the EPUB manifest (recursive)
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct EpubTocEntryView {
    pub title: Option<String>,
    pub href: Option<String>,
    #[serde(default)]
    pub children: Vec<EpubTocEntryView>,
}

/// `MediaExtensionEpub` projection needed by the EPUB manifest
#[derive(Debug, Clone, Default)]
pub struct MediaExtensionEpubView {
    pub toc: Vec<EpubTocEntryView>,
    pub landmarks: Vec<EpubTocEntryView>,
    pub page_list: Vec<EpubTocEntryView>,
    pub is_fixed_layout: Option<bool>,
}

/// Decodes the toc/landmarks/pageList/isFixedLayout parts of the gzipped extension blob.
/// Malformed input yields None, like `deserializeMediaExtension` returning null.
pub fn decode_epub_extension_view(blob: Option<&[u8]>) -> Option<MediaExtensionEpubView> {
    use std::io::Read;
    let blob = blob?;
    let mut json = vec![];
    flate2::read::GzDecoder::new(blob)
        .read_to_end(&mut json)
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&json).ok()?;
    let entries = |key: &str| {
        value
            .get(key)
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    };
    Some(MediaExtensionEpubView {
        toc: entries("toc"),
        landmarks: entries("landmarks"),
        page_list: entries("pageList"),
        is_fixed_layout: value.get("isFixedLayout").and_then(|v| v.as_bool()),
    })
}

/// Book links shared by every manifest (`BookDto.toWPLinkDtos`):
/// self manifest, optional divina manifest, acquisition file, plus extras.
fn book_link_dtos(
    book: &BookDto,
    base: &str,
    segments: &[&str],
    extra_links: Vec<WPLinkDto>,
    properties: &BTreeMap<String, serde_json::Value>,
) -> Vec<WPLinkDto> {
    let book_base = book_url(base, segments, &format!("books/{}", book.id));
    let self_type = match profile_of(&book.media) {
        Some(MediaProfile::Divina) => MEDIATYPE_DIVINA_JSON,
        _ => MEDIATYPE_WEBPUB_JSON,
    };
    let mut links = vec![WPLinkDto {
        rel: Some("self".to_string()),
        href: Some(format!("{book_base}/manifest")),
        type_: Some(self_type.to_string()),
        properties: properties.clone(),
        ..WPLinkDto::new()
    }];
    let profile = profile_of(&book.media);
    if profile == Some(MediaProfile::Pdf)
        || (profile == Some(MediaProfile::Epub) && book.media.epub_divina_compatible)
    {
        links.push(WPLinkDto {
            href: Some(format!("{book_base}/manifest/divina")),
            type_: Some(MEDIATYPE_DIVINA_JSON.to_string()),
            properties: properties.clone(),
            ..WPLinkDto::new()
        });
    }
    links.push(WPLinkDto {
        rel: Some("http://opds-spec.org/acquisition".to_string()),
        type_: Some(export_type(&book.media.media_type).to_string()),
        href: Some(format!("{book_base}/file")),
        properties: properties.clone(),
        ..WPLinkDto::new()
    });
    links.extend(extra_links);
    links
}

fn book_url(base: &str, segments: &[&str], suffix: &str) -> String {
    let mut url = base.trim_end_matches('/').to_string();
    crate::http::base_url::path_segment(&mut url, segments);
    let suffix = suffix.trim_start_matches('/');
    if !suffix.is_empty() {
        url.push('/');
        url.push_str(suffix);
    }
    url
}

/// `buildThumbnailLinkDtos`: thumbnail link with `properties` from the caller
fn build_thumbnail_link_dtos(
    book_id: &str,
    base: &str,
    segments: &[&str],
    thumbnail_media_type: &str,
    properties: &BTreeMap<String, serde_json::Value>,
) -> Vec<WPLinkDto> {
    vec![WPLinkDto {
        href: Some(book_url(
            base,
            segments,
            &format!("books/{book_id}/thumbnail"),
        )),
        type_: Some(thumbnail_media_type.to_string()),
        properties: properties.clone(),
        ..WPLinkDto::new()
    }]
}

/// `WebPubGenerator.toBasePublicationDto` (with `OpdsGenerator` overrides via parameters)
pub fn to_base_publication_dto(
    book: &BookDto,
    base: &str,
    segments: &[&str],
    series_link: Option<WPLinkDto>,
    extra_links: Vec<WPLinkDto>,
    properties: &BTreeMap<String, serde_json::Value>,
) -> WPPublicationDto {
    WPPublicationDto {
        context: Some(CONTEXT_WEBPUB.to_string()),
        metadata: to_wp_metadata_dto(book, series_link),
        links: book_link_dtos(book, base, segments, extra_links, properties),
        images: vec![],
        reading_order: vec![],
        resources: vec![],
        toc: vec![],
        landmarks: vec![],
        page_list: vec![],
    }
}

fn to_wp_metadata_dto(book: &BookDto, series_link: Option<WPLinkDto>) -> WPMetadataDto {
    let mut metadata = WPMetadataDto {
        title: book.metadata.title.clone(),
        identifier: if book.metadata.isbn.is_empty() {
            None
        } else {
            Some(format!("urn:isbn:{}", book.metadata.isbn))
        },
        type_: None,
        conforms_to: None,
        sort_as: None,
        subtitle: None,
        modified: Some(book.last_modified),
        published: book.metadata.release_date,
        language: None,
        author: vec![],
        translator: vec![],
        editor: vec![],
        artist: vec![],
        illustrator: vec![],
        letterer: vec![],
        penciler: vec![],
        colorist: vec![],
        inker: vec![],
        contributor: vec![],
        publisher: vec![],
        subject: book.metadata.tags.iter().cloned().collect(),
        reading_progression: None,
        description: if book.metadata.summary.is_empty() {
            None
        } else {
            Some(book.metadata.summary.clone())
        },
        number_of_pages: Some(book.media.pages_count),
        belongs_to: Some(WPBelongsToDto {
            series: vec![WPContributorDto {
                name: book.series_title.clone(),
                position: Some(book.metadata.number_sort),
                links: series_link.map(|l| vec![l]).unwrap_or_default(),
            }],
            collection: vec![],
        }),
        rendition: BTreeMap::new(),
    };
    with_authors(&mut metadata, &book.metadata.authors);
    metadata
}

const WP_KNOWN_ROLES: [&str; 10] = [
    "author",
    "translator",
    "editor",
    "artist",
    "illustrator",
    "letterer",
    "penciler",
    "penciller",
    "colorist",
    "inker",
];

fn with_authors(metadata: &mut WPMetadataDto, authors: &[komga_core::dto::common::AuthorDto]) {
    for author in authors {
        let bucket: &mut Vec<String> = match author.role.as_str() {
            "author" => &mut metadata.author,
            "translator" => &mut metadata.translator,
            "editor" => &mut metadata.editor,
            "artist" => &mut metadata.artist,
            "illustrator" => &mut metadata.illustrator,
            "letterer" => &mut metadata.letterer,
            "penciler" | "penciller" => &mut metadata.penciler,
            "colorist" => &mut metadata.colorist,
            "inker" => &mut metadata.inker,
            _ => &mut metadata.contributor,
        };
        bucket.push(author.name.clone());
    }
    let _ = WP_KNOWN_ROLES;
}

fn with_series_metadata(metadata: &mut WPMetadataDto, series_metadata: &SeriesMetadata) {
    metadata.language = if series_metadata.language.is_empty() {
        None
    } else {
        Some(series_metadata.language.clone())
    };
    metadata.reading_progression = series_metadata.reading_direction.map(|d| match d {
        ReadingDirection::LeftToRight => WPReadingProgressionDto::Ltr,
        ReadingDirection::RightToLeft => WPReadingProgressionDto::Rtl,
        ReadingDirection::Vertical | ReadingDirection::Webtoon => WPReadingProgressionDto::Ttb,
    });
}

/// `WebPubGenerator.toManifestDivina`
pub fn to_manifest_divina(
    book: &BookDto,
    media: &Media,
    series_metadata: &SeriesMetadata,
    base: &str,
    segments: &[&str],
    thumbnail_media_type: &str,
) -> WPPublicationDto {
    let mut publication =
        to_base_publication_dto(book, base, segments, None, vec![], &BTreeMap::new());
    publication
        .metadata
        .with_series_metadata_mut(series_metadata);
    publication.metadata.conforms_to = Some(PROFILE_DIVINA.to_string());

    let pages = if profile_of(&book.media) == Some(MediaProfile::Pdf) {
        komga_media::container::get_pdf_pages_dynamic(media).unwrap_or_default()
    } else {
        media.pages.clone()
    };

    let recommended = [detect::IMAGE_JPEG, detect::IMAGE_PNG, detect::IMAGE_GIF];
    publication.reading_order = pages
        .iter()
        .enumerate()
        .map(|(index, page)| {
            let href = format!(
                "{}/pages/{}?contentNegotiation=false",
                book_url(base, segments, &format!("books/{}", book.id)),
                index + 1
            );
            let alternate = if !recommended.contains(&page.media_type.as_str())
                && komga_media::image::can_convert(&page.media_type, ImageType::Jpeg)
            {
                vec![WPLinkDto {
                    href: Some(format!("{href}&convert=jpeg")),
                    type_: Some(detect::IMAGE_JPEG.to_string()),
                    width: page.width,
                    height: page.height,
                    ..WPLinkDto::new()
                }]
            } else {
                vec![]
            };
            WPLinkDto {
                href: Some(href),
                type_: Some(page.media_type.clone()),
                width: page.width,
                height: page.height,
                alternate,
                ..WPLinkDto::new()
            }
        })
        .collect();
    publication.resources = build_thumbnail_link_dtos(
        &book.id,
        base,
        segments,
        thumbnail_media_type,
        &BTreeMap::new(),
    );
    publication
}

trait WithSeriesMetadataMut {
    fn with_series_metadata_mut(&mut self, series_metadata: &SeriesMetadata);
}

impl WithSeriesMetadataMut for WPMetadataDto {
    fn with_series_metadata_mut(&mut self, series_metadata: &SeriesMetadata) {
        with_series_metadata(self, series_metadata);
    }
}

/// `WebPubGenerator.toManifestPdf`
pub fn to_manifest_pdf(
    book: &BookDto,
    media: &Media,
    series_metadata: &SeriesMetadata,
    base: &str,
    segments: &[&str],
    thumbnail_media_type: &str,
) -> WPPublicationDto {
    let mut publication =
        to_base_publication_dto(book, base, segments, None, vec![], &BTreeMap::new());
    publication
        .metadata
        .with_series_metadata_mut(series_metadata);
    publication.metadata.conforms_to = Some(PROFILE_PDF.to_string());
    let book_base = book_url(base, segments, &format!("books/{}", book.id));
    publication.reading_order = (0..media.page_count)
        .map(|index| {
            WPLinkDto::href_type(
                format!("{book_base}/pages/{}/raw", index + 1),
                detect::APPLICATION_PDF,
            )
        })
        .collect();
    publication.resources = build_thumbnail_link_dtos(
        &book.id,
        base,
        segments,
        thumbnail_media_type,
        &BTreeMap::new(),
    );
    publication
}

/// `WebPubGenerator.toManifestEpub`
pub fn to_manifest_epub(
    book: &BookDto,
    media: &Media,
    extension: Option<&MediaExtensionEpubView>,
    series_metadata: &SeriesMetadata,
    base: &str,
    segments: &[&str],
    thumbnail_media_type: &str,
) -> WPPublicationDto {
    let mut publication =
        to_base_publication_dto(book, base, segments, None, vec![], &BTreeMap::new());
    publication
        .metadata
        .with_series_metadata_mut(series_metadata);
    publication.metadata.conforms_to = Some(PROFILE_EPUB.to_string());
    if let Some(fixed) = extension.and_then(|e| e.is_fixed_layout) {
        publication.metadata.rendition.insert(
            "layout".to_string(),
            if fixed { "fixed" } else { "reflowable" }.to_string(),
        );
    }

    let resource_base = book_url(base, segments, &format!("books/{}/resource", book.id));
    publication.reading_order = media
        .files
        .iter()
        .filter(|f| f.sub_type == Some(MediaFileSubType::EpubPage))
        .map(|f| WPLinkDto {
            href: Some(format!("{}/{}", resource_base, f.file_name)),
            type_: f.media_type.clone(),
            ..WPLinkDto::new()
        })
        .collect();

    let mut resources = build_thumbnail_link_dtos(
        &book.id,
        base,
        segments,
        thumbnail_media_type,
        &BTreeMap::new(),
    );
    resources.extend(
        media
            .files
            .iter()
            .filter(|f| f.sub_type == Some(MediaFileSubType::EpubAsset))
            .map(|f| WPLinkDto {
                href: Some(format!("{}/{}", resource_base, f.file_name)),
                type_: f.media_type.clone(),
                ..WPLinkDto::new()
            }),
    );
    publication.resources = resources;

    let toc_entries = |entries: &[EpubTocEntryView]| -> Vec<WPLinkDto> {
        entries
            .iter()
            .map(|entry| toc_entry_to_link(entry, &resource_base))
            .collect()
    };
    if let Some(extension) = extension {
        publication.toc = toc_entries(&extension.toc);
        publication.landmarks = toc_entries(&extension.landmarks);
        publication.page_list = toc_entries(&extension.page_list);
    }
    publication
}

fn toc_entry_to_link(entry: &EpubTocEntryView, resource_base: &str) -> WPLinkDto {
    let href = entry.href.as_ref().map(|href| {
        let fragment = href.rsplit_once('#').map(|(_, f)| f.to_string());
        let path = match &fragment {
            Some(_) => href.rsplit_once('#').map(|(p, _)| p).unwrap_or(href),
            None => href,
        };
        let mut url = format!("{resource_base}/");
        // Spring's `path()` appends without encoding (spaces and non-ASCII go through as-is)
        url.push_str(path);
        match fragment {
            Some(f) if !f.is_empty() => format!("{url}#{f}"),
            _ => url,
        }
    });
    WPLinkDto {
        title: entry.title.clone(),
        href,
        children: entry
            .children
            .iter()
            .map(|c| toc_entry_to_link(c, resource_base))
            .collect(),
        ..WPLinkDto::new()
    }
}

/// `OpdsGenerator.toOpdsPublicationDto`: base publication plus thumbnail images, the OPDS
/// series link in `belongsTo`, and the progression link with the `authenticate` property.
#[allow(dead_code)] // used by the OPDS v2 endpoints
pub fn to_opds_publication_dto(
    book: &BookDto,
    base: &str,
    thumbnail_media_type: &str,
) -> WPPublicationDto {
    let segments = ["opds", "v2"];
    let auth_href = book_url(base, &segments, "auth");
    let mut properties = BTreeMap::new();
    properties.insert(
        "authenticate".to_string(),
        serde_json::json!({
            "href": auth_href,
            "type": MEDIATYPE_OPDS_AUTHENTICATION_JSON,
        }),
    );
    let series_link = WPLinkDto {
        href: Some(book_url(
            base,
            &segments,
            &format!("series/{}", book.series_id),
        )),
        type_: Some(MEDIATYPE_OPDS_JSON.to_string()),
        ..WPLinkDto::new()
    };
    let progression_link = WPLinkDto {
        type_: Some(MEDIATYPE_PROGRESSION_JSON.to_string()),
        rel: Some(REL_PROGRESSION_API.to_string()),
        href: Some(book_url(
            base,
            &segments,
            &format!("books/{}/progression", book.id),
        )),
        properties: properties.clone(),
        ..WPLinkDto::new()
    };
    let mut publication = to_base_publication_dto(
        book,
        base,
        &segments,
        Some(series_link),
        vec![progression_link],
        &properties,
    );
    publication.images =
        build_thumbnail_link_dtos(&book.id, base, &segments, thumbnail_media_type, &properties);
    publication
}

#[cfg(test)]
mod tests {
    use super::*;
    use komga_core::dto::book::{BookDto, BookMetadataDto, MediaDto};
    use komga_core::dto::common::{AuthorDto, WebLinkDto};
    use komga_core::model::media::{BookPage, MediaFile, MediaStatus};
    use komga_core::time_codec::{now_utc, parse_date};
    use std::collections::BTreeSet;

    fn sample_book() -> BookDto {
        let created = now_utc();
        BookDto {
            id: "b1".into(),
            series_id: "s1".into(),
            series_title: "Berserk".into(),
            library_id: "l1".into(),
            name: "v01".into(),
            url: "/data/berserk/v01.cbz".into(),
            number: 1,
            created,
            last_modified: created,
            file_last_modified: created,
            size_bytes: 1000,
            size: "1000 B".into(),
            media: MediaDto {
                status: "READY".into(),
                media_type: detect::APPLICATION_ZIP.to_string(),
                pages_count: 2,
                comment: String::new(),
                epub_divina_compatible: false,
                epub_is_kepub: false,
                media_profile: "DIVINA".into(),
            },
            metadata: BookMetadataDto {
                title: "Berserk v01".into(),
                title_lock: false,
                summary: "Guts saga".into(),
                summary_lock: false,
                number: "1".into(),
                number_lock: false,
                number_sort: 1.0,
                number_sort_lock: false,
                release_date: parse_date("1990-08-25"),
                release_date_lock: false,
                authors: vec![
                    AuthorDto {
                        name: "Kentaro Miura".into(),
                        role: "writer".into(),
                    },
                    AuthorDto {
                        name: "Studio Gaga".into(),
                        role: "penciller".into(),
                    },
                ],
                authors_lock: false,
                tags: ["seinen"]
                    .into_iter()
                    .map(String::from)
                    .collect::<BTreeSet<_>>(),
                tags_lock: false,
                isbn: "9781593070205".into(),
                isbn_lock: false,
                links: vec![WebLinkDto {
                    label: "wiki".into(),
                    url: "https://example.org".into(),
                }],
                links_lock: false,
                created,
                last_modified: created,
            },
            read_progress: None,
            deleted: false,
            file_hash: String::new(),
            oneshot: false,
        }
    }

    fn sample_series_metadata() -> SeriesMetadata {
        SeriesMetadata {
            series_id: "s1".into(),
            status: komga_core::model::series::SeriesStatus::Ongoing,
            title: "Berserk".into(),
            title_sort: "Berserk".into(),
            summary: String::new(),
            reading_direction: Some(ReadingDirection::RightToLeft),
            publisher: "Hakusensha".into(),
            age_rating: Some(18),
            language: "ja".into(),
            genres: BTreeSet::new(),
            tags: BTreeSet::new(),
            total_book_count: Some(41),
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
        }
    }

    fn media_zip() -> Media {
        Media {
            book_id: "b1".into(),
            status: MediaStatus::Ready,
            media_type: Some(detect::APPLICATION_ZIP.to_string()),
            comment: None,
            page_count: 2,
            pages: vec![
                BookPage {
                    file_name: "p1.png".into(),
                    media_type: detect::IMAGE_PNG.to_string(),
                    width: Some(800),
                    height: Some(1200),
                    file_hash: String::new(),
                    file_size: None,
                },
                BookPage {
                    file_name: "p2.webp".into(),
                    media_type: detect::IMAGE_WEBP.to_string(),
                    width: Some(800),
                    height: Some(1200),
                    file_hash: String::new(),
                    file_size: None,
                },
            ],
            files: vec![],
            extension_class: None,
            extension_value: None,
            epub_divina_compatible: false,
            epub_is_kepub: false,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    const BASE: &str = "http://localhost:25600";
    const SEGMENTS: &[&str] = &["api", "v1"];

    #[test]
    fn publication_json_shape() {
        let publication = to_base_publication_dto(
            &sample_book(),
            BASE,
            SEGMENTS,
            None,
            vec![],
            &BTreeMap::new(),
        );
        let json = serde_json::to_value(&publication).unwrap();
        assert_eq!(json["@context"], CONTEXT_WEBPUB);
        let metadata = &json["metadata"];
        assert_eq!(metadata["title"], "Berserk v01");
        assert_eq!(metadata["identifier"], "urn:isbn:9781593070205");
        assert_eq!(metadata["numberOfPages"], 2);
        assert_eq!(metadata["published"], "1990-08-25");
        assert_eq!(metadata["subject"], serde_json::json!(["seinen"]));
        assert_eq!(metadata["belongsTo"]["series"][0]["name"], "Berserk");
        assert_eq!(metadata["belongsTo"]["series"][0]["position"], 1.0);
        // NON_EMPTY: empty vectors and null fields are absent
        assert!(metadata.get("author").is_none());
        assert!(metadata.get("translator").is_none());
        assert!(metadata.get("language").is_none());
        assert!(metadata.get("rendition").is_none());
        // NON_NULL on the publication: list fields are always present, even empty
        assert_eq!(json["images"], serde_json::json!([]));
        assert_eq!(json["toc"], serde_json::json!([]));
        assert_eq!(json["readingOrder"], serde_json::json!([]));
        assert_eq!(json["resources"], serde_json::json!([]));
        assert_eq!(json["landmarks"], serde_json::json!([]));
        assert_eq!(json["pageList"], serde_json::json!([]));

        let links = json["links"].as_array().unwrap();
        assert_eq!(links[0]["rel"], "self");
        // zip is DIVINA: self link is the divina manifest
        assert_eq!(links[0]["type"], MEDIATYPE_DIVINA_JSON);
        assert!(links[0]["href"]
            .as_str()
            .unwrap()
            .ends_with("/api/v1/books/b1/manifest"));
        // zip is DIVINA: no divina alternate manifest for profile DIVINA (only PDF/EPUB get one)
        let acquisition = links
            .iter()
            .find(|l| l["rel"] == "http://opds-spec.org/acquisition")
            .unwrap();
        assert_eq!(acquisition["type"], "application/vnd.comicbook+zip");
    }

    #[test]
    fn links_self_type_by_profile() {
        let mut book = sample_book();
        book.media.media_type = detect::APPLICATION_PDF.to_string();
        let publication =
            to_base_publication_dto(&book, BASE, SEGMENTS, None, vec![], &BTreeMap::new());
        let links = serde_json::to_value(&publication.links).unwrap();
        let arr = links.as_array().unwrap();
        // PDF: self is webpub, plus a divina manifest link
        assert_eq!(arr[0]["type"], MEDIATYPE_WEBPUB_JSON);
        assert!(arr
            .iter()
            .any(|l| l["href"].as_str().unwrap().ends_with("/manifest/divina")));

        let mut book = sample_book();
        book.media.media_type = "application/x-rar-compressed; version=4".to_string();
        let publication =
            to_base_publication_dto(&book, BASE, SEGMENTS, None, vec![], &BTreeMap::new());
        let links = serde_json::to_value(&publication.links).unwrap();
        let arr = links.as_array().unwrap();
        assert_eq!(arr[0]["type"], MEDIATYPE_DIVINA_JSON);
        let acquisition = arr
            .iter()
            .find(|l| l["rel"] == "http://opds-spec.org/acquisition")
            .unwrap();
        assert_eq!(acquisition["type"], "application/vnd.comicbook-rar");
    }

    #[test]
    fn manifest_divina_reading_order_and_alternate() {
        let book = sample_book();
        let publication = to_manifest_divina(
            &book,
            &media_zip(),
            &sample_series_metadata(),
            BASE,
            SEGMENTS,
            detect::IMAGE_JPEG,
        );
        let json = serde_json::to_value(&publication).unwrap();
        assert_eq!(json["metadata"]["conformsTo"], PROFILE_DIVINA);
        assert_eq!(json["metadata"]["language"], "ja");
        assert_eq!(json["metadata"]["readingProgression"], "rtl");
        let order = json["readingOrder"].as_array().unwrap();
        assert_eq!(order.len(), 2);
        assert!(order[0]["href"]
            .as_str()
            .unwrap()
            .contains("/pages/1?contentNegotiation=false"));
        // png is a recommended type: no alternate
        assert!(order[0].get("alternate").is_none());
        // webp gets a jpeg alternate
        let alternate = order[1]["alternate"].as_array().unwrap();
        assert_eq!(alternate.len(), 1);
        assert!(alternate[0]["href"]
            .as_str()
            .unwrap()
            .contains("convert=jpeg"));
        let resources = json["resources"].as_array().unwrap();
        assert!(resources[0]["href"]
            .as_str()
            .unwrap()
            .ends_with("/books/b1/thumbnail"));
    }

    #[test]
    fn manifest_pdf_raw_links() {
        let mut book = sample_book();
        book.media.media_type = detect::APPLICATION_PDF.to_string();
        let publication = to_manifest_pdf(
            &book,
            &media_zip(),
            &sample_series_metadata(),
            BASE,
            SEGMENTS,
            detect::IMAGE_JPEG,
        );
        let json = serde_json::to_value(&publication).unwrap();
        assert_eq!(json["metadata"]["conformsTo"], PROFILE_PDF);
        let order = json["readingOrder"].as_array().unwrap();
        assert_eq!(order.len(), 2);
        assert!(order[0]["href"].as_str().unwrap().ends_with("/pages/1/raw"));
        assert_eq!(order[0]["type"], "application/pdf");
    }

    #[test]
    fn manifest_epub_files_toc_and_rendition() {
        let mut book = sample_book();
        book.media.media_type = detect::APPLICATION_EPUB.to_string();
        let mut media = media_zip();
        media.media_type = Some(detect::APPLICATION_EPUB.to_string());
        media.files = vec![
            MediaFile {
                file_name: "text/ch1.xhtml".into(),
                media_type: Some("application/xhtml+xml".to_string()),
                sub_type: Some(MediaFileSubType::EpubPage),
                file_size: None,
            },
            MediaFile {
                file_name: "images/cover.jpg".into(),
                media_type: Some(detect::IMAGE_JPEG.to_string()),
                sub_type: Some(MediaFileSubType::EpubAsset),
                file_size: None,
            },
        ];
        let extension = MediaExtensionEpubView {
            toc: vec![EpubTocEntryView {
                title: Some("Chapter 1".into()),
                href: Some("text/ch1.xhtml#start".into()),
                children: vec![EpubTocEntryView {
                    title: Some("Section 1.1".into()),
                    href: Some("text/ch1.xhtml#s11".into()),
                    children: vec![],
                }],
            }],
            landmarks: vec![],
            page_list: vec![],
            is_fixed_layout: Some(true),
        };
        let publication = to_manifest_epub(
            &book,
            &media,
            Some(&extension),
            &sample_series_metadata(),
            BASE,
            SEGMENTS,
            detect::IMAGE_JPEG,
        );
        let json = serde_json::to_value(&publication).unwrap();
        assert_eq!(json["metadata"]["conformsTo"], PROFILE_EPUB);
        assert_eq!(json["metadata"]["rendition"]["layout"], "fixed");
        let order = json["readingOrder"].as_array().unwrap();
        assert_eq!(order.len(), 1);
        assert!(order[0]["href"]
            .as_str()
            .unwrap()
            .contains("/books/b1/resource/text/ch1.xhtml"));
        let resources = json["resources"].as_array().unwrap();
        assert_eq!(resources.len(), 2);
        assert!(resources[1]["href"]
            .as_str()
            .unwrap()
            .contains("/resource/images/cover.jpg"));
        let toc = json["toc"].as_array().unwrap();
        assert_eq!(toc[0]["title"], "Chapter 1");
        assert_eq!(
            toc[0]["href"].as_str().unwrap(),
            "http://localhost:25600/api/v1/books/b1/resource/text/ch1.xhtml#start"
        );
        assert_eq!(toc[0]["children"][0]["title"], "Section 1.1");
    }

    #[test]
    fn opds_publication_images_series_and_progression() {
        let publication = to_opds_publication_dto(&sample_book(), BASE, detect::IMAGE_JPEG);
        let json = serde_json::to_value(&publication).unwrap();
        let images = json["images"].as_array().unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(
            images[0]["properties"]["authenticate"]["type"],
            MEDIATYPE_OPDS_AUTHENTICATION_JSON
        );
        let links = json["links"].as_array().unwrap();
        let progression = links
            .iter()
            .find(|l| l["rel"] == REL_PROGRESSION_API)
            .unwrap();
        assert_eq!(progression["type"], MEDIATYPE_PROGRESSION_JSON);
        assert!(progression["href"]
            .as_str()
            .unwrap()
            .contains("/opds/v2/books/b1/progression"));
        // the series link lives in belongsTo.series[].links, not in the top-level links
        assert!(!links
            .iter()
            .any(|l| l["href"].as_str().unwrap().contains("/opds/v2/series/s1")));
        let series = &json["metadata"]["belongsTo"]["series"][0];
        assert_eq!(series["name"], "Berserk");
        assert_eq!(
            series["links"][0]["href"].as_str().unwrap(),
            &format!("{BASE}/opds/v2/series/s1")
        );
    }
}
