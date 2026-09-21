# syntax=docker/dockerfile:1

# Packages prebuilt kmrs binaries; nothing is compiled here. Expected layout,
# as produced by the release workflow (or by hand for a local build):
#   dist/amd64/kmrs   x86_64-unknown-linux-gnu build
#   dist/arm64/kmrs   aarch64-unknown-linux-gnu build
#   dist/webui.tar.gz  prebuilt kmweb bundle (see docs/webui.md)

# Runs on the build platform, so no emulation is ever needed: downloads
# libpdfium (a lazy runtime dependency for PDF support; kmrs looks it up next
# to the executable) and kepubify (EPUB -> KEPUB conversion for Kobo sync,
# same as the komga image), and prepares the mount points for the target stage.
FROM --platform=$BUILDPLATFORM debian:trixie-slim AS base
ARG TARGETARCH
RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && case "$TARGETARCH" in \
      amd64) arch=x64; kepub=64bit ;; \
      arm64) arch=arm64; kepub=arm64 ;; \
      *) echo "unsupported TARGETARCH: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    curl -fsSL "https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-linux-$arch.tgz" \
      | tar -xz -C /tmp --strip-components=1 lib/libpdfium.so \
 && test -s /tmp/libpdfium.so \
 && curl -fsSL "https://github.com/pgaskin/kepubify/releases/latest/download/kepubify-linux-$kepub" -o /tmp/kepubify \
 && test -s /tmp/kepubify \
 && install -d -m 777 /staging/config /staging/data

FROM debian:trixie-slim
ARG TARGETARCH
# compose healthchecks shell out to curl (the Java image ships one); the slim base doesn't
RUN apt-get update && apt-get install -y --no-install-recommends curl \
 && rm -rf /var/lib/apt/lists/*
LABEL org.opencontainers.image.source="https://github.com/kmworks/kmrs" \
      org.opencontainers.image.description="Rust rewrite of the Komga server, with the kmweb UI bundled" \
      org.opencontainers.image.licenses="MIT"
# Drop-in replacement for gotson/komga: same port, same /config and /data
# mounts, same KOMGA_* env vars.
# KOMGA_WEBUI_DIR points at the bundled web UI; run with an empty value to disable.
ENV KOMGA_CONFIG_DIR=/config \
    KOMGA_KOBO_KEPUBIFYPATH=/usr/local/bin/kepubify \
    KOMGA_WEBUI_DIR=/webui
COPY --chmod=755 "dist/$TARGETARCH/kmrs" /usr/local/bin/kmrs
COPY --from=base /tmp/libpdfium.so /usr/local/bin/libpdfium.so
COPY --chmod=755 --from=base /tmp/kepubify /usr/local/bin/kepubify
# ADD auto-extracts the tarball into /webui
ADD dist/webui.tar.gz /webui
# 777 so an arbitrary --user uid:gid can write when nothing is bind-mounted;
# COPY of a directory preserves the modes set in the base stage
COPY --from=base /staging /
EXPOSE 25600
ENTRYPOINT ["/usr/local/bin/kmrs"]
