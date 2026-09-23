# kmrs

[![CI](https://github.com/kmworks/kmrs/actions/workflows/ci.yml/badge.svg)](https://github.com/kmworks/kmrs/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/kmworks/kmrs)](https://github.com/kmworks/kmrs/releases/latest)
[![Docker image](https://img.shields.io/badge/ghcr.io-kmworks%2Fkmrs-blue)](https://github.com/kmworks/kmrs/pkgs/container/kmrs)
[![License: MIT](https://img.shields.io/github/license/kmworks/kmrs)](LICENSE)

**[Documentation](https://kmworks.github.io/kmrs/)** · [Installation](https://kmworks.github.io/kmrs/docs/installation) · [Configuration](https://kmworks.github.io/kmrs/docs/configuration)

A comic & manga server in a single static Rust binary, drop-in compatible with [Komga](https://komga.org) — same API, same database.

*kmrs: Keep Manga Reading Simple.*

- **Drop-in replacement** for `gotson/komga`: same port (25600), same `/config` and `/data` mounts, same `KOMGA_*` environment variables
- **Data-level compatibility**: opens and upgrades existing komga data directories (`database.sqlite`, `tasks.sqlite`) in place — and the Java version can still open libraries written by kmrs
- **API parity**: REST `/api/**`, OPDS v1.2/v2, SSE, Kobo sync, and KOReader progress sync — endpoints, DTOs, pagination, error shapes, and authentication behavior match the Java version
- **Verified against the Java version**: byte-for-byte Flyway migrations, a differential test harness comparing ~105 endpoints against a live Java instance, and schema contract tests

## Quick start

```sh
docker run -d \
  --name=komga \
  --user 1000:1000 \
  -p 25600:25600 \
  --mount type=bind,source=/path/to/config,target=/config \
  --mount type=bind,source=/path/to/data,target=/data \
  --restart unless-stopped \
  ghcr.io/kmworks/kmrs
```

The image bundles [kmweb](https://github.com/kmworks/kmweb), a React web UI built for kmrs, served at `/` out of the box. Prebuilt binaries (Linux, macOS, Windows; x86_64 and aarch64) are on the [releases page](https://github.com/kmworks/kmrs/releases/latest) — the standalone binary carries no UI, pair it with a Komga-compatible client like [KMReader](https://github.com/kmworks/kmreader), KOReader, or Kobo.

## Documentation

Full documentation lives at **[kmworks.github.io/kmrs](https://kmworks.github.io/kmrs/)**:

- [Installation](https://kmworks.github.io/kmrs/docs/installation) — Docker, prebuilt binaries, build from source
- [Configuration](https://kmworks.github.io/kmrs/docs/configuration) — `config.toml` key reference, env vars, precedence
- [Serving a web UI](https://kmworks.github.io/kmrs/docs/webui) — bundled kmweb, auto-updates, reverse-proxy setups
- [Search](https://kmworks.github.io/kmrs/docs/search) — analyzer chain, CJK cross-search extensions
- [Compatibility](https://kmworks.github.io/kmrs/docs/compatibility) and [Known limitations](https://kmworks.github.io/kmrs/docs/limitations)
- [Development](https://kmworks.github.io/kmrs/docs/development) — tests, crate structure, heap profiling

## License

kmrs is under the [MIT License](LICENSE). The SQL migration files, the OpenAPI document, and the test fixtures are copied from the [komga](https://github.com/gotson/komga) source tree (see [NOTICE](NOTICE)); everything else is a rewritten implementation. kmrs is not affiliated with the komga project.
