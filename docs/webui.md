# Using the original Komga web UI with kmrs

kmrs does not embed a web UI in the binary (the release docker image bundles
one — see below). If you run the bare binary and want a browser interface,
build the official
[komga-webui](https://github.com/gotson/komga) yourself — kmrs can serve it for you
(option A), or you can host it behind a reverse proxy (option B). Both give you the
full web UI: basic and OAuth2 login, the divina/EPUB readers, live updates over SSE,
and the admin pages.

## Build the web UI

Either option starts with a build from the komga source at kmrs's compatibility
target (komga 1.27.0; note the tags carry no `v` prefix):

```sh
git clone https://github.com/gotson/komga.git
cd komga
git checkout 1.27.0
cd komga-webui
npm ci                   # Node 22, see .nvmrc
NODE_OPTIONS=--max-old-space-size=4096 npm run build   # outputs dist/
```

## Docker image

The release image (`ghcr.io/kmworks/kmrs`) bundles the web UI at `/komga-webui`
and presets `KOMGA_WEBUI_DIR` to it, so the UI works out of the box — options A
and B below are for running the bare binary. Run with an empty
`KOMGA_WEBUI_DIR=` to disable the UI.

For a local `docker build`, stage the webui bundle next to the binaries
yourself — the Dockerfile only packages, it compiles nothing:

```sh
mkdir -p dist
# prebuilt bundle from kmworks/kmweb …
gh release download komga-webui/v1.27.0 -R kmworks/kmweb \
  -p 'komga-webui-*.tar.gz' -O dist/webui.tar.gz
# … or tar up your own build from the section above
tar -czf dist/webui.tar.gz -C /path/to/komga-webui/dist .
```

## Option A: let kmrs serve it (simplest)

Point kmrs at the `dist/` directory — `webui.dir` in `<config-dir>/config.toml`, or
the environment:

```sh
KOMGA_WEBUI_DIR=/path/to/komga-webui/dist kmrs
```

That is all. kmrs serves the files at `/`, and paths that match no backend route
(e.g. `/login`, `/libraries/<id>`) fall back to `index.html`, so the UI's
history-mode routing works on refresh. Cache headers mirror the Java version:
content-hashed assets (`css/`, `fonts/`, `img/`, `js/`, `assets/`) are cached for a
year, entry files are `no-store`. Misses under backend prefixes (`/api/`, `/opds/`,
`/sse/`, …) stay 404.

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
must route the API prefixes to kmrs and serve everything else from `dist/`:

```nginx
# kmrs + komga-webui — complete nginx example

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
    root /var/www/komga-webui;
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

    # 9) static files — mirrors komga's WebMvcConfiguration:
    #    content-hashed assets cache for a year, entry files never cached
    location ~* ^/(css|fonts|img|js|assets)/ {
        add_header Cache-Control "public, max-age=31536000";
        try_files $uri =404;
    }
    location ~* ^/(index\.html|favicon.*|manifest\.json|mstile-.*|apple-touch-icon.*|android-chrome-.*)$ {
        add_header Cache-Control "no-store";
        try_files $uri =404;
    }

    # 10) everything else is the SPA; history-mode routes fall back to index.html
    location / {
        try_files $uri $uri/ /index.html;
    }
}
```

## Prefix reference

kmrs listens on the following top-level prefixes. Options A and B1 need none of
this (kmrs routes internally); option B2 needs the ones you use.

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
