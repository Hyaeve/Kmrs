# kmrs

A Rust rewrite of the [Komga](https://komga.org) server. The goal is **full compatibility with the Java version's data formats and API behavior**:

- Can directly open/upgrade existing komga data directories (`database.sqlite`, `tasks.sqlite`), and the Java version of komga can still open libraries written by kmrs
- Endpoints, DTOs, pagination, error shapes, and authentication behavior for REST `/api/**`, OPDS v1.2/v2, SSE, Kobo, and KOReader match the Java version
- No UI

## Structure

- `crates/komga-core`: domain model, TSID, time encoding/decoding, natural-sort comparator, error codes
- `crates/komga-db`: Flyway-compatible migrator (migration files are byte-for-byte copies of komga's Flyway migrations), connection pool, UDFs/collations, DAO
- `crates/komga-media`: media pipeline (sniffing/extraction/hashing/thumbnails/metadata)
- `crates/komga-search`: tantivy search index and Lucene query syntax
- `crates/komga-server`: axum HTTP layer (DTOs, authentication, SSE, OPDS, task queue)
- `xtask`: engineering helper commands

## Development

```sh
cargo test --workspace          # run tests (self-contained: fixtures are vendored under crates/*/tests/resources)
cargo clippy --all-targets      # lint
cargo xtask sync-migrations     # reconcile with komga's Flyway migrations (requires a komga source checkout)
cargo xtask dump-schema         # print the final migrated schema
cargo xtask dump-checksums      # print Flyway CRC32 for all migrations
```

`sync-migrations` looks for a `komga` source checkout next to this repo by default; `KOMGA_REPO_DIR` can be used to point elsewhere.

Run: `cargo run -p komga-server` (default port 25600, data directory `~/.komga`, overridable with `KOMGA_CONFIG_DIR`).

## Compatibility testing

`tests/diff/diff.py` starts the Java komga and kmrs side by side over the same fixture library and compares ~105 endpoints (status, normalized JSON/XML bodies, headers, zip structure):

```sh
python3 tests/diff/diff.py --java-jar /path/to/komga.jar --rust-bin ./target/debug/komga-server
```

## License

kmrs is under the [MIT License](LICENSE). The SQL migration files, the OpenAPI document, and the test fixtures are copied from the komga source tree; everything else is a rewritten implementation.
