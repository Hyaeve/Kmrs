---
slug: /
title: Introduction
---

# kmrs

kmrs is a drop-in, API-compatible reimplementation of the [Komga](https://komga.org) comic/manga server in Rust: a single static binary.

*The name: Keep Manga Reading Simple.*

:::note
The docker image bundles [kmweb](https://github.com/kmworks/kmweb), a React web UI built for kmrs, served at `/` out of the box. The standalone binary carries no UI: pair it with a Komga-compatible client ([KMReader](https://github.com/kmworks/kmreader) for iOS/macOS/tvOS, KOReader, Kobo, and [others](https://komga.org/docs/category/readers)) or let it serve a built webui. See [Serving a web UI](./webui.md).
:::

## Features

- **Drop-in replacement** for `gotson/komga`: same port (25600), same `/config` and `/data` mounts, same `KOMGA_*` environment variables
- **Data-level compatibility**: opens and upgrades existing komga data directories (`database.sqlite`, `tasks.sqlite`) in place, and the Java version can still open libraries written by kmrs
- **API parity**: REST `/api/**`, OPDS v1.2/v2, SSE, Kobo sync, and KOReader progress sync, with endpoints, DTOs, pagination, error shapes, and authentication behavior matching the Java version
- **Verified against the Java version**: byte-for-byte Flyway migrations, a differential test harness comparing ~105 endpoints against a live Java instance, and schema contract tests

## Enhancements

On top of drop-in compatibility, kmrs adds improvements the Java version does not have:

- **Search**: simplified ↔ traditional Chinese cross-search and CJK boundary unigrams (e.g. `3月` matches `3月的狮子`). See [Search](./search.md).
