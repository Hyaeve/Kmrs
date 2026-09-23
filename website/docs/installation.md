---
title: Installation
---

# Installation

## Docker

Every release publishes an image to `ghcr.io/kmworks/kmrs` (tags: `latest`, `MAJOR.x`, `x.y.z`; platforms: `linux/amd64`, `linux/arm64`). The [official Komga Docker instructions](https://komga.org/docs/installation/docker) apply verbatim; just swap the image name:

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

An existing komga `/config` directory (with `database.sqlite` / `tasks.sqlite`) is picked up and upgraded in place.

The image bundles the kmweb UI at `/webui` and serves it at `/`, so `http://<host>:25600/` works in a browser immediately. Run with an empty `KOMGA_WEBUI_DIR=` to disable the UI.

## Prebuilt binaries

Download the archive for your platform from the [latest release](https://github.com/kmworks/kmrs/releases/latest) (Linux, macOS, Windows; x86_64 and aarch64).

The binary serves on port 25600 with data directory `~/.komga` (override with `KOMGA_CONFIG_DIR`).

## Build from source

```sh
cargo build --release -p komga-server   # produces target/release/kmrs
```
