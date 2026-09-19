//! `SeriesCollectionLifecycle.kt`: collection CRUD, membership, thumbnails, and the mosaic.

use crate::events::DomainEvent;
use crate::state::AppState;
use komga_core::model::collection::SeriesCollection;
use komga_core::model::series::Series;
use komga_core::model::thumbnail::{Dimension, ThumbnailSeriesCollection};
use komga_core::time_codec::now_utc;
use komga_db::dao::collection::CollectionDao;
use komga_db::dao::thumbnail::ThumbnailSeriesCollectionDao;

pub const DUPLICATE_NAME_MESSAGE: &str = "Collection name already exists";

#[derive(Debug)]
pub enum CollectionError {
    DuplicateName,
    Db(komga_db::Error),
}

impl From<komga_db::Error> for CollectionError {
    fn from(e: komga_db::Error) -> Self {
        Self::Db(e)
    }
}

impl From<CollectionError> for komga_db::Error {
    fn from(e: CollectionError) -> Self {
        match e {
            CollectionError::DuplicateName => {
                komga_db::Error::EnumValue(DUPLICATE_NAME_MESSAGE.to_string())
            }
            CollectionError::Db(e) => e,
        }
    }
}

pub type Result<T> = std::result::Result<T, CollectionError>;

pub fn add_collection(state: &AppState, collection: SeriesCollection) -> Result<SeriesCollection> {
    tracing::info!("Adding new collection: {collection:?}");
    let dao = CollectionDao::new(state.db.clone());
    if dao.exists_by_name(&collection.name)? {
        return Err(CollectionError::DuplicateName);
    }
    let id = dao.insert(&collection)?;
    let created = dao
        .find_by_id(&id)?
        .expect("collection not found after insert");
    let _ = state
        .events
        .send(DomainEvent::CollectionAdded(created.clone()));
    Ok(created)
}

pub fn update_collection(state: &AppState, to_update: &SeriesCollection) -> Result<()> {
    tracing::info!("Update collection: {to_update:?}");
    let dao = CollectionDao::new(state.db.clone());
    dao.find_by_id(&to_update.id)?
        .expect("cannot update collection that does not exist");
    let existing = dao.find_by_id(&to_update.id)?.expect("checked above");
    if !existing.name.eq_ignore_ascii_case(&to_update.name)
        && dao.exists_by_name(&to_update.name)?
    {
        return Err(CollectionError::DuplicateName);
    }
    dao.update(to_update)?;
    let _ = state
        .events
        .send(DomainEvent::CollectionUpdated(to_update.clone()));
    Ok(())
}

pub fn delete_collection(state: &AppState, collection: &SeriesCollection) -> komga_db::Result<()> {
    ThumbnailSeriesCollectionDao::new(state.db.clone()).delete_by_collection_id(&collection.id)?;
    CollectionDao::new(state.db.clone()).delete(&collection.id)?;
    let _ = state
        .events
        .send(DomainEvent::CollectionDeleted(collection.clone()));
    Ok(())
}

/// Adds the series to the named collection, creating it when missing.
pub fn add_series_to_collection(
    state: &AppState,
    name: &str,
    series: &Series,
) -> komga_db::Result<()> {
    let dao = CollectionDao::new(state.db.clone());
    match dao.find_by_name(name)? {
        Some(existing) => {
            if existing.series_ids.iter().any(|id| id == &series.id) {
                tracing::debug!("Series is already in existing collection '{name}'");
                return Ok(());
            }
            tracing::debug!(
                "Adding series '{}' to existing collection '{name}'",
                series.name
            );
            let updated = SeriesCollection {
                series_ids: existing
                    .series_ids
                    .iter()
                    .cloned()
                    .chain(std::iter::once(series.id.clone()))
                    .collect(),
                ..existing
            };
            update_collection(state, &updated).map_err(komga_db::Error::from)
        }
        None => add_collection(
            state,
            SeriesCollection {
                id: String::new(),
                name: name.to_string(),
                ordered: false,
                series_ids: vec![series.id.clone()],
                filtered: false,
                created_date: now_utc(),
                last_modified_date: now_utc(),
            },
        )
        .map_err(komga_db::Error::from)
        .map(|_| ()),
    }
}

pub fn delete_empty_collections(state: &AppState) -> komga_db::Result<()> {
    tracing::info!("Deleting empty collections");
    let dao = CollectionDao::new(state.db.clone());
    let to_delete = dao.find_all_empty()?;
    if to_delete.is_empty() {
        return Ok(());
    }
    let ids: Vec<String> = to_delete.iter().map(|c| c.id.clone()).collect();
    ThumbnailSeriesCollectionDao::new(state.db.clone()).delete_by_collection_ids(&ids)?;
    for id in &ids {
        CollectionDao::new(state.db.clone()).delete(id)?;
    }
    for collection in to_delete {
        let _ = state
            .events
            .send(DomainEvent::CollectionDeleted(collection));
    }
    Ok(())
}

pub fn add_thumbnail(
    state: &AppState,
    thumbnail: ThumbnailSeriesCollection,
) -> komga_db::Result<ThumbnailSeriesCollection> {
    let dao = ThumbnailSeriesCollectionDao::new(state.db.clone());
    let id = dao.insert(&thumbnail)?;
    let mut thumbnail = thumbnail;
    thumbnail.id = id;
    if thumbnail.selected {
        dao.mark_selected(&thumbnail)?;
    }
    let _ = state
        .events
        .send(DomainEvent::ThumbnailSeriesCollectionAdded(
            thumbnail.clone(),
        ));
    Ok(thumbnail)
}

pub fn mark_selected_thumbnail(
    state: &AppState,
    thumbnail: &ThumbnailSeriesCollection,
) -> komga_db::Result<()> {
    ThumbnailSeriesCollectionDao::new(state.db.clone()).mark_selected(thumbnail)?;
    let _ = state
        .events
        .send(DomainEvent::ThumbnailSeriesCollectionAdded(
            ThumbnailSeriesCollection {
                selected: true,
                ..thumbnail.clone()
            },
        ));
    Ok(())
}

pub fn delete_thumbnail(
    state: &AppState,
    thumbnail: &ThumbnailSeriesCollection,
) -> komga_db::Result<()> {
    ThumbnailSeriesCollectionDao::new(state.db.clone()).delete(&thumbnail.id)?;
    thumbnails_house_keeping(state, &thumbnail.collection_id)?;
    let _ = state
        .events
        .send(DomainEvent::ThumbnailSeriesCollectionDeleted(
            thumbnail.clone(),
        ));
    Ok(())
}

/// `SeriesCollectionLifecycle.getThumbnailBytes`: the selected thumbnail, or a 2x2 mosaic of the
/// first 4 member series' covers (the id list is cycled to fill the grid, as in komga)
pub fn get_thumbnail_bytes(
    state: &AppState,
    collection: &SeriesCollection,
    user_id: &str,
) -> komga_db::Result<Vec<u8>> {
    if let Some(selected) = ThumbnailSeriesCollectionDao::new(state.db.clone())
        .find_selected_by_collection_id(&collection.id)?
    {
        return Ok(selected.thumbnail);
    }
    let mut ids = Vec::new();
    while ids.len() < 4 && !collection.series_ids.is_empty() {
        ids.extend(collection.series_ids.iter().take(4).cloned());
    }
    ids.truncate(4);
    let mut images = Vec::new();
    for id in &ids {
        if let Some(bytes) = crate::service::series::get_thumbnail_bytes(state, id, user_id)? {
            images.push(bytes);
        }
    }
    create_mosaic(&images, state.settings.get().thumbnail_size.max_edge())
}

fn thumbnails_house_keeping(state: &AppState, collection_id: &str) -> komga_db::Result<()> {
    tracing::info!("House keeping thumbnails for collection: {collection_id}");
    let dao = ThumbnailSeriesCollectionDao::new(state.db.clone());
    let all = dao.find_all_by_collection_id(collection_id)?;
    let selected: Vec<&ThumbnailSeriesCollection> = all.iter().filter(|t| t.selected).collect();
    if selected.len() > 1 {
        tracing::info!("More than one thumbnail is selected, removing extra ones");
        dao.mark_selected(selected[0])?;
    } else if selected.is_empty() {
        if let Some(first) = all.first() {
            tracing::info!("Collection has no selected thumbnail, choosing one automatically");
            dao.mark_selected(first)?;
        }
    }
    Ok(())
}

/// `MosaicGenerator.createMosaic`: 2x2 grid with top-left anchored cells on a black background,
/// JPEG output; width = round(height * 0.7066666667)
pub(crate) fn create_mosaic(images: &[Vec<u8>], max_edge: u32) -> komga_db::Result<Vec<u8>> {
    let height = max_edge;
    let width = (height as f64 * 0.7066666667).round() as u32;
    let mut mosaic = image::RgbImage::new(width, height);
    let positions = [
        (0i64, 0i64),
        ((width / 2) as i64, 0),
        (0, (height / 2) as i64),
        ((width / 2) as i64, (height / 2) as i64),
    ];
    for (bytes, (x, y)) in images.iter().take(4).zip(positions) {
        let img = image::load_from_memory(bytes).map_err(|e| {
            komga_db::Error::EnumValue(format!("could not decode mosaic image: {e}"))
        })?;
        // `resize` keeps the aspect ratio and never upscales, like Thumbnailator's size()
        let thumb = img.resize(
            height / 2,
            height / 2,
            image::imageops::FilterType::Lanczos3,
        );
        image::imageops::overlay(&mut mosaic, &thumb.to_rgb8(), x, y);
    }
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(mosaic)
        .write_to(&mut out, image::ImageFormat::Jpeg)
        .map_err(|e| komga_db::Error::EnumValue(format!("could not encode mosaic: {e}")))?;
    Ok(out.into_inner())
}

/// `Dimension(0, 0)` used when an uploaded image's dimensions cannot be read
pub(crate) fn zero_dimension() -> Dimension {
    Dimension {
        width: 0,
        height: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::collections::tests::{seed_base, test_state};
    use komga_core::model::thumbnail::ThumbnailType;
    use komga_core::time_codec::now_utc;

    fn sample_collection(name: &str) -> SeriesCollection {
        SeriesCollection {
            id: String::new(),
            name: name.into(),
            ordered: false,
            series_ids: vec![],
            filtered: false,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    fn series(id: &str, library_id: &str) -> Series {
        Series {
            id: id.into(),
            name: id.into(),
            url: format!("file:/l/{id}/"),
            file_last_modified: now_utc(),
            library_id: library_id.into(),
            book_count: 0,
            deleted_date: None,
            oneshot: false,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    fn make_thumbnail(collection_id: &str, selected: bool) -> ThumbnailSeriesCollection {
        ThumbnailSeriesCollection {
            id: String::new(),
            collection_id: collection_id.into(),
            thumbnail: vec![1],
            selected,
            type_: ThumbnailType::UserUploaded,
            media_type: "image/jpeg".into(),
            file_size: 1,
            dimension: zero_dimension(),
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    #[test]
    fn add_collection_persists_and_emits() {
        let state = test_state();
        seed_base(&state.db);
        let mut rx = state.events.subscribe();
        let collection = add_collection(&state, sample_collection("New")).unwrap();
        assert_eq!(collection.name, "New");
        assert!(!collection.id.is_empty());
        assert!(matches!(rx.try_recv(), Ok(DomainEvent::CollectionAdded(_))));
        assert!(matches!(
            add_collection(&state, sample_collection("new")),
            Err(CollectionError::DuplicateName)
        ));
    }

    #[test]
    fn update_collection_rename_and_duplicate() {
        let state = test_state();
        seed_base(&state.db);
        let mut rx = state.events.subscribe();
        let collection = add_collection(&state, sample_collection("Original")).unwrap();
        let mut updated = collection.clone();
        updated.name = "Renamed".into();
        update_collection(&state, &updated).unwrap();
        assert!(matches!(rx.try_recv(), Ok(DomainEvent::CollectionAdded(_))));
        assert!(matches!(
            rx.try_recv(),
            Ok(DomainEvent::CollectionUpdated(_))
        ));
        assert_eq!(
            CollectionDao::new(state.db.clone())
                .find_by_id(&collection.id)
                .unwrap()
                .unwrap()
                .name,
            "Renamed"
        );

        add_collection(&state, sample_collection("other")).unwrap();
        updated.name = "OTHER".into();
        assert!(matches!(
            update_collection(&state, &updated),
            Err(CollectionError::DuplicateName)
        ));
        // same name with different case is allowed (equals(..., true)豁免)
        updated.name = "renamed".into();
        assert!(update_collection(&state, &updated).is_ok());
    }

    #[test]
    fn add_series_to_collection_create_append_skip() {
        let state = test_state();
        seed_base(&state.db);
        let mut rx = state.events.subscribe();
        let s1 = series("s1", "l1");
        add_series_to_collection(&state, "Auto", &s1).unwrap();
        let created = CollectionDao::new(state.db.clone())
            .find_by_name("Auto")
            .unwrap()
            .unwrap();
        assert_eq!(created.series_ids, vec!["s1"]);
        assert!(!created.ordered);
        assert!(matches!(rx.try_recv(), Ok(DomainEvent::CollectionAdded(_))));

        let s2 = series("s2", "l1");
        add_series_to_collection(&state, "Auto", &s2).unwrap();
        let found = CollectionDao::new(state.db.clone())
            .find_by_name("Auto")
            .unwrap()
            .unwrap();
        assert_eq!(found.series_ids, vec!["s1", "s2"]);
        assert!(matches!(
            rx.try_recv(),
            Ok(DomainEvent::CollectionUpdated(_))
        ));

        add_series_to_collection(&state, "Auto", &s1).unwrap();
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn delete_collection_cascades_thumbnails() {
        let state = test_state();
        seed_base(&state.db);
        let mut rx = state.events.subscribe();
        let collection = add_collection(&state, sample_collection("Doomed")).unwrap();
        add_thumbnail(&state, make_thumbnail(&collection.id, true)).unwrap();
        delete_collection(&state, &collection).unwrap();
        assert!(matches!(rx.try_recv(), Ok(DomainEvent::CollectionAdded(_))));
        assert!(matches!(
            rx.try_recv(),
            Ok(DomainEvent::ThumbnailSeriesCollectionAdded(_))
        ));
        assert!(matches!(
            rx.try_recv(),
            Ok(DomainEvent::CollectionDeleted(_))
        ));
        assert!(CollectionDao::new(state.db.clone())
            .find_by_id(&collection.id)
            .unwrap()
            .is_none());
        assert_eq!(
            ThumbnailSeriesCollectionDao::new(state.db.clone())
                .find_all_by_collection_id(&collection.id)
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn delete_empty_collections_only_empty() {
        let state = test_state();
        seed_base(&state.db);
        let mut rx = state.events.subscribe();
        let empty = add_collection(&state, sample_collection("Empty")).unwrap();
        let kept = add_collection(&state, {
            let mut c = sample_collection("Kept");
            c.series_ids = vec!["s1".into()];
            c
        })
        .unwrap();
        delete_empty_collections(&state).unwrap();
        assert!(matches!(rx.try_recv(), Ok(DomainEvent::CollectionAdded(_))));
        assert!(matches!(rx.try_recv(), Ok(DomainEvent::CollectionAdded(_))));
        assert!(matches!(
            rx.try_recv(),
            Ok(DomainEvent::CollectionDeleted(_))
        ));
        assert!(CollectionDao::new(state.db.clone())
            .find_by_id(&empty.id)
            .unwrap()
            .is_none());
        assert!(CollectionDao::new(state.db.clone())
            .find_by_id(&kept.id)
            .unwrap()
            .is_some());
    }

    #[test]
    fn thumbnail_add_mark_delete_and_housekeeping() {
        let state = test_state();
        seed_base(&state.db);
        let collection = add_collection(&state, sample_collection("Thumbs")).unwrap();
        let t1 = add_thumbnail(&state, make_thumbnail(&collection.id, true)).unwrap();
        let t2 = add_thumbnail(&state, make_thumbnail(&collection.id, false)).unwrap();
        mark_selected_thumbnail(&state, &t2).unwrap();
        let selected = ThumbnailSeriesCollectionDao::new(state.db.clone())
            .find_selected_by_collection_id(&collection.id)
            .unwrap()
            .unwrap();
        assert_eq!(selected.id, t2.id);

        delete_thumbnail(&state, &t2).unwrap();
        let selected = ThumbnailSeriesCollectionDao::new(state.db.clone())
            .find_selected_by_collection_id(&collection.id)
            .unwrap()
            .unwrap();
        assert_eq!(selected.id, t1.id);
    }

    #[test]
    fn get_thumbnail_bytes_selected_first() {
        let state = test_state();
        seed_base(&state.db);
        let collection = add_collection(&state, sample_collection("M")).unwrap();
        let bytes = add_thumbnail(&state, make_thumbnail(&collection.id, true)).unwrap();
        let out = get_thumbnail_bytes(&state, &collection, "u1").unwrap();
        assert_eq!(out, bytes.thumbnail);
    }

    #[test]
    fn get_thumbnail_bytes_mosaic_when_empty_collection() {
        let state = test_state();
        seed_base(&state.db);
        let collection = add_collection(&state, sample_collection("Empty")).unwrap();
        // empty collection: no member images, the mosaic is a plain black JPEG (no infinite loop)
        let out = get_thumbnail_bytes(&state, &collection, "u1").unwrap();
        let img = image::load_from_memory(&out).unwrap();
        let max_edge = state.settings.get().thumbnail_size.max_edge();
        assert_eq!(img.height(), max_edge);
        assert_eq!(img.width(), (max_edge as f64 * 0.7066666667).round() as u32);
    }
}
