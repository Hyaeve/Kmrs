---
title: Development
---

# Development

```sh
cargo nextest run --workspace      # run tests (self-contained: fixtures are vendored under crates/*/tests/resources)
cargo clippy --all-targets         # lint
cargo xtask sync-migrations        # reconcile with komga's Flyway migrations (requires a komga source checkout)
cargo xtask dump-schema            # print the final migrated schema
cargo xtask dump-checksums         # print Flyway CRC32 for all migrations
```

`sync-migrations` looks for a `komga` source checkout next to this repo by default; `KOMGA_REPO_DIR` can be used to point elsewhere. The checkout should be at the compatibility target (`1.27.1`).

## Profiling memory usage

Release binaries (Linux and macOS) ship with heap profiling: the global allocator is jemalloc with allocation sampling always on, the same model as Go's pprof, a backtrace per ~512 KiB allocated. The profile endpoint is ADMIN-only:

```sh
curl -H "X-API-Key: $KEY" -o heap.profile http://localhost:25600/debug/pprof/heap
jeprof --svg ./kmrs heap.profile   # or --collapsed for flamegraphs
```

Release binaries keep their symbol table, so the published binary from the same release symbolizes the dump. Sampling can be toggled at runtime with `kill -USR1 <pid>` (in Docker: `docker kill --signal=USR1 kmrs`); when it's off the endpoint answers 409. `kill -USR2 <pid>` writes the same dump to `$TMPDIR/kmrs.<pid>.<seq>.heap` instead.

For local analysis, `cargo build --profile profiling --features profiling` produces a release binary with line tables.

## Structure

- `crates/komga-core`: domain model, TSID, time encoding/decoding, natural-sort comparator, error codes
- `crates/komga-db`: Flyway-compatible migrator (migration files are byte-for-byte copies of komga's Flyway migrations), connection pool, UDFs/collations, DAO
- `crates/komga-media`: media pipeline (sniffing/extraction/hashing/thumbnails/metadata)
- `crates/komga-search`: tantivy search index and Lucene query syntax
- `crates/komga-server`: axum HTTP layer (DTOs, authentication, SSE, OPDS, task queue)
- `xtask`: engineering helper commands
