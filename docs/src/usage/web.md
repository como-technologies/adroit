# Web Dashboard

adroit ships a **read-only** web dashboard for exploring an ADR repo in the
browser: browse and read ADRs (with cross-links), full-text search, a stats
dashboard, and a supersession graph. Authoring stays in the CLI and TUI — the
web surface never writes.

The dashboard is built behind the `web` Cargo feature.

## Running it

```sh
just serve            # build the SPA, then serve with live-reload (port 8080)
```

Or manually:

```sh
just web-build        # build the Vue SPA into web/dist (npm install && npm run build)
cargo run --features web -- serve --dir /path/to/repo/src/adrs
```

Options:

- `--host <addr>` — interface to bind (default `127.0.0.1`, loopback only).
- `--port <n>` — port to listen on (default `8080`).
- `--dir`, `--format`, `--layout` — resolved the same way as every other
  command, so the dashboard opens your repo identically to the CLI/TUI.

Open the printed `http://127.0.0.1:8080` URL. The store is reopened on each
request, so every response reflects the current on-disk state.

## Auto live-reload

You never need to refresh manually. When ADR files change on disk — because you
edited one via the CLI, the TUI, or `$EDITOR`, or ran a git operation like
`checkout` or `pull` — the open dashboard updates automatically.

Under the hood, the server runs a filesystem watcher on the ADR directory and
pushes a change signal to the browser over a Server-Sent Events stream
(`/api/events`); the page then re-fetches the data for the view you're looking
at. Bursts of filesystem events (a single save can emit several) are coalesced
so the dashboard isn't flooded, and the browser reconnects automatically if the
connection drops. A small "live" indicator (and a brief "updated" flash) appear
in the header.

## JSON API

The dashboard is a thin client over a read-only JSON API, which you can also use
directly:

| Endpoint | Returns |
|---|---|
| `GET /api/adrs?status=&sort=` | list of ADR summaries |
| `GET /api/adrs/:number` | one ADR with rendered HTML body and links |
| `GET /api/search?q=` | summaries matching a full-text query |
| `GET /api/stats` | counts by status, ages, review-due, created-over-time |
| `GET /api/graph` | nodes + edges for the supersession graph |
| `GET /api/events` | SSE stream of live-reload change events |
