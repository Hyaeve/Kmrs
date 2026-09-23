---
title: Known limitations
---

# Known limitations

Places where kmrs deviates from the Java version. Scope exclusions are intentional and not listed: no web UI in the standalone binary (the docker image bundles the kmweb UI), and no actuator endpoints or metrics that only expose JVM/Spring internals (beans, conditions, env, configprops, loggers, mappings, heapdump, threaddump, `jvm.*`/`system.*`/`http.server.requests` meters and the like).

## Media formats

- No JXL / HEIF / JPEG2000 decoding: those types are sniffed but excluded from the readable image types, so page convert/resize fails like an unsupported reader. The Java version decodes them via ImageIO/TwelveMonkeys.
- PDF support requires a runtime libpdfium, looked up in `KOMGA_PDFIUM_PATH`, next to the executable, then system paths; when unavailable, every PDF operation returns `Unsupported`. The Java version bundles PDFBox. The Docker image ships libpdfium next to the binary.
- JPEG output is not byte-identical to ImageIO (different encoder), an accepted deviation that affects byte-level comparisons of thumbnails and page hashes.

## Search

- Lucene fuzzy (`~`) and phrase-slop (`~N`) queries are unsupported and yield empty results.
- `komga.lucene.index-analyzer.*` and `komga.lucene.commit-delay` are ignored (warned and dropped during Java config migration); the analyzer is fixed to the multilingual ngram chain.

## Database / migrations

- Deprecated BCP47 aliases (e.g. `iw` → `he`) are not normalized by the language-code migration for pre-2023-08 libraries, an accepted deviation.

## Ignored Java configuration keys

Warned about and dropped during `application.yml` migration:

- `server.error.*`
- `komga.database.batch-chunk-size`, `komga.tasks-db.batch-chunk-size`
- `komga.database.check-local-filesystem`, `komga.tasks-db.check-local-filesystem`

Ignored but behavior-equivalent (not limitations): `server.tomcat.*` (tomcat-specific), `server.forward-headers-strategy` (always `framework`), shutdown handling (always graceful).
