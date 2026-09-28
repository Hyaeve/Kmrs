//! Optional file storage for book/series thumbnails (`thumbnails.storage = "file"`,
//! kmrs enhancement; Java always keeps blobs in the DB). Files live under
//! `<config-dir>/thumbnails/<kind>/<id[0..2]>/<id><ext>`; the row then carries only the
//! file URL, which the existing read path (blob first, URL fallback) already serves.

use crate::config::ThumbnailStorage;
use crate::state::AppState;
use komga_core::model::thumbnail::{ThumbnailBook, ThumbnailSeries};
use komga_db::dao::thumbnail::{ThumbnailBookDao, ThumbnailSeriesDao};
use komga_db::dao::thumbnail_migration::ThumbnailMigrationDao;
use komga_media::scanner::path_to_url;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const MIGRATION_BATCH_SIZE: u32 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailKind {
    Book,
    Series,
}

impl ThumbnailKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Book => "book",
            Self::Series => "series",
        }
    }
}

pub fn thumbnails_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("thumbnails")
}

/// Writes thumbnail bytes to `<dir>/<kind>/<id[0..2]>/<id><ext>`: a temp file in the
/// same directory renamed into place, so readers never see a partial file. Returns the
/// file's URL for the row.
pub fn write(
    dir: &Path,
    kind: ThumbnailKind,
    id: &str,
    media_type: &str,
    bytes: &[u8],
) -> std::io::Result<String> {
    let shard_dir = dir.join(kind.as_str()).join(id.get(..2).unwrap_or(id));
    std::fs::create_dir_all(&shard_dir)?;
    let file_name = format!("{id}{}", extension_for(media_type));
    let tmp = shard_dir.join(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, shard_dir.join(&file_name))?;
    Ok(path_to_url(&shard_dir.join(file_name)))
}

fn extension_for(media_type: &str) -> &'static str {
    match media_type {
        "image/jpeg" => ".jpg",
        "image/png" => ".png",
        "image/webp" => ".webp",
        _ => ".img",
    }
}

/// Deletes the file behind `url`, but only when it lives under `dir` — a sidecar URL
/// points into the library and must never be deleted here. A missing file is a no-op.
pub fn remove_url_if_managed(dir: &Path, url: &str) -> std::io::Result<()> {
    let path = normalize_lexically(Path::new(&komga_core::dto::url_to_file_path(url)));
    if !path.starts_with(normalize_lexically(dir)) {
        return Ok(());
    }
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// `url_to_file_path` decodes but does not normalize, and `Path::starts_with` is purely
/// lexical: without resolving `.`/`..` first, a traversal component would pass the
/// prefix check and still escape `dir` at file-open time.
fn normalize_lexically(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Best-effort removal of files behind rows that were just replaced or deleted.
/// Failures are logged, not fatal: the daily orphan sweep reclaims anything left behind.
pub fn remove_managed_files<'a>(state: &AppState, urls: impl IntoIterator<Item = &'a str>) {
    if state.config.thumbnail_storage != ThumbnailStorage::File {
        return;
    }
    let dir = thumbnails_dir(&state.config.config_dir);
    for url in urls {
        if let Err(e) = remove_url_if_managed(&dir, url) {
            tracing::warn!("could not remove thumbnail file {url}: {e}");
        }
    }
}

/// Field access shared by `ThumbnailBook`/`ThumbnailSeries` for the blob→file migration.
trait BlobThumbnail {
    fn id(&self) -> &str;
    fn media_type(&self) -> &str;
    fn take_blob(&mut self) -> Option<Vec<u8>>;
    fn set_url(&mut self, url: String);
}

impl BlobThumbnail for ThumbnailBook {
    fn id(&self) -> &str {
        &self.id
    }
    fn media_type(&self) -> &str {
        &self.media_type
    }
    fn take_blob(&mut self) -> Option<Vec<u8>> {
        self.thumbnail.take()
    }
    fn set_url(&mut self, url: String) {
        self.url = Some(url);
    }
}

impl BlobThumbnail for ThumbnailSeries {
    fn id(&self) -> &str {
        &self.id
    }
    fn media_type(&self) -> &str {
        &self.media_type
    }
    fn take_blob(&mut self) -> Option<Vec<u8>> {
        self.thumbnail.take()
    }
    fn set_url(&mut self, url: String) {
        self.url = Some(url);
    }
}

/// One-time migration of existing blob thumbnails to files, run at startup when
/// `thumbnails.storage = "file"`. Each kind is marker-gated in kmrs.sqlite so an
/// interrupted run resumes instead of restarting; a row whose file cannot be written
/// keeps its blob (still readable) and is logged.
pub fn migrate_blobs_to_files(state: &AppState) -> komga_db::Result<()> {
    let dir = thumbnails_dir(&state.config.config_dir);
    let markers = ThumbnailMigrationDao::new(state.kmrs_db.clone());
    let mut migrated_total = 0u64;
    for kind in [ThumbnailKind::Book, ThumbnailKind::Series] {
        if markers.is_done(kind.as_str())? {
            continue;
        }
        let migrated = match kind {
            ThumbnailKind::Book => migrate_kind(
                &dir,
                kind,
                &markers,
                |after, limit| {
                    ThumbnailBookDao::new(state.task_db.clone()).find_with_blob_batch(after, limit)
                },
                |thumbnail| ThumbnailBookDao::new(state.task_db.clone()).update(thumbnail),
            )?,
            ThumbnailKind::Series => migrate_kind(
                &dir,
                kind,
                &markers,
                |after, limit| {
                    ThumbnailSeriesDao::new(state.task_db.clone())
                        .find_with_blob_batch(after, limit)
                },
                |thumbnail| ThumbnailSeriesDao::new(state.task_db.clone()).update(thumbnail),
            )?,
        };
        migrated_total += migrated;
        tracing::info!("migrated {migrated} {} thumbnails to files", kind.as_str());
    }
    // The vacuum marker is set before VACUUM and cleared after success, so a crash
    // between mark_done and VACUUM is redone on the next start.
    if migrated_total > 0 || markers.is_done(VACUUM_MARKER)? {
        markers.mark_done(VACUUM_MARKER)?;
        match state.task_db.rw().execute_batch("VACUUM") {
            Ok(()) => markers.clear(VACUUM_MARKER)?,
            // failure only costs disk space; the marker stays so the next start retries
            Err(e) => {
                tracing::warn!(
                    "could not VACUUM the main database after the thumbnail migration: {e}"
                )
            }
        }
    }
    Ok(())
}

const VACUUM_MARKER: &str = "vacuum";

fn migrate_kind<T: BlobThumbnail>(
    dir: &Path,
    kind: ThumbnailKind,
    markers: &ThumbnailMigrationDao,
    mut fetch: impl FnMut(i64, u32) -> komga_db::Result<Vec<(i64, T)>>,
    mut update: impl FnMut(&T) -> komga_db::Result<u64>,
) -> komga_db::Result<u64> {
    let mut migrated = 0u64;
    let mut after_rowid = 0i64;
    loop {
        let batch = fetch(after_rowid, MIGRATION_BATCH_SIZE)?;
        if batch.is_empty() {
            break;
        }
        for (rowid, mut thumbnail) in batch {
            after_rowid = rowid;
            let Some(bytes) = thumbnail.take_blob() else {
                continue;
            };
            match write(dir, kind, thumbnail.id(), thumbnail.media_type(), &bytes) {
                Ok(url) => {
                    thumbnail.set_url(url.clone());
                    match update(&thumbnail) {
                        Ok(1) => migrated += 1,
                        Ok(_) => {
                            // the row was deleted concurrently: the just-written file
                            // has no owner, remove it instead of leaking it
                            tracing::warn!(
                                "{} thumbnail {} vanished during migration, removing its file",
                                kind.as_str(),
                                thumbnail.id()
                            );
                            if let Err(e) = remove_url_if_managed(dir, &url) {
                                tracing::warn!(
                                    "could not remove the file of vanished {} thumbnail {}: {e}",
                                    kind.as_str(),
                                    thumbnail.id()
                                );
                            }
                        }
                        // the row keeps its blob (still readable); the unreferenced file
                        // is reclaimed by the sweep
                        Err(e) => tracing::warn!(
                            "could not update {} thumbnail {} after writing its file: {e}",
                            kind.as_str(),
                            thumbnail.id()
                        ),
                    }
                }
                Err(e) => tracing::warn!(
                    "could not migrate {} thumbnail {} to a file, keeping the blob: {e}",
                    kind.as_str(),
                    thumbnail.id()
                ),
            }
        }
    }
    markers.mark_done(kind.as_str())?;
    Ok(migrated)
}

/// Deletes files under `<dir>/book` and `<dir>/series` that no THUMBNAIL_BOOK /
/// THUMBNAIL_SERIES row references (a row deleted without removing its file leaves an
/// orphan behind). Runs in both storage modes; missing directories are a no-op.
pub fn sweep_orphan_files(state: &AppState) -> komga_db::Result<usize> {
    sweep_orphan_files_with_grace(state, SWEEP_GRACE)
}

/// Files younger than the grace period are skipped: the write path renames the file
/// before the row insert lands, so a fresh unreferenced file may still get its row
/// (and a fresh `.tmp` may still be renamed).
const SWEEP_GRACE: std::time::Duration = std::time::Duration::from_secs(600);

fn sweep_orphan_files_with_grace(
    state: &AppState,
    grace: std::time::Duration,
) -> komga_db::Result<usize> {
    let dir = thumbnails_dir(&state.config.config_dir);
    if !dir.is_dir() {
        return Ok(0);
    }
    let mut referenced: HashSet<String> = HashSet::new();
    referenced.extend(ThumbnailBookDao::new(state.task_db.clone()).all_urls()?);
    referenced.extend(ThumbnailSeriesDao::new(state.task_db.clone()).all_urls()?);
    let mut removed = 0usize;
    for kind in [ThumbnailKind::Book, ThumbnailKind::Series] {
        let Ok(shards) = std::fs::read_dir(dir.join(kind.as_str())) else {
            continue;
        };
        for shard in shards.flatten() {
            let Ok(files) = std::fs::read_dir(shard.path()) else {
                continue;
            };
            for file in files.flatten() {
                if !file.file_type().is_ok_and(|t| t.is_file()) {
                    continue;
                }
                let path = file.path();
                if referenced.contains(&path_to_url(&path)) {
                    continue;
                }
                let fresh = file
                    .metadata()
                    .and_then(|m| m.modified())
                    .is_ok_and(|t| t.elapsed().unwrap_or_default() < grace);
                if fresh {
                    continue;
                }
                match std::fs::remove_file(&path) {
                    Ok(()) => {
                        removed += 1;
                        tracing::debug!("removed orphaned thumbnail file {}", path.display());
                    }
                    Err(e) => tracing::warn!(
                        "could not remove orphaned thumbnail file {}: {e}",
                        path.display()
                    ),
                }
            }
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ThumbnailStorage;
    use crate::service::series::create_series;
    use crate::service::series::tests as series_tests;
    use komga_core::model::thumbnail::{Dimension, ThumbnailType};
    use komga_core::time_codec::now_utc;
    use std::path::PathBuf;

    fn file_state() -> AppState {
        crate::state::test_state_with_thumbnail_storage(
            series_tests::test_state(),
            ThumbnailStorage::File,
        )
    }

    fn file_path(url: &str) -> PathBuf {
        PathBuf::from(komga_core::dto::url_to_file_path(url))
    }

    #[test]
    fn write_lands_at_deterministic_path_and_url_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let url = write(
            dir.path(),
            ThumbnailKind::Book,
            "0123456789abc",
            "image/jpeg",
            b"jpeg-bytes",
        )
        .unwrap();
        let expected = dir.path().join("book").join("01").join("0123456789abc.jpg");
        assert_eq!(std::fs::read(&expected).unwrap(), b"jpeg-bytes");
        assert_eq!(url, path_to_url(&expected));
        assert_eq!(file_path(&url), expected);

        for (media_type, ext) in [
            ("image/png", ".png"),
            ("image/webp", ".webp"),
            ("image/gif", ".img"),
        ] {
            let url = write(dir.path(), ThumbnailKind::Series, "abc", media_type, b"x").unwrap();
            assert!(url.ends_with(ext), "{url} should end with {ext}");
        }
    }

    #[test]
    fn remove_url_if_managed_only_deletes_inside_the_dir() {
        let dir = tempfile::tempdir().unwrap();
        let url = write(dir.path(), ThumbnailKind::Book, "abc", "image/png", b"x").unwrap();

        // a URL outside the thumbnails dir is refused, the file survives
        let outside = tempfile::tempdir().unwrap();
        let outside_file = outside.path().join("cover.jpg");
        std::fs::write(&outside_file, b"library sidecar").unwrap();
        remove_url_if_managed(dir.path(), &path_to_url(&outside_file)).unwrap();
        assert!(outside_file.exists());

        // a traversal URL whose decoded path lexically starts with dir but resolves
        // outside it is refused too
        let escapee = outside.path().join("escapee.jpg");
        std::fs::write(&escapee, b"x").unwrap();
        let attack = format!(
            "file:{}/x/%2E%2E/%2E%2E/{}/escapee.jpg",
            dir.path().display(),
            outside.path().file_name().unwrap().to_string_lossy()
        );
        remove_url_if_managed(dir.path(), &attack).unwrap();
        assert!(escapee.exists());

        let managed = file_path(&url);
        remove_url_if_managed(dir.path(), &url).unwrap();
        assert!(!managed.exists());
        // a missing file is not an error
        remove_url_if_managed(dir.path(), &url).unwrap();
    }

    #[test]
    fn migrate_blobs_to_files_flips_rows_and_is_idempotent() {
        let state = file_state();
        series_tests::seed_library(&state.db, "lib1");
        let series = create_series(&state, &series_tests::sample_series("lib1", "S")).unwrap();
        let book = series_tests::insert_book_with_media(&state, &series, "v01", 1);
        let book_dao = ThumbnailBookDao::new(state.db.clone());
        let series_dao = ThumbnailSeriesDao::new(state.db.clone());

        let book_blob_id = book_dao
            .insert(&ThumbnailBook {
                id: String::new(),
                book_id: book.id.clone(),
                thumbnail: Some(vec![1, 2, 3]),
                url: None,
                selected: true,
                type_: ThumbnailType::Generated,
                media_type: "image/jpeg".into(),
                file_size: 3,
                dimension: Dimension {
                    width: 1,
                    height: 1,
                },
                created_date: now_utc(),
                last_modified_date: now_utc(),
            })
            .unwrap();
        let book_sidecar_id = book_dao
            .insert(&ThumbnailBook {
                id: String::new(),
                book_id: book.id.clone(),
                thumbnail: None,
                url: Some("file:/library/cover.jpg".into()),
                selected: false,
                type_: ThumbnailType::Sidecar,
                media_type: "image/jpeg".into(),
                file_size: 3,
                dimension: Dimension {
                    width: 1,
                    height: 1,
                },
                created_date: now_utc(),
                last_modified_date: now_utc(),
            })
            .unwrap();
        let series_blob_id = series_dao
            .insert(&ThumbnailSeries {
                id: String::new(),
                series_id: series.id.clone(),
                thumbnail: Some(vec![9, 9]),
                url: None,
                selected: true,
                type_: ThumbnailType::UserUploaded,
                media_type: "image/png".into(),
                file_size: 2,
                dimension: Dimension {
                    width: 1,
                    height: 1,
                },
                created_date: now_utc(),
                last_modified_date: now_utc(),
            })
            .unwrap();

        migrate_blobs_to_files(&state).unwrap();

        let dir = thumbnails_dir(&state.config.config_dir);
        let book_row = book_dao.find_by_id(&book_blob_id).unwrap().unwrap();
        assert!(book_row.thumbnail.is_none());
        let book_file = file_path(book_row.url.as_ref().unwrap());
        assert_eq!(std::fs::read(&book_file).unwrap(), vec![1, 2, 3]);
        assert!(book_file.starts_with(dir.join("book")));
        assert!(book_file.ends_with(format!("{book_blob_id}.jpg")));

        let series_row = series_dao.find_by_id(&series_blob_id).unwrap().unwrap();
        assert!(series_row.thumbnail.is_none());
        let series_file = file_path(series_row.url.as_ref().unwrap());
        assert_eq!(std::fs::read(&series_file).unwrap(), vec![9, 9]);
        assert!(series_file.starts_with(dir.join("series")));
        assert!(series_file.ends_with(format!("{series_blob_id}.png")));

        // the sidecar row (URL-only) is not a blob row and stays untouched
        let sidecar_row = book_dao.find_by_id(&book_sidecar_id).unwrap().unwrap();
        assert_eq!(sidecar_row.url.as_deref(), Some("file:/library/cover.jpg"));
        assert!(sidecar_row.thumbnail.is_none());

        let markers = ThumbnailMigrationDao::new(state.kmrs_db.clone());
        assert!(markers.is_done("book").unwrap());
        assert!(markers.is_done("series").unwrap());

        // a second run is a no-op (markers done, nothing left to migrate)
        migrate_blobs_to_files(&state).unwrap();
        let book_row = book_dao.find_by_id(&book_blob_id).unwrap().unwrap();
        assert!(book_row.thumbnail.is_none());
        assert_eq!(std::fs::read(&book_file).unwrap(), vec![1, 2, 3]);
    }

    fn blob_row(book_id: &str, id: &str, bytes: Vec<u8>) -> ThumbnailBook {
        ThumbnailBook {
            id: id.into(),
            book_id: book_id.into(),
            thumbnail: Some(bytes),
            url: None,
            selected: false,
            type_: ThumbnailType::Generated,
            media_type: "image/jpeg".into(),
            file_size: 1,
            dimension: Dimension {
                width: 1,
                height: 1,
            },
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    /// Serves `rows` on the first fetch, then an empty batch.
    fn fetch_once<T: Clone>(
        rows: Vec<(i64, T)>,
    ) -> impl FnMut(i64, u32) -> komga_db::Result<Vec<(i64, T)>> {
        let mut served = false;
        move |_, _| {
            if served {
                Ok(vec![])
            } else {
                served = true;
                Ok(rows.clone())
            }
        }
    }

    #[test]
    fn migrate_kind_removes_the_file_when_the_row_vanishes() {
        let dir = tempfile::tempdir().unwrap();
        let markers = ThumbnailMigrationDao::new(crate::state::test_kmrs_db());

        let migrated = migrate_kind(
            dir.path(),
            ThumbnailKind::Book,
            &markers,
            fetch_once(vec![(1, blob_row("b1", "t1", vec![1]))]),
            |_| Ok(0),
        )
        .unwrap();
        assert_eq!(migrated, 0);
        // the just-written file had no row to belong to and was removed again
        let shard = dir.path().join("book").join("t1");
        assert_eq!(std::fs::read_dir(shard).unwrap().count(), 0);
        assert!(markers.is_done("book").unwrap());
    }

    #[test]
    fn migrate_kind_continues_when_the_update_fails() {
        let dir = tempfile::tempdir().unwrap();
        let markers = ThumbnailMigrationDao::new(crate::state::test_kmrs_db());

        let migrated = migrate_kind(
            dir.path(),
            ThumbnailKind::Book,
            &markers,
            fetch_once(vec![
                (1, blob_row("b1", "t1", vec![1])),
                (2, blob_row("b1", "t2", vec![2])),
            ]),
            |thumbnail| {
                if thumbnail.id() == "t1" {
                    Err(komga_db::Error::EnumValue("boom".into()))
                } else {
                    Ok(1)
                }
            },
        )
        .unwrap();
        assert_eq!(migrated, 1);
        // the failed row's file is left for the sweep; the good row's file stays
        assert!(dir.path().join("book").join("t1").join("t1.jpg").exists());
        assert!(dir.path().join("book").join("t2").join("t2.jpg").exists());
    }

    #[test]
    fn migrate_resumes_after_an_interrupted_run() {
        let state = file_state();
        series_tests::seed_library(&state.db, "lib1");
        let series = create_series(&state, &series_tests::sample_series("lib1", "S")).unwrap();
        let book = series_tests::insert_book_with_media(&state, &series, "v01", 1);
        let book_dao = ThumbnailBookDao::new(state.db.clone());
        let id_a = book_dao
            .insert(&blob_row(&book.id, "", vec![b'a']))
            .unwrap();
        let id_b = book_dao
            .insert(&blob_row(&book.id, "", vec![b'b']))
            .unwrap();

        // simulate a crash after the first row's flip but before mark_done
        let dir = thumbnails_dir(&state.config.config_dir);
        let mut row_a = book_dao.find_by_id(&id_a).unwrap().unwrap();
        let url_a = write(
            &dir,
            ThumbnailKind::Book,
            &row_a.id,
            &row_a.media_type,
            row_a.thumbnail.as_ref().unwrap(),
        )
        .unwrap();
        row_a.thumbnail = None;
        row_a.url = Some(url_a.clone());
        assert_eq!(book_dao.update(&row_a).unwrap(), 1);

        migrate_blobs_to_files(&state).unwrap();

        // row A was already flipped and is not rewritten; row B is migrated
        let row_a = book_dao.find_by_id(&id_a).unwrap().unwrap();
        assert_eq!(row_a.url.as_deref(), Some(url_a.as_str()));
        assert!(row_a.thumbnail.is_none());
        let row_b = book_dao.find_by_id(&id_b).unwrap().unwrap();
        assert!(row_b.thumbnail.is_none());
        let file_b = file_path(row_b.url.as_ref().unwrap());
        assert_eq!(std::fs::read(&file_b).unwrap(), vec![b'b']);
        let markers = ThumbnailMigrationDao::new(state.kmrs_db.clone());
        assert!(markers.is_done("book").unwrap());
        assert!(markers.is_done("series").unwrap());
    }

    #[test]
    fn migrate_redoes_vacuum_when_the_marker_survives_a_crash() {
        let state = file_state();
        let markers = ThumbnailMigrationDao::new(state.kmrs_db.clone());
        // simulate a crash between mark_done and VACUUM: no blob rows are left,
        // but the vacuum marker forces a redo
        markers.mark_done(VACUUM_MARKER).unwrap();
        migrate_blobs_to_files(&state).unwrap();
        assert!(!markers.is_done(VACUUM_MARKER).unwrap());
    }

    #[test]
    fn sweep_removes_orphans_and_keeps_referenced_files() {
        let state = file_state();
        series_tests::seed_library(&state.db, "lib1");
        let series = create_series(&state, &series_tests::sample_series("lib1", "S")).unwrap();
        let book = series_tests::insert_book_with_media(&state, &series, "v01", 1);
        let dir = thumbnails_dir(&state.config.config_dir);

        // a file a thumbnail row points at survives
        let referenced_url =
            write(&dir, ThumbnailKind::Book, "ref1", "image/png", b"keep").unwrap();
        ThumbnailBookDao::new(state.db.clone())
            .insert(&ThumbnailBook {
                id: String::new(),
                book_id: book.id.clone(),
                thumbnail: None,
                url: Some(referenced_url.clone()),
                selected: false,
                type_: ThumbnailType::Sidecar,
                media_type: "image/png".into(),
                file_size: 4,
                dimension: Dimension {
                    width: 1,
                    height: 1,
                },
                created_date: now_utc(),
                last_modified_date: now_utc(),
            })
            .unwrap();
        // no row points at this one
        let orphan_url = write(
            &dir,
            ThumbnailKind::Series,
            "orphan1",
            "image/jpeg",
            b"gone",
        )
        .unwrap();
        // a sidecar file outside the thumbnails dir is never touched
        let outside = state.config.config_dir.join("library-cover.jpg");
        std::fs::write(&outside, b"library").unwrap();

        // stale orphan: past the grace period, it is reclaimed
        let removed = sweep_orphan_files_with_grace(&state, std::time::Duration::ZERO).unwrap();
        assert_eq!(removed, 1);
        assert!(file_path(&referenced_url).exists());
        assert!(!file_path(&orphan_url).exists());
        assert!(outside.exists());
    }

    #[test]
    fn sweep_keeps_files_within_the_grace_period() {
        let state = file_state();
        let dir = thumbnails_dir(&state.config.config_dir);

        // a fresh unreferenced file may still get its row (the write path renames the
        // file before the insert lands), and a fresh .tmp may still be renamed
        let fresh_url = write(&dir, ThumbnailKind::Book, "fresh1", "image/png", b"x").unwrap();
        let shard = dir.join("book").join("fr");
        std::fs::create_dir_all(&shard).unwrap();
        let tmp = shard.join(".fresh1.png.abc.tmp");
        std::fs::write(&tmp, b"x").unwrap();

        assert_eq!(sweep_orphan_files(&state).unwrap(), 0);
        assert!(file_path(&fresh_url).exists());
        assert!(tmp.exists());

        // once stale, both are reclaimed
        let removed = sweep_orphan_files_with_grace(&state, std::time::Duration::ZERO).unwrap();
        assert_eq!(removed, 2);
        assert!(!file_path(&fresh_url).exists());
        assert!(!tmp.exists());
    }

    #[test]
    fn sweep_without_a_thumbnails_dir_is_a_no_op() {
        let state = file_state();
        assert_eq!(sweep_orphan_files(&state).unwrap(), 0);
    }
}
