# AGENTS.md

Rules for working in this repository. They apply to humans and agents alike.

## Layout

- Rust workspace: `crates/komga-core` (shared primitives), `crates/komga-db` (SQLite, DAOs, migrations), `crates/komga-media` (analyzers, metadata readers), `crates/komga-search` (tantivy), `crates/komga-server` (HTTP API, config, services). `xtask/` holds dev tasks.
- `website/`: Docusaurus documentation site.
- `examples/config.toml`: the canonical all-keys configuration reference.
- `docs/schema-main.sql`: reference schema of the main database.
- `tests/diff/`: differential test harness that compares kmrs against a live Java komga instance.

## Compatibility (the core promise)

kmrs is a drop-in, data-level compatible reimplementation of [gotson/komga](https://github.com/gotson/komga); the compatibility target is **komga 1.27.1**.

- The schemas of `database.sqlite` and `tasks.sqlite` are byte-for-byte the Java Flyway migrations. `crates/komga-db/tests/schema_contract.rs` enforces this against `docs/schema-main.sql`.
- kmrs-private state never goes into the main database. It lives in `kmrs.sqlite` — its own migration track (`crates/komga-db/migrations_kmrs/`, `kmrs_migrations()`), its own connection, its own config key (`kmrs-db` / `KOMGA_KMRSDB_FILE`) — so the Java version can always open a kmrs-written data directory.
- When porting behavior, verify against the Java/Kotlin sources, never from memory.
- Actuator endpoints and metrics that only expose JVM/Spring internals (beans, env, loggers, mappings, `jvm.*`, `system.*`, ...) are intentional scope exclusions, not gaps.

## Versioning

- The version lives in the workspace root `Cargo.toml` (`workspace.package.version`); crates inherit it via `version.workspace = true`.
- A version bump rides along with the feature PR as a separate `chore: bump version` commit — never its own PR.

## Configuration keys

A new configuration key must land in all three places: the loader/render (`crates/komga-server/src/config/`), `examples/config.toml`, and `website/docs/configuration.md`.

## Documentation (website/docs)

- `limitations.md`: accepted gaps that are not planned to be closed (e.g. Lucene fuzzy/slop queries).
- `enhancements.md`: improvements over the Java version, plus intentional behavior differences.
- `compatibility.md`: parity target and the differential harness; links to the two pages above.
- Verify changes with `npm run build` in `website/` — Docusaurus fails on broken links.

## Comments

- Comments say why, never what. No migration history ("no longer", "previously", dates, codenames) — the code and git history already say that.
- Assertions about dependency behavior (stdlib, crates) must be checked against the dependency's source before writing them down.

## Checks and workflow

- CI: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test`, builds on ubuntu/macos/windows, docker. Keep clippy warning-free.
- PRs are in English (title, body, comments).
- Squash-merge with a hand-written commit message (`gh pr merge --squash --subject ... --body ...`), never the default message.
