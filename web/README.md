# adroit web dashboard

A read-only Vue 3 SPA for exploring an ADR repo, served by `adroit serve`
(behind the Rust `web` Cargo feature). It browses/reads ADRs, full-text search,
a stats dashboard, a relationship graph, and a repo-health panel (the same checks
as `adroit check`, via `GET /api/check`), and can switch which ADR directory it
views. It never writes to ADRs — authoring stays in the CLI and TUI.

## Switching ADR directories

The dashboard starts on the directory `adroit serve` was launched with, but you
can point it at any other directory from the header's directory chip (it opens a
picker that lists subfolders and shows each folder's ADR count). Because a
browser page can't enumerate the local filesystem, the listing is done by the
server, which runs on the user's own machine:

- `GET /api/workspace` — the active ADR directory.
- `GET /api/browse?path=` — subdirectories of `path` (default: the active dir).
- `POST /api/workspace { path }` — switch the active directory. This re-points
  the live-reload watcher and pushes a `change` tick so every open tab re-fetches.

This is a local convenience (the tool already has the user's filesystem access),
not a remote API — there is still no write path into the ADRs themselves.

## How it fits together

```
on-disk ADRs ──▶ Axum JSON API (src/serve/mod.rs) ──▶ this SPA (fetch /api/*)
       │
       └─ notify watcher (src/serve/watch.rs) ─▶ broadcast ─▶ GET /api/events (SSE)
                                                                      │
                                              EventSource ◀───────────┘  (this SPA)
                                                   │
                                                   └─▶ re-fetch current view
```

The SPA is built into `web/dist` and embedded into the binary at compile time
via `rust-embed`, so a single `adroit` binary serves both the API and the UI.

## Auto live-reload

When ADR files change on disk (CLI/TUI/`$EDITOR` edits, or git operations like
`checkout`/`pull`), the dashboard refreshes automatically — no manual reload.

- The server runs one recursive `notify` watcher on the ADR directory, coalesces
  bursty filesystem events with a short (~250ms) debounce, and publishes a tick
  on a broadcast channel.
- `GET /api/events` is a Server-Sent Events stream; each browser tab subscribes
  and receives an `event: change` per coalesced change (plus keep-alive
  comments). `EventSource` reconnects automatically if the stream drops.
- The shared composable `src/useLiveReload.ts` opens the `EventSource`; each
  view (`DashboardView`, `BrowseView`, `DetailView`, `InsightsView`)
  passes a callback that re-fetches just its own data on `change`. A subtle
  "updated" badge and a live/read-only indicator appear in the header.

## Build

From the repo root:

```sh
just web-build      # npm install && npm run build  →  web/dist
just serve          # web-build, then `cargo run --features web -- serve`
```

Or directly in this folder:

```sh
npm install
npm run dev         # Vite dev server (proxy /api to a running `adroit serve`)
npm run build       # production build into web/dist
```

`web/dist` is a build artifact and is not committed (a `.gitkeep` keeps the embed
directory present so the Rust crate compiles without a Vue build). Rebuild it
whenever the SPA changes, then rebuild the binary with `--features web`.

## Design system

Tailwind CSS v4 (`@tailwindcss/vite`), dark-mode-first:

- `src/style.css` — semantic design tokens (`--ad-*`, light + dark), an
  indigo→violet `brand` scale, and the `.card-glass` / `.hero-gradient` /
  `.btn-spring` utilities. Dark mode is class-based (`.dark` on `<html>`).
- Inter is bundled via `@fontsource-variable/inter` (no network dependency, so
  the dashboard renders identically offline).
- Markdown bodies render through `@tailwindcss/typography` (`prose` /
  `prose-invert`); icons come from `lucide-vue-next`.

## Layout

- `src/api.ts` — typed client over the read-only `/api/*` endpoints.
- `src/useLiveReload.ts` — SSE (`/api/events`) live-reload composable.
- `src/composables/` — `useTheme` (tri-state theme toggle, persisted),
  `useCountUp` (animated stat tiles), and `useWorkspace` (active ADR directory +
  switch action).
- `src/components/` — `StatusPill` (theme-aware status badge), `StatTile`,
  `RelationsGraph` (the force-directed wiki-graph), the chart components
  (`DonutChart` / `GrowthChart` / `CohortChart`), `SelectMenu`, and
  `DirectoryPicker` (the workspace-switch modal).
- `src/router.ts` — routes for dashboard / browse / detail / insights.
- `src/views/*` — one component per view; each subscribes to live-reload.
