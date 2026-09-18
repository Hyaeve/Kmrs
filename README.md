# komga-rs

A Rust rewrite of the [Komga](https://github.com/gotson/komga) server (work in progress). The goal is **full compatibility with the Java version's data formats and API behavior**:

- Can directly open/upgrade existing komga data directories (`database.sqlite`, `tasks.sqlite`), and the Java version of komga can still open libraries written by this version
- Endpoints, DTOs, pagination, error shapes, and authentication behavior for REST `/api/**`, OPDS v1.2/v2, SSE, Kobo, and KOReader match the Java version
- No UI

## Structure

- `crates/komga-core`: domain model, TSID, time encoding/decoding, natural-sort comparator, error codes
- `crates/komga-db`: Flyway-compatible migrator (migration files are byte-for-byte copies of komga's Flyway migrations), connection pool, UDFs/collations, DAO
- `crates/komga-media`: media pipeline (sniffing/extraction/hashing/thumbnails/metadata, WIP)
- `crates/komga-search`: search (WIP)
- `crates/komga-server`: axum HTTP layer (DTOs, authentication, SSE, OPDS, task queue, WIP)
- `xtask`: engineering helper commands

## Development

```sh
cargo test --workspace          # run tests
cargo clippy --all-targets      # lint
cargo xtask sync-migrations     # reconcile with komga's Flyway migrations (requires a komga source checkout)
cargo xtask dump-schema         # print the final migrated schema
cargo xtask dump-checksums      # print Flyway CRC32 for all migrations
```

`sync-migrations` looks for a `komga` source checkout next to this repo by default; `KOMGA_REPO_DIR` can be used to point elsewhere.

Run: `cargo run -p komga-server` (default port 25600, data directory `~/.komga`, overridable with `KOMGA_CONFIG_DIR`).

## License

komga is under the [MIT License](LICENSE). The SQL migration files in this repo are copied from the komga source tree; everything else is a rewritten implementation.
