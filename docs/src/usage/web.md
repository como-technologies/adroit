# Web Dashboard

adroit ships a **read-only** web dashboard for exploring an ADR repo in the
browser: browse and read ADRs (with cross-links), full-text search, a stats
dashboard, an interactive **relationship graph**, and a **repo-health panel**
that surfaces the same problems as [`adroit check`](../reference/cli.md#adroit-check)
(duplicate identifiers, status/dir mismatches, broken or stale links). Authoring
stays in the CLI and TUI — the web surface never writes.

The **Insights** page renders a force-directed "wiki-graph" of the repo: each
ADR is a status-colored node, and relationships are colored edges — supersession,
the typed links (`depends_on` / `refines` / `relates_to`), and plain body links.
Drag nodes to arrange, scroll to zoom/pan, toggle edge kinds in the legend, and
click a node to open that ADR.

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

## Switching ADR directories

The dashboard starts on the directory `adroit serve` was launched with, but you
can repoint it at another ADR directory **without restarting the server**. The
header shows the active directory as a chip; click it to open a directory picker
that lists subfolders (with each folder's ADR count) and an "up" control, then
switch. Because a browser page can't read your local filesystem, the listing and
switch are performed by the server (which runs on your own machine):

- `GET /api/workspace` — the active ADR directory.
- `GET /api/browse?path=` — the subdirectories of `path` (default: the active
  dir), each with its ADR count.
- `POST /api/workspace { path }` — switch the active directory. This re-points the
  live-reload watcher at the new directory and pushes a `change` tick, so every
  open tab re-fetches the new repo automatically.

This is a local convenience — the tool already has your filesystem access — not a
hardened remote API. It stays read-only with respect to the ADRs: nothing here
writes to a decision record.

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
| `GET /api/adrs/{id}` | one ADR (addressed by number / slug / uuid prefix / `category/NNNN`) with rendered HTML body and links |
| `GET /api/search?q=` | summaries matching a full-text query |
| `GET /api/stats` | counts by status, ages, review-due, created-over-time |
| `GET /api/graph` | nodes + typed edges for the relationship graph |
| `GET /api/check` | repo-validation report (the same checks as `adroit check`) |
| `GET /api/workspace` | the active ADR directory |
| `POST /api/workspace` | switch the active ADR directory (body `{ "path": … }`) |
| `GET /api/browse?path=` | subdirectories of `path` + ADR count (powers the directory picker) |
| `GET /api/events` | SSE stream of live-reload change events |
