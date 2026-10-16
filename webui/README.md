# kmweb

Web UI for kmrs, the Komga-compatible media server rewritten in Rust. It lives
in the `webui/` directory of the [kmrs](https://github.com/kmworks/kmrs)
repository and is embedded into the kmrs binary at build time, version-locked
to the server.

Built with React 19, Vite, Tailwind CSS v4, TanStack Query and Zustand.
The design language follows [KMReader](https://github.com/kmworks/kmreader):
dark-first themes, a warm orange accent, √2 covers, serif section titles and
cover-tinted resume cards.

## Features

- Dashboard with configurable library scope: Keep Reading, On Deck, Recently
  Released/Added Books, Recently Added/Updated Series, Recently Read
- Browse series, books, collections and read lists with facet filters
  (read status, genres, tags, publishers, authors, age rating, language,
  release years), sort options and infinite scroll
- Series and book detail pages with metadata, progress and quick actions
- Full-text search across series, books, collections and read lists
- Comic reader: single/double page spreads (with cover handling), LTR/RTL,
  vertical and webtoon modes, fit/original scaling, keyboard shortcuts, swipe,
  thumbnail explorer, per-series direction override, read-progress sync and
  continuous reading across books (read-list context aware)
- Account pages in the sidebar: profile, security (password change, login
  activity), API keys, appearance (theme, card style, grid density, spoiler
  blur) and reader defaults
- Live updates over SSE (lists refresh and covers reload when the server
  changes them)

## Develop

```sh
pnpm install
pnpm dev           # http://localhost:5173, proxies /api /sse /actuator to localhost:25600
pnpm lint
pnpm build         # type-check + production bundle in dist/
```

A kmrs build embeds `dist/` into the binary (a placeholder page is substituted
when `dist/` is absent) and serves it at `/` with SPA fallback, so rebuild the
bundle before `cargo build` when you want the UI in the served binary.

## Reader shortcuts

Arrows turn pages (mirrored in RTL, vertical in vertical mode), `Home`/`End`
first/last page, `L`/`R`/`V`/`W` reading direction, `C` scale, `D` page
layout, `F` fullscreen, `M` menu, `S` settings, `T` thumbnails, `H` help,
`Esc` close/back. Webtoon: `Space`/`PageUp`/`PageDown` scroll, `P` side
padding, `N` page gap.
