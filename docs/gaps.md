# Compatibility gaps vs komga 1.27.0

Known places where kmrs deviates from the Java version, each linked to the code that declares it. Scope exclusions (no web UI) are intentional and not listed here.

## Kobo sync

- kepubify conversion is not integrated: `convert_kepub=true` on `GET /kobo/{token}/v1/books/{id}/file/epub` always fails (503), download URLs always advertise `EPUB3` without conversion, and the `kepubifyPath` setting is stored but never consumed. (`crates/komga-server/src/api/kobo.rs:908`, `crates/komga-server/src/settings.rs:70`, `crates/komga-media/src/analyzer.rs:1263`)
- Kobo proxy (`koboProxy`) is unavailable: unmatched `/kobo/{token}/*` paths hit a catch-all returning `{}`. (`crates/komga-server/src/api/kobo.rs:987`)
- Reading positions for plain (non-KEPUB) EPUBs are estimated from resource sizes instead of real kobo spans — a downstream effect of the missing kepubify integration. (`crates/komga-media/src/analyzer.rs:1258`)

## Media formats

- No JXL / HEIF / JPEG2000 decoding: those types are sniffed but excluded from the readable image types, so page convert/resize fails like an unsupported reader. The Java version decodes them via ImageIO/TwelveMonkeys. (`crates/komga-media/src/container.rs:108`)
- PDF support requires a runtime libpdfium, looked up in `KOMGA_PDFIUM_PATH`, next to the executable, then system paths; when unavailable, every PDF operation returns `Unsupported`. The Java version bundles PDFBox. The Docker image ships libpdfium next to the binary. (`crates/komga-media/src/pdf.rs:3`)
- JPEG output is not byte-identical to ImageIO (different encoder) — an accepted deviation that affects byte-level comparisons of thumbnails and page hashes. (`crates/komga-media/src/image.rs:3`)

## Search

- Lucene fuzzy (`~`) and phrase-slop (`~N`) queries are unsupported and yield empty results. (`crates/komga-search/src/syntax.rs:6`)
- `komga.lucene.index-analyzer.*` and `komga.lucene.commit-delay` are ignored (warned and dropped during Java config migration); the analyzer is fixed to the multilingual ngram chain. (`crates/komga-server/src/config/java.rs:237`)
- Index codec upgrade is a no-op: tantivy has no such concept, and version-based reindexing already covers it. (`crates/komga-search/src/lib.rs:320`)

## Tasks / events

- Changing a library's `scanInterval` does not reschedule its periodic scan; the new interval takes effect on restart. (`crates/komga-server/src/service/library.rs:87`)
- The Mihon read-progress endpoint (`PUT /api/v2/series/{id}/read-progress/tachiyomi`) marks books read but emits no SSE events; other read-progress paths do publish. (`crates/komga-server/src/api/series.rs:876`)

## Database / migrations

- Flyway baseline is not supported: a database that has objects but no `flyway_schema_history` table hard-errors instead of being adopted. (`crates/komga-db/src/migrate.rs:157`)
- Deprecated BCP47 aliases (e.g. `iw` → `he`) are not normalized by the language-code migration for pre-2023-08 libraries — an accepted deviation. (`crates/komga-db/src/java_migrations.rs:156`)

## Actuator

- Only a subset of Spring's actuator is implemented (health, info, metrics, scheduledtasks, shutdown), and metrics are limited to about ten names. (`crates/komga-server/src/api/actuator.rs:329`)

## Ignored Java configuration keys

Warned about and dropped during `application.yml` migration (`crates/komga-server/src/config/java.rs:173`, `crates/komga-server/src/config/java.rs:331`):

- `server.error.*`
- `komga.database.batch-chunk-size`, `komga.tasks-db.batch-chunk-size`
- `komga.database.check-local-filesystem`, `komga.tasks-db.check-local-filesystem`

Ignored but behavior-equivalent (not gaps): `server.tomcat.*` (tomcat-specific), `server.forward-headers-strategy` (always `framework`), shutdown handling (always graceful).
