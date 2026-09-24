//! Tracks the latest kmweb release on GitHub: downloads the bundle, verifies it
//! against the sha256 published alongside, and atomically points the served web UI
//! directory at it. The managed copy lives under `<config-dir>/webui` so it survives
//! container rebuilds; the bundled/configured directory stays the offline baseline.
//!
//! Layout of the managed copy:
//!   <config-dir>/webui/current            active version, e.g. "0.3.0"
//!   <config-dir>/webui/versions/<v>/      extracted bundle (+ a kmweb.version marker)
//!   <config-dir>/webui/.tmp/              download/extract staging, cleaned after use

use crate::state::AppState;
use anyhow::{bail, Context};
use std::path::{Path, PathBuf};

const GITHUB_RELEASES: &str = "https://api.github.com/repos/kmworks/kmweb/releases";
const MARKER: &str = "kmweb.version";

fn managed_root(config_dir: &Path) -> PathBuf {
    config_dir.join("webui")
}

fn managed_dir(root: &Path) -> Option<PathBuf> {
    let version = std::fs::read_to_string(root.join("current")).ok()?;
    let dir = root.join("versions").join(version.trim());
    dir.join("index.html").is_file().then_some(dir)
}

/// The web UI directory to serve at startup: the updater's managed copy when one is
/// already installed, otherwise the configured bundle. A disabled UI stays disabled.
pub fn initial_dir(config: &crate::config::ServerConfig) -> Option<PathBuf> {
    let configured = config.webui_dir.as_ref()?;
    Some(managed_dir(&managed_root(&config.config_dir)).unwrap_or_else(|| configured.clone()))
}

/// Version of a served bundle, from its `kmweb.version` marker (stamped into the
/// docker image's bundled copy, and written by the updater into managed copies).
fn served_version(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join(MARKER))
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

pub enum Outcome {
    UpToDate,
    Updated(String),
}

pub struct WebuiUpdater {
    base_url: String,
    http: reqwest::Client,
}

impl WebuiUpdater {
    fn new(base_url: &str) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!("kmrs/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("reqwest client");
        Self {
            base_url: base_url.to_string(),
            http,
        }
    }

    /// Periodic checks: first one on startup, then every `webui.update-interval`.
    pub fn start(state: AppState) -> tokio::task::JoinHandle<()> {
        let updater = Self::new(GITHUB_RELEASES);
        let period = state.config.webui_update_interval;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(period);
            loop {
                // interval's first tick completes immediately
                interval.tick().await;
                match updater.check_once(&state).await {
                    Ok(Outcome::Updated(v)) => tracing::info!("web UI updated to kmweb v{v}"),
                    Ok(Outcome::UpToDate) => tracing::debug!("web UI is up to date"),
                    Err(e) => tracing::warn!("web UI update check failed: {e:#}"),
                }
            }
        })
    }

    async fn check_once(&self, state: &AppState) -> anyhow::Result<Outcome> {
        let Some(served) = state.webui_dir.get() else {
            return Ok(Outcome::UpToDate);
        };
        let release = self.latest_release().await?;
        let version = release.tag_name.trim_start_matches('v').to_string();
        if version.is_empty() {
            bail!("unexpected kmweb release tag: {:?}", release.tag_name);
        }
        if served_version(&served).as_deref() == Some(version.as_str()) {
            return Ok(Outcome::UpToDate);
        }

        let tarball_name = format!("kmweb-v{version}.tar.gz");
        let tarball_url = asset_url(&release, &tarball_name)?;
        let sha_url = asset_url(&release, &format!("{tarball_name}.sha256"))?;

        let root = managed_root(&state.config.config_dir);
        let tmp = root.join(".tmp");
        std::fs::create_dir_all(&tmp)?;
        let tarball = tmp.join(&tarball_name);
        let bytes = self.get_bytes(&tarball_url).await?;
        tokio::fs::write(&tarball, &bytes).await?;
        let expected = parse_sha256(&String::from_utf8(self.get_bytes(&sha_url).await?)?)?;

        let install = Install {
            root: root.clone(),
            tmp: tmp.clone(),
            tarball,
            expected,
            version: version.clone(),
        };
        let managed = tokio::task::spawn_blocking(move || install.run())
            .await
            .context("install task")??;
        state.webui_dir.set(Some(managed));
        Ok(Outcome::Updated(version))
    }

    async fn latest_release(&self) -> anyhow::Result<Release> {
        Ok(self
            .http
            .get(format!("{}/latest", self.base_url))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    async fn get_bytes(&self, url: &str) -> anyhow::Result<Vec<u8>> {
        Ok(self
            .http
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?
            .to_vec())
    }
}

/// Download/install split so the blocking filesystem half can run in spawn_blocking.
struct Install {
    root: PathBuf,
    tmp: PathBuf,
    tarball: PathBuf,
    expected: String,
    version: String,
}

impl Install {
    fn run(self) -> anyhow::Result<PathBuf> {
        if sha256_hex(&self.tarball)? != self.expected {
            bail!("sha256 mismatch for {}", self.tarball.display());
        }
        let staging = self.tmp.join(format!("extract-{}", self.version));
        let _ = std::fs::remove_dir_all(&staging);
        extract_bundle(&self.tarball, &staging)?;
        if !staging.join("index.html").is_file() {
            bail!("{} has no index.html at its root", self.tarball.display());
        }
        std::fs::write(staging.join(MARKER), &self.version)?;

        let versions = self.root.join("versions");
        std::fs::create_dir_all(&versions)?;
        let target = versions.join(&self.version);
        let _ = std::fs::remove_dir_all(&target);
        std::fs::rename(&staging, &target)?;

        // write-then-rename keeps `current` consistent with a fully installed version
        let current_tmp = self.tmp.join("current");
        std::fs::write(&current_tmp, &self.version)?;
        std::fs::rename(&current_tmp, self.root.join("current"))?;

        let _ = std::fs::remove_dir_all(&self.tmp);
        for entry in std::fs::read_dir(&versions)?.flatten() {
            if entry.file_name() != self.version.as_str() {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
        Ok(target)
    }
}

fn asset_url(release: &Release, name: &str) -> anyhow::Result<String> {
    release
        .assets
        .iter()
        .find(|a| a.name == name)
        .map(|a| a.browser_download_url.clone())
        .with_context(|| format!("kmweb release {} has no asset {name}", release.tag_name))
}

/// First whitespace-separated token of a `sha256sum`-format file.
fn parse_sha256(text: &str) -> anyhow::Result<String> {
    let token = text.split_whitespace().next().unwrap_or_default();
    if token.len() == 64 && token.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(token.to_ascii_lowercase())
    } else {
        bail!("no sha256 found in {text:?}");
    }
}

fn sha256_hex(path: &Path) -> anyhow::Result<String> {
    use sha2::Digest;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = sha2::Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// `unpack_in` refuses entries escaping the destination; turn that into an error
/// instead of silently skipping them.
fn extract_bundle(tarball: &Path, dst: &Path) -> anyhow::Result<()> {
    let file = std::fs::File::open(tarball)?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    std::fs::create_dir_all(dst)?;
    for entry in archive.entries()? {
        let mut entry = entry?;
        if !entry.unpack_in(dst)? {
            bail!(
                "archive entry escapes the target directory: {:?}",
                entry.path()
            );
        }
    }
    Ok(())
}

#[derive(serde::Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(serde::Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth;
    use crate::config::ServerConfig;
    use crate::settings::SettingsProvider;
    use crate::state::test_kmrs_db;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use komga_db::pool::{Database, JournalMode};
    use komga_db::{Migrator, Placeholders};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tower::ServiceExt;

    const VERSION: &str = "0.9.9";

    fn test_state(config_dir: &Path, webui_dir: Option<PathBuf>) -> AppState {
        let db = Database::open_in_memory(true).unwrap();
        Migrator::new(&komga_db::main_migrations(), Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        let tasks_db = Database::open_in_memory(false).unwrap();
        // dedicated task pools reuse the same in-memory database: task execution and assertions stay in sync
        let task_db = db.clone();
        Migrator::new(&komga_db::tasks_migrations(), Placeholders::default())
            .migrate(&tasks_db.rw())
            .unwrap();
        let db_config = |register_udfs| komga_db::pool::DatabaseConfig {
            file: std::env::temp_dir(),
            register_udfs,
            journal_mode: JournalMode::Wal,
            ..Default::default()
        };
        let config = ServerConfig {
            config_dir: config_dir.to_path_buf(),
            lucene_dir: std::env::temp_dir(),
            fonts_dir: std::env::temp_dir(),
            port: 0,
            database: db_config(true),
            tasks_db: db_config(false),
            kmrs_db: db_config(false),
            session_timeout: std::time::Duration::from_secs(3600),
            cors_allowed_origins: vec![],
            page_hashing: 3,
            epub_divina_letter_count_threshold: 15,
            kobo_sync_item_limit: 100,
            kepubify_path: None,
            server_context_path: None,
            migration_placeholders: Default::default(),
            oauth2: Default::default(),
            webui_dir: webui_dir.clone(),
            webui_auto_update: true,
            webui_update_interval: std::time::Duration::from_secs(24 * 3600),
        };
        AppState {
            sessions: auth::SessionStore::new(config.session_timeout),
            settings: Arc::new(SettingsProvider::load(db.clone())),
            tsid: Arc::new(komga_core::tsid::TsidFactory::new_random_node()),
            events: crate::events::event_bus(),
            task_emitter: Arc::new(crate::service::TaskEmitter::new(
                db.clone(),
                tasks_db.clone(),
                std::sync::Arc::new(tokio::sync::Notify::new()),
            )),
            db,
            task_db,
            tasks_db,
            kmrs_db: test_kmrs_db(),
            webui_dir: crate::webui::WebuiDir::new(initial_dir(&config)),
            config: Arc::new(config),
            search_index: crate::state::test_search_index(),
            kepub: crate::service::kepub::KepubConverter::new(tempfile::tempdir().unwrap().keep()),
            kobo_proxy: crate::service::kobo_proxy::KoboProxy::new(),
            shutdown_tx: tokio::sync::watch::channel(false).0,
        }
    }

    /// A bundle tarball with the given index.html, gzipped the way kmweb publishes it.
    fn bundle_tarball(index_html: &str) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        {
            let mut builder = tar::Builder::new(&mut encoder);
            let mut header = tar::Header::new_gnu();
            header.set_size(index_html.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, "./index.html", index_html.as_bytes())
                .unwrap();
            builder.finish().unwrap();
        }
        encoder.finish().unwrap()
    }

    /// Serves /releases/latest plus the two assets; asset URLs point back at this server.
    async fn serve_github(tarball: Vec<u8>, sha: String) -> (String, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = {
            let hits = hits.clone();
            axum::Router::new()
                .route(
                    "/releases/latest",
                    axum::routing::get(move || async move {
                        axum::Json(serde_json::json!({
                            "tag_name": format!("v{VERSION}"),
                            "assets": [
                                {"name": format!("kmweb-v{VERSION}.tar.gz"),
                                 "browser_download_url": format!("http://{addr}/kmweb.tar.gz")},
                                {"name": format!("kmweb-v{VERSION}.tar.gz.sha256"),
                                 "browser_download_url": format!("http://{addr}/kmweb.sha256")},
                            ],
                        }))
                    }),
                )
                .route(
                    "/kmweb.tar.gz",
                    axum::routing::get(move || {
                        let hits = hits.clone();
                        async move {
                            hits.fetch_add(1, Ordering::SeqCst);
                            tarball
                        }
                    }),
                )
                .route(
                    "/kmweb.sha256",
                    axum::routing::get(move || async move { sha }),
                )
        };
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{addr}/releases"), hits)
    }

    fn sha_of(bytes: &[u8]) -> String {
        use sha2::Digest;
        format!("{:x}", sha2::Sha256::digest(bytes))
    }

    fn baseline(index_html: &str, marker: Option<&str>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), index_html).unwrap();
        if let Some(version) = marker {
            std::fs::write(dir.path().join(MARKER), version).unwrap();
        }
        dir
    }

    async fn get_root(state: &AppState) -> (StatusCode, String) {
        let response = crate::build_router(state.clone())
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn installs_new_version_and_serves_it_immediately() {
        let config_dir = tempfile::tempdir().unwrap();
        let baseline = baseline("<html>old</html>", None);
        let state = test_state(config_dir.path(), Some(baseline.path().to_path_buf()));
        let tarball = bundle_tarball("<html>new</html>");
        let (base, _) = serve_github(
            tarball.clone(),
            format!("{}  dist-release/kmweb.tar.gz", sha_of(&tarball)),
        )
        .await;
        let updater = WebuiUpdater::new(&base);

        let outcome = updater.check_once(&state).await.unwrap();
        assert!(matches!(outcome, Outcome::Updated(v) if v == VERSION));

        let managed = config_dir.path().join("webui/versions").join(VERSION);
        assert_eq!(state.webui_dir.get().unwrap(), managed);
        assert_eq!(
            std::fs::read_to_string(config_dir.path().join("webui/current")).unwrap(),
            VERSION
        );
        assert_eq!(
            std::fs::read_to_string(managed.join(MARKER)).unwrap(),
            VERSION
        );
        let (status, body) = get_root(&state).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "<html>new</html>");
        // .tmp cleaned, only the active version kept
        assert!(!config_dir.path().join("webui/.tmp").exists());

        // the managed marker makes the next check a no-op
        let outcome = updater.check_once(&state).await.unwrap();
        assert!(matches!(outcome, Outcome::UpToDate));
    }

    #[tokio::test]
    async fn up_to_date_marker_skips_the_download() {
        let config_dir = tempfile::tempdir().unwrap();
        let baseline = baseline("<html>old</html>", Some(VERSION));
        let state = test_state(config_dir.path(), Some(baseline.path().to_path_buf()));
        let tarball = bundle_tarball("<html>new</html>");
        let (base, hits) = serve_github(tarball.clone(), sha_of(&tarball)).await;
        let updater = WebuiUpdater::new(&base);

        let outcome = updater.check_once(&state).await.unwrap();
        assert!(matches!(outcome, Outcome::UpToDate));
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        assert!(!config_dir.path().join("webui/current").exists());
    }

    #[tokio::test]
    async fn sha256_mismatch_keeps_the_baseline() {
        let config_dir = tempfile::tempdir().unwrap();
        let baseline = baseline("<html>old</html>", None);
        let baseline_path = baseline.path().to_path_buf();
        let state = test_state(config_dir.path(), Some(baseline_path.clone()));
        let tarball = bundle_tarball("<html>new</html>");
        let (base, _) = serve_github(tarball, format!("{}  kmweb.tar.gz", "0".repeat(64))).await;
        let updater = WebuiUpdater::new(&base);

        assert!(updater.check_once(&state).await.is_err());
        assert_eq!(state.webui_dir.get().unwrap(), baseline_path);
        assert!(!config_dir.path().join("webui/current").exists());
    }

    #[test]
    fn extract_rejects_path_traversal() {
        let tarball = {
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            {
                let mut builder = tar::Builder::new(&mut encoder);
                // Builder's set_path refuses `..`, so write the name field directly —
                // this is the shape a hostile archive would arrive in
                let mut header = tar::Header::new_gnu();
                header.as_mut_bytes()[..11].copy_from_slice(b"../evil.txt");
                header.set_size(4);
                header.set_mode(0o644);
                header.set_cksum();
                builder.append(&header, b"evil".as_slice()).unwrap();
                builder.finish().unwrap();
            }
            encoder.finish().unwrap()
        };
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("evil.tar.gz");
        std::fs::write(&file, tarball).unwrap();
        let dst = tmp.path().join("dst");
        assert!(extract_bundle(&file, &dst).is_err());
        assert!(!tmp.path().join("evil.txt").exists());
    }

    #[test]
    fn initial_dir_resolution() {
        let config_dir = tempfile::tempdir().unwrap();
        let configured = PathBuf::from("/bundled/webui");
        let mut config = ServerConfig::from_env();
        config.config_dir = config_dir.path().to_path_buf();
        config.webui_dir = Some(configured.clone());

        // no managed copy yet: the configured bundle wins
        assert_eq!(initial_dir(&config), Some(configured.clone()));

        // a valid managed copy wins
        let managed = config_dir.path().join("webui/versions").join(VERSION);
        std::fs::create_dir_all(&managed).unwrap();
        std::fs::write(managed.join("index.html"), "<html/>").unwrap();
        std::fs::write(config_dir.path().join("webui/current"), VERSION).unwrap();
        assert_eq!(initial_dir(&config), Some(managed));

        // a disabled UI stays disabled even with a managed copy present
        config.webui_dir = None;
        assert_eq!(initial_dir(&config), None);

        // a broken managed copy (no index.html) falls back to the baseline
        config.webui_dir = Some(configured.clone());
        std::fs::remove_file(
            config_dir
                .path()
                .join("webui/versions")
                .join(VERSION)
                .join("index.html"),
        )
        .unwrap();
        assert_eq!(initial_dir(&config), Some(configured));
    }

    #[test]
    fn parses_sha256sum_format() {
        let hash = "a".repeat(64);
        assert_eq!(
            parse_sha256(&format!("{hash}  dist-release/kmweb.tar.gz\n")).unwrap(),
            hash
        );
        assert!(parse_sha256("not a hash").is_err());
        assert!(parse_sha256("").is_err());
    }
}
