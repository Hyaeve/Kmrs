# AGENTS.md

Rules that must not be violated when working in this repository. For architecture and feature details, read the code.

## Project

**kmweb** is the web UI of [kmrs](https://github.com/kmworks/kmrs), built with React 19, TypeScript, Vite, Tailwind CSS v4, TanStack Query, Zustand and Radix UI. It lives in `webui/` of the kmrs repository and is embedded into the kmrs binary at build time, version-locked to the server. There are no test targets: validate with builds, lint, and manual testing.

## Commands

```bash
pnpm dev      # dev server on :5173, proxies /api /sse /actuator to :25600
pnpm build    # typecheck + production bundle; the validation gate
pnpm lint     # eslint
```

## Coding Conventions

1. **Comments**: minimal, English only; explain why, not what; no issue/PR numbers (link issues in the PR description instead).
2. **Git-facing text**: commit messages, PR titles/bodies, and review comments are always in English.
3. Light/dark color differences belong in the CSS custom properties of the global stylesheet (`--ink`, `--raised`, `--line`, …), not in JS theme branching.
4. Icon weight (phosphor `weight="fill"` and friends) is a render-site decision: components expose the base icon and the site that knows its context picks the weight. Exception: an icon that renders filled in every context is part of the status's identity (e.g. the completed checkmark).
5. Never render empty wrapper elements; put the condition around the wrapper itself.
6. API endpoints live in `src/lib/api/*`; request building stays out of components.
7. Components passed as `trigger` to `Menu` (Radix `asChild`) must forward props and ref (`forwardRef` + `{...rest}`) — the injected handlers and state attributes never reach the DOM otherwise, and the menu silently never opens.
8. When a change alters one of these conventions or a subsystem boundary, update this file in the same change.
9. This file documents conventions, not code layout: never name specific source files — they move and the reference rots. Name subsystems, directories, or components instead.
10. **i18n**: all user-facing strings go through react-i18next (`t()` / `<Trans>`), never literal JSX text or string-valued attributes (`aria-label`, `title`, `placeholder`). Locale resources live in `src/lib/i18n/locales/<lang>/<namespace>.json` and are registered by glob — adding a file needs no other wiring; `en` is the source language and every other locale mirrors its key set (the supported-language list is declared in the i18n module). Shared vocabulary and enum label maps live in the `common` namespace; feature areas use their own. Plurals use i18next `count` with the locale's CLDR categories as key suffixes (`key_one`/`key_other` for en, `key_other` only for CJK, `key_one`/`key_few`/`key_many`/`key_other` for ru, …); sentences with dynamic parts use interpolation, never string concatenation. Backend error messages pass through untranslated. Dates and relative times go through the locale-aware helpers in the format utils, not ad-hoc `toLocaleString` calls; page titles go through `useDocumentTitle`.
11. **PWA**: the service worker caches only the app shell (navigations network-first, hashed `/assets/*` cache-first) and deliberately never intercepts backend traffic — any new proxied path must be added to the bypass list in the worker alongside `/api`, `/sse`, `/actuator`. Home-screen icons are pre-rendered PNGs from the official KMReader 2048 canvas (light brand: white icon fill, maskable keeps the glyph inside the safe zone, monochrome is the silhouette for Android themed icons); splash `background_color` must match the icon fill.

## Cards

- Grid cards (books, series) navigate to the detail page; only horizontal cards (Keep Reading) open the reader directly. The card menu complements the tap and never duplicates it: grid cards get a Read action, horizontal cards get Book details — Read and Details are mutually exclusive. Every Read entry point is paired with a Peek action (reader with `?incognito=true`, no progress saved) — grid card menus, the series/readlist/book/oneshot detail pages; the horizontal Keep Reading cards are the exception.
- Oneshots always land on the oneshot detail page (`/oneshot/:seriesId`): cards link there directly, and the book/series detail pages redirect as a safety net for URLs built elsewhere.
- Card menus share `BookCardMenu` / `SeriesCardMenu` (they also host the dialogs their items open). The trigger is the hover/focus-revealed `CardMenuButton` at the cover's top-left, always visible at reduced opacity on touch (`pointer-coarse`); menus hide in selection mode.
- Entering selection mode is a menu item, so the corner checkbox (`SelectBadge`) appears only once selection is active.
- In selection mode, shift+click toggles the inclusive range between the last toggled card and the clicked one — deselecting when the clicked card is selected, selecting otherwise. Grids pass their loaded id order to `cardSelection` so ranges can span pages loaded so far.
- Corner badges: series cards show the unread count, which hides in selection mode (like the card menu) so it doesn't fight the `SelectBadge`. Book cards have no corner badge — the completed state already shows in the meta line.
- Horizontal cards follow the Apple Books contract: semibold title on top, series line under it, meta line at the bottom in stepped-down sizes (13/12/11px); vertical slack distributes evenly across every gap; the cover height matches the four-line text column so the card is no taller than its text. They sit on a cover-tinted background — the cover's average color, saturation- and brightness-clamped so white text stays readable (`useCoverTint`); until the tint resolves the card falls back to the plain card fill with adaptive colors. On the tint all text is white: title full (70% once completed), series 85%, meta and accessories 70%.
- Oneshots are marked by a shared `OneshotLine`: a single-book icon (the `Book` counterpart to the `Books` stack that means series) followed by the primary author, falling back to the plain "Oneshot" label when no author is known. Book grid cards show it in the series-title slot (the title still clamps to one line), oneshot series cards as their status line (checkmark when completed), and Keep Reading cards in the series line.
