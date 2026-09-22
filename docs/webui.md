# Serving a web UI with kmrs

kmrs does not embed a web UI in the binary (the release docker image bundles
one — see below). If you run the bare binary and want a browser interface,
point kmrs at a built [kmweb](https://github.com/kmworks/kmweb) bundle and it
serves it for you (option A), or host the files behind a reverse proxy
(option B). kmweb is the React UI built for kmrs: dashboard, browsing with
facet filters, full-text search, the comic reader, account settings, live
updates over SSE.

## Get the web UI files

Download the prebuilt bundle from a [kmweb release](https://github.com/kmworks/kmweb/releases)
and unpack it:

```sh
gh release download v0.3.0 -R kmworks/kmweb -p 'kmweb-v0.3.0.tar.gz'
mkdir kmweb && tar -xzf kmweb-v0.3.0.tar.gz -C kmweb
```

Or build from source (Node 24, pnpm):

```sh
git clone https://github.com/kmworks/kmweb.git
cd kmweb
pnpm install
pnpm build              # outputs dist/
```

## Docker image

The release image (`ghcr.io/kmworks/kmrs`) bundles kmweb at `/webui` and
presets `KOMGA_WEBUI_DIR` to it, so the UI works out of the box — options A
and B below are for running the bare binary. Run with an empty
`KOMGA_WEBUI_DIR=` to disable the UI. The bundled bundle is stamped with its
kmweb version in `/webui/kmweb.version`, which the auto-updater below uses to
tell whether an update is needed.

For a local `docker build`, stage the webui bundle next to the binaries
yourself — the Dockerfile only packages, it compiles nothing:

```sh
mkdir -p dist
# prebuilt kmweb bundle …
gh release download v0.3.0 -R kmworks/kmweb \
  -p 'kmweb-v0.3.0.tar.gz' -O dist/webui.tar.gz
# … or tar up your own build from the section above
tar -czf dist/webui.tar.gz -C /path/to/kmweb/dist .
```

## Automatic updates

The bundled UI is only a baseline — kmrs can track new kmweb releases at
runtime, with no container rebuild or restart. Off by default; enable it in
`<config-dir>/config.toml` or via the environment:

```sh
KOMGA_WEBUI_AUTOUPDATE=true
KOMGA_WEBUI_UPDATEINTERVAL=6h   # how often to check, default 1d
```

With auto-update on and a web UI enabled, kmrs checks the latest kmweb release
on startup and then on the interval, downloads the bundle, verifies it against
the sha256 published with the release, and swaps the served directory
atomically. The managed copy lives under `<config-dir>/webui` (`/config/webui`
in the container), so it survives image upgrades; browsers pick the new version
up on the next page load (`index.html` is never cached).

- A fresh or offline install always has a working UI: as long as no managed
  copy exists, the bundled one is served. If its version is unknown (older
  image, hand-staged bundle without `kmweb.version`), the updater installs the
  latest release once even if it turns out identical.
- `KOMGA_WEBUI_DIR=` (empty) disables the UI entirely, and the updater with it.
- Auto-update tracks the kmweb bundle: with it on, whatever `webui.dir` points
  at is replaced by the latest kmweb release.
- Air-gapped hosts: leave it off and keep pointing `webui.dir` at a bundle you
  stage yourself (option A).

## Option A: let kmrs serve it (simplest)

Point kmrs at the unpacked bundle (or the `dist/` directory of a source
build) — `webui.dir` in `<config-dir>/config.toml`, or the environment:

```sh
KOMGA_WEBUI_DIR=/path/to/kmweb kmrs
```

That is all. kmrs serves the files at `/`, and paths that match no backend route
(e.g. `/login`, `/libraries/<id>`) fall back to `index.html`, so the UI's
history-mode routing works on refresh. Cache headers: content-hashed assets
under `assets/` are cached for a year, everything else is `no-store`. Misses
under backend prefixes (`/api/`, `/opds/`, `/sse/`, …) stay 404.

kmrs speaks plain HTTP. If you need HTTPS, put any TLS-terminating proxy in front —
with option A it can be a dumb pipe, since kmrs tells the SPA and the API apart
itself:

```nginx
server {
    listen 443 ssl;
    server_name komga.example.com;
    ssl_certificate     /etc/letsencrypt/live/komga.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/komga.example.com/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:25600;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header X-Forwarded-Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    }
}
```

## Option B: nginx hosts the SPA itself

Use this when you want nginx to serve the static files (sendfile, its own caching
rules) and only proxy the API. The web UI calls the API on its own origin, so nginx
must route the API prefixes to kmrs and serve everything else from the bundle:

```nginx
# kmrs + web UI — complete nginx example

# 1) HTTP → HTTPS redirect
server {
    listen 80;
    server_name komga.example.com;
    return 301 https://$host$request_uri;
}

server {
    listen 443 ssl;
    http2 on;                          # nginx >= 1.25; older: listen 443 ssl http2;
    server_name komga.example.com;

    # 2) TLS — adjust paths to your ACME client
    ssl_certificate     /etc/letsencrypt/live/komga.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/komga.example.com/privkey.pem;
    ssl_protocols       TLSv1.2 TLSv1.3;
    ssl_session_cache   shared:KomgaTLS:10m;
    ssl_session_timeout 1d;

    # 3) the SPA from the build step
    root /var/www/webui;
    index index.html;

    # book import and thumbnail uploads can be large
    client_max_body_size 0;

    gzip on;
    gzip_types text/css application/javascript application/json image/svg+xml;
    gzip_min_length 1024;

    # common proxy headers, inherited by every location that doesn't set its own:
    # - X-Forwarded-Proto/Host: kmrs builds absolute URLs (OAuth2 redirect_uri, OPDS links) from them
    # - X-Forwarded-For: kmrs reads the client IP from it (forward-headers-strategy: framework)
    proxy_http_version 1.1;
    proxy_set_header Host              $host;
    proxy_set_header X-Real-IP         $remote_addr;
    proxy_set_header X-Forwarded-For   $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
    proxy_set_header X-Forwarded-Host  $host;
    proxy_read_timeout 300s;

    # 4) kmrs API
    location /api/ {
        proxy_pass http://127.0.0.1:25600;
    }

    # 5) SSE live updates — the header set is repeated because any proxy_set_header
    #    in a location disables inheritance from the server level
    location /sse/ {
        proxy_pass http://127.0.0.1:25600;
        proxy_set_header Host              $host;
        proxy_set_header X-Real-IP         $remote_addr;
        proxy_set_header X-Forwarded-For   $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header X-Forwarded-Host  $host;
        proxy_set_header Connection        "";
        proxy_buffering    off;
        proxy_cache        off;
        proxy_read_timeout 1h;
    }

    # 6) OAuth2 login
    location /oauth2/       { proxy_pass http://127.0.0.1:25600; }
    location /login/oauth2/ { proxy_pass http://127.0.0.1:25600; }

    # 7) admin pages: server management, metrics, updates
    location /actuator/ { proxy_pass http://127.0.0.1:25600; }

    # 8) optional clients — drop what you don't use
    location /opds/     { proxy_pass http://127.0.0.1:25600; }  # OPDS v1.2/v2 readers
    location /koreader/ { proxy_pass http://127.0.0.1:25600; }  # KOReader progress sync
    location /kobo/     { proxy_pass http://127.0.0.1:25600; }  # Kobo sync
    location = /v3/api-docs { proxy_pass http://127.0.0.1:25600; }  # OpenAPI document
    # location /debug/ { proxy_pass http://127.0.0.1:25600; }  # pprof heap (ADMIN-only)

    # 9) static files — mirrors kmrs: content-hashed assets cache for a year,
    #    everything else (index.html included) is never cached
    location ~* ^/assets/ {
        add_header Cache-Control "public, max-age=31536000";
        try_files $uri =404;
    }

    # 10) everything else is the SPA; history-mode routes fall back to index.html
    location / {
        add_header Cache-Control "no-store";
        try_files $uri $uri/ /index.html;
    }
}
```

## Prefix reference

kmrs listens on the following top-level prefixes. Option A needs none of
this (kmrs routes internally); option B needs the ones you use.

| Prefix | Purpose | Needed by |
| --- | --- | --- |
| `/api/` | REST API (`/api/v1`, `/api/v2`, `/api/logout`) | web UI |
| `/sse/` | SSE event stream (`/sse/v1/events`) | web UI |
| `/oauth2/` | OAuth2 authorization entry | web UI (with OAuth2 providers) |
| `/login/oauth2/` | OAuth2 callback | web UI (with OAuth2 providers) |
| `/actuator/` | info, shutdown, logfile, metrics | web UI admin pages |
| `/opds/` | OPDS v1.2/v2 catalogs | OPDS readers |
| `/koreader/` | KOReader progress sync | KOReader |
| `/kobo/` | Kobo sync | Kobo |
| `/v3/api-docs` | OpenAPI document | optional |
| `/debug/` | heap profiling endpoint | optional |

## Notes

- The web UI's files and SPA routes are served without authentication — the
  login page has to load anonymously. Authentication is enforced by the API,
  same as the Java version (static resources are `permitAll` there too).

- `/login` is a web UI route; only `/login/oauth2/` goes to kmrs. nginx matches the
  longest prefix, so the two `location` blocks coexist safely.
- Reader pages, thumbnails, and downloads are plain same-origin requests
  authenticated by the session cookie — no extra proxy rules needed.
- `GET /actuator/logfile` returns an empty body (kmrs logs to stderr and keeps no
  log file); the download button in Server Management still succeeds.
- Cross-origin hosting (web UI on a different origin than the API) is not supported:
  kmrs parses `KOMGA_CORS_ALLOWEDORIGINS` but does not apply CORS headers. Serve
  same-origin as shown above.
