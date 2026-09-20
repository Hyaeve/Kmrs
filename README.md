# kmrs

[![CI](https://github.com/everpcpc/kmrs/actions/workflows/ci.yml/badge.svg)](https://github.com/everpcpc/kmrs/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/everpcpc/kmrs)](https://github.com/everpcpc/kmrs/releases/latest)
[![Docker image](https://img.shields.io/badge/ghcr.io-everpcpc%2Fkmrs-blue)](https://github.com/everpcpc/kmrs/pkgs/container/kmrs)
[![License: MIT](https://img.shields.io/github/license/everpcpc/kmrs)](LICENSE)

A drop-in, API-compatible reimplementation of the [Komga](https://komga.org) comic/manga server in Rust — a single static binary, no JVM required.

> [!NOTE]
> kmrs serves the API and OPDS feeds only — there is **no web UI**. Pair it with a Komga-compatible client — [KMReader](https://github.com/everpcpc/KMReader) (iOS/macOS/tvOS), KOReader, Kobo, and [others](https://komga.org/docs/category/readers).

## Features

- **Drop-in replacement** for `gotson/komga`: same port (25600), same `/config` and `/data` mounts, same `KOMGA_*` environment variables
- **Data-level compatibility**: opens and upgrades existing komga data directories (`database.sqlite`, `tasks.sqlite`) in place — and the Java version can still open libraries written by kmrs
- **API parity**: REST `/api/**`, OPDS v1.2/v2, SSE, Kobo sync, and KOReader progress sync — endpoints, DTOs, pagination, error shapes, and authentication behavior match the Java version
- **Verified against the Java version**: byte-for-byte Flyway migrations, a differential test harness comparing ~105 endpoints against a live Java instance, and schema contract tests

## Quick start

### Docker

Every release publishes an image to `ghcr.io/everpcpc/kmrs` (tags: `latest`, `MAJOR.x`, `x.y.z`; platforms: `linux/amd64`, `linux/arm64`). The [official Komga Docker instructions](https://komga.org/docs/installation/docker) apply verbatim — just swap the image name:

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

An existing komga `/config` directory (with `database.sqlite` / `tasks.sqlite`) is picked up and upgraded in place.

### Prebuilt binaries

Download the archive for your platform from the [latest release](https://github.com/everpcpc/kmrs/releases/latest) (Linux, macOS, Windows; x86_64 and aarch64).

### Build from source

```sh
cargo build --release -p komga-server   # produces target/release/kmrs
```

The binary serves on port 25600 with data directory `~/.komga` (override with `KOMGA_CONFIG_DIR`).

## Configuration

`kmrs --help` lists the CLI flags (`--config-dir`, `--port`). The configuration file is always `<config-dir>/config.toml`; on first start it is generated from the built-in defaults, carrying over values from the Java komga's `application.yml`/`application.yaml` found in the same directory. See [examples/config.toml](examples/config.toml) for the full key list with defaults. Precedence: defaults < TOML file < env vars (Spring relaxed binding, e.g. `KOMGA_DATABASE_FILE`) < CLI flags.

## Compatibility

The compatibility target is **komga 1.27.0**: the Flyway migrations, the OpenAPI document, and the behavior fixtures are taken from that release, and API behavior is ported from it. Known deviations are listed under [Known limitations](#known-limitations).

`tests/diff/diff.py` starts the Java komga and kmrs side by side over the same fixture library and compares ~105 endpoints (status, normalized JSON/XML bodies, headers, zip structure):

```sh
python3 tests/diff/diff.py --java-jar /path/to/komga.jar --rust-bin ./target/debug/kmrs
```

## Development

```sh
cargo nextest run --workspace      # run tests (self-contained: fixtures are vendored under crates/*/tests/resources)
cargo clippy --all-targets         # lint
cargo xtask sync-migrations        # reconcile with komga's Flyway migrations (requires a komga source checkout)
cargo xtask dump-schema            # print the final migrated schema
cargo xtask dump-checksums         # print Flyway CRC32 for all migrations
```

`sync-migrations` looks for a `komga` source checkout next to this repo by default; `KOMGA_REPO_DIR` can be used to point elsewhere. The checkout should be at the compatibility target (`v1.27.0`).

### Profiling memory usage

Release binaries (Linux and macOS) ship with heap profiling: the global allocator is jemalloc with allocation sampling always on — the same model as Go's pprof, a backtrace per ~512 KiB allocated. The profile endpoint is ADMIN-only:

```sh
curl -H "X-API-Key: $KEY" -o heap.profile http://localhost:25600/debug/pprof/heap
jeprof --svg ./kmrs heap.profile   # or --collapsed for flamegraphs
```

Release binaries keep their symbol table, so the published binary from the same release symbolizes the dump. Sampling can be toggled at runtime with `kill -USR1 <pid>` (in Docker: `docker kill --signal=USR1 kmrs`); when it's off the endpoint answers 409. `kill -USR2 <pid>` writes the same dump to `$TMPDIR/kmrs.<pid>.<seq>.heap` instead.

For local analysis, `cargo build --profile profiling --features profiling` produces a release binary with line tables.

### Structure

- `crates/komga-core`: domain model, TSID, time encoding/decoding, natural-sort comparator, error codes
- `crates/komga-db`: Flyway-compatible migrator (migration files are byte-for-byte copies of komga's Flyway migrations), connection pool, UDFs/collations, DAO
- `crates/komga-media`: media pipeline (sniffing/extraction/hashing/thumbnails/metadata)
- `crates/komga-search`: tantivy search index and Lucene query syntax
- `crates/komga-server`: axum HTTP layer (DTOs, authentication, SSE, OPDS, task queue)
- `xtask`: engineering helper commands

## Known limitations

Places where kmrs deviates from the Java version. Scope exclusions are intentional and not listed: no web UI, and no actuator endpoints or metrics that only expose JVM/Spring internals (beans, conditions, env, configprops, loggers, mappings, heapdump, threaddump, `jvm.*`/`system.*`/`http.server.requests` meters and the like).

### Media formats

- No JXL / HEIF / JPEG2000 decoding: those types are sniffed but excluded from the readable image types, so page convert/resize fails like an unsupported reader. The Java version decodes them via ImageIO/TwelveMonkeys.
- PDF support requires a runtime libpdfium, looked up in `KOMGA_PDFIUM_PATH`, next to the executable, then system paths; when unavailable, every PDF operation returns `Unsupported`. The Java version bundles PDFBox. The Docker image ships libpdfium next to the binary.
- JPEG output is not byte-identical to ImageIO (different encoder) — an accepted deviation that affects byte-level comparisons of thumbnails and page hashes.

### Search

- Lucene fuzzy (`~`) and phrase-slop (`~N`) queries are unsupported and yield empty results.
- `komga.lucene.index-analyzer.*` and `komga.lucene.commit-delay` are ignored (warned and dropped during Java config migration); the analyzer is fixed to the multilingual ngram chain.

### Database / migrations

- Deprecated BCP47 aliases (e.g. `iw` → `he`) are not normalized by the language-code migration for pre-2023-08 libraries — an accepted deviation.

### Ignored Java configuration keys

Warned about and dropped during `application.yml` migration:

- `server.error.*`
- `komga.database.batch-chunk-size`, `komga.tasks-db.batch-chunk-size`
- `komga.database.check-local-filesystem`, `komga.tasks-db.check-local-filesystem`

Ignored but behavior-equivalent (not limitations): `server.tomcat.*` (tomcat-specific), `server.forward-headers-strategy` (always `framework`), shutdown handling (always graceful).

## License

kmrs is under the [MIT License](LICENSE). The SQL migration files, the OpenAPI document, and the test fixtures are copied from the [komga](https://github.com/gotson/komga) source tree (see [NOTICE](NOTICE)); everything else is a rewritten implementation. kmrs is not affiliated with the komga project.
