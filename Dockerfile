# syntax=docker/dockerfile:1

# Packages prebuilt kmrs binaries; nothing is compiled here. Expected layout,
# as produced by the release workflow (or by hand for a local build):
#   dist/amd64/kmrs   x86_64-unknown-linux-gnu build
#   dist/arm64/kmrs   aarch64-unknown-linux-gnu build

# Runs on the build platform, so no emulation is ever needed: downloads
# libpdfium (a lazy runtime dependency for PDF support; kmrs looks it up next
# to the executable) and prepares the mount points for the target stage.
FROM --platform=$BUILDPLATFORM debian:trixie-slim AS base
ARG TARGETARCH
RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && case "$TARGETARCH" in \
      amd64) arch=x64 ;; \
      arm64) arch=arm64 ;; \
      *) echo "unsupported TARGETARCH: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    curl -fsSL "https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-linux-$arch.tgz" \
      | tar -xz -C /tmp --strip-components=1 lib/libpdfium.so \
 && test -s /tmp/libpdfium.so \
 && install -d -m 777 /staging/config /staging/data

FROM debian:trixie-slim
ARG TARGETARCH
LABEL org.opencontainers.image.source="https://github.com/everpcpc/kmrs" \
      org.opencontainers.image.description="Rust rewrite of the Komga server (API only, no UI)" \
      org.opencontainers.image.licenses="MIT"
# Drop-in replacement for gotson/komga: same port, same /config and /data
# mounts, same KOMGA_* env vars.
ENV KOMGA_CONFIG_DIR=/config
COPY --chmod=755 "dist/$TARGETARCH/kmrs" /usr/local/bin/kmrs
COPY --from=base /tmp/libpdfium.so /usr/local/bin/libpdfium.so
# 777 so an arbitrary --user uid:gid can write when nothing is bind-mounted;
# COPY of a directory preserves the modes set in the base stage
COPY --from=base /staging /
EXPOSE 25600
ENTRYPOINT ["/usr/local/bin/kmrs"]
