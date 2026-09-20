# kmrs

A Rust rewrite of the [Komga](https://komga.org) server. The goal is **full compatibility with the Java version's data formats and API behavior**:

- Can directly open/upgrade existing komga data directories (`database.sqlite`, `tasks.sqlite`), and the Java version of komga can still open libraries written by kmrs
- Endpoints, DTOs, pagination, error shapes, and authentication behavior for REST `/api/**`, OPDS v1.2/v2, SSE, Kobo, and KOReader match the Java version
- No UI

The compatibility target is **komga 1.27.0**: the Flyway migrations, the OpenAPI document, and the behavior fixtures are taken from that release, and API behavior is ported from it. Known deviations are listed under [Known limitations](#known-limitations).

## Structure

- `crates/komga-core`: domain model, TSID, time encoding/decoding, natural-sort comparator, error codes
- `crates/komga-db`: Flyway-compatible migrator (migration files are byte-for-byte copies of komga's Flyway migrations), connection pool, UDFs/collations, DAO
- `crates/komga-media`: media pipeline (sniffing/extraction/hashing/thumbnails/metadata)
- `crates/komga-search`: tantivy search index and Lucene query syntax
- `crates/komga-server`: axum HTTP layer (DTOs, authentication, SSE, OPDS, task queue)
- `xtask`: engineering helper commands

## Development

```sh
cargo nextest run --workspace     # run tests (self-contained: fixtures are vendored under crates/*/tests/resources)
cargo clippy --all-targets        # lint
cargo xtask sync-migrations     # reconcile with komga's Flyway migrations (requires a komga source checkout)
cargo xtask dump-schema         # print the final migrated schema
cargo xtask dump-checksums      # print Flyway CRC32 for all migrations
```

`sync-migrations` looks for a `komga` source checkout next to this repo by default; `KOMGA_REPO_DIR` can be used to point elsewhere. The checkout should be at the compatibility target (`v1.27.0`).

Run: `cargo run -p komga-server` produces the `kmrs` binary (default port 25600, data directory `~/.komga`, overridable with `KOMGA_CONFIG_DIR`).

## Configuration

`kmrs --help` lists the CLI flags (`--config-dir`, `--port`). The configuration file is always `<config-dir>/config.toml`; on first start it is generated from the built-in defaults, carrying over values from the Java komga's `application.yml`/`application.yaml` found in the same directory. See [examples/config.toml](examples/config.toml) for the full key list with defaults. Precedence: defaults < TOML file < env vars (Spring relaxed binding, e.g. `KOMGA_DATABASE_FILE`) < CLI flags.

## Docker

Every release publishes an image to `ghcr.io/everpcpc/kmrs` (tags: `latest`, `MAJOR.x`, `x.y.z`; platforms: `linux/amd64`, `linux/arm64`). It is a drop-in replacement for `gotson/komga` — same port, same `/config` and `/data` mounts, same `KOMGA_*` environment variables, so the [official Komga Docker instructions](https://komga.org/docs/installation/docker) apply verbatim, just with the image name swapped:

```sh
docker run -d \
  --name=komga \
  --user 1000:1000 \
  -p 25600:25600 \
  --mount type=bind,source=/path/to/config,target=/config \
  --mount type=bind,source=/path/to/data,target=/data \
  --restart unless-stopped \
  ghcr.io/everpcpc/kmrs
```

An existing komga `/config` directory (with `database.sqlite` / `tasks.sqlite`) is picked up and upgraded in place. Note that kmrs serves the API/OPDS only — there is no web UI.

## Compatibility testing

`tests/diff/diff.py` starts the Java komga and kmrs side by side over the same fixture library and compares ~105 endpoints (status, normalized JSON/XML bodies, headers, zip structure):

```sh
python3 tests/diff/diff.py --java-jar /path/to/komga.jar --rust-bin ./target/debug/kmrs
```

## Known limitations

Places where kmrs deviates from the Java version. Scope exclusions (no web UI) are intentional and not listed.

### Media formats

- No JXL / HEIF / JPEG2000 decoding: those types are sniffed but excluded from the readable image types, so page convert/resize fails like an unsupported reader. The Java version decodes them via ImageIO/TwelveMonkeys.
- PDF support requires a runtime libpdfium, looked up in `KOMGA_PDFIUM_PATH`, next to the executable, then system paths; when unavailable, every PDF operation returns `Unsupported`. The Java version bundles PDFBox. The Docker image ships libpdfium next to the binary.
- JPEG output is not byte-identical to ImageIO (different encoder) — an accepted deviation that affects byte-level comparisons of thumbnails and page hashes.

### Search

- Lucene fuzzy (`~`) and phrase-slop (`~N`) queries are unsupported and yield empty results.
- `komga.lucene.index-analyzer.*` and `komga.lucene.commit-delay` are ignored (warned and dropped during Java config migration); the analyzer is fixed to the multilingual ngram chain.
- Index codec upgrade is a no-op: tantivy has no such concept, and version-based reindexing already covers it.

### Database / migrations

- Flyway baseline is not supported: a database that has objects but no `flyway_schema_history` table hard-errors instead of being adopted.
- Deprecated BCP47 aliases (e.g. `iw` → `he`) are not normalized by the language-code migration for pre-2023-08 libraries — an accepted deviation.

### Actuator

- Only a subset of Spring's actuator is implemented (health, info, metrics, scheduledtasks, shutdown), and metrics are limited to about ten names.

### Ignored Java configuration keys

Warned about and dropped during `application.yml` migration:

- `server.error.*`
- `komga.database.batch-chunk-size`, `komga.tasks-db.batch-chunk-size`
- `komga.database.check-local-filesystem`, `komga.tasks-db.check-local-filesystem`

Ignored but behavior-equivalent (not limitations): `server.tomcat.*` (tomcat-specific), `server.forward-headers-strategy` (always `framework`), shutdown handling (always graceful).

## License

kmrs is under the [MIT License](LICENSE). The SQL migration files, the OpenAPI document, and the test fixtures are copied from the komga source tree; everything else is a rewritten implementation.
