//! Read-only local web dashboard (feature `web`).
//!
//! A local Axum server that exposes the shared [`crate::query`] layer as a JSON
//! API under `/api/*` and serves an embedded Vue SPA for everything else.
//!
//! **Read-only:** there are no write endpoints, and this module imports only the
//! read side — `query::*` plus `Store::open_or_create_with`/`list`/`read`. It
//! never references `set_status`/`supersede`/`set_body`/`write`.
//!
//! Markdown→HTML rendering happens here (server-side, via `pulldown-cmark`) so
//! the SPA receives ready-to-display HTML in `AdrDetail.body_html`. The store is
//! reopened per request, so the dashboard always reflects current on-disk state.
//!
//! **Auto live-reload:** a single recursive [`notify`] filesystem watcher (see
//! [`watch`]) observes the ADR dir and fans coalesced change ticks out over a
//! [`tokio::sync::broadcast`] channel held in [`AppState`]. The `GET
//! /api/events` SSE endpoint subscribes each browser to that channel and emits
//! an `event: change` line per tick (plus keep-alive comments); the SPA
//! re-fetches the current view on receipt. The watcher only observes — there
//! are still no write endpoints.

mod watch;

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::future::IntoFuture;
use std::net::SocketAddr;
use std::path::{Path as FsPath, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{StatusCode, Uri, header},
    response::{
        Html, IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::get,
};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use tokio_stream::{Stream, StreamExt, wrappers::BroadcastStream};

use crate::adr::Status;
use crate::config::Config;
use crate::query::{self, Filter, Sort};
use crate::store::{Store, StoreError, StoreOptions};
use watch::Watcher;

/// Embedded built Vue assets.
///
/// Points at `web/dist`. If the Vue app has not been built yet the folder is
/// empty and [`Assets::get`] returns `None`, which the SPA fallback turns into a
/// friendly "run `just web-build`" page — so `cargo build --features web` works
/// out of the box with no Vue build present.
#[derive(RustEmbed)]
#[folder = "web/dist/"]
struct Assets;

/// Shared server state: the resolved ADR dir + store options used to (re)open
/// the store on each request, so the dashboard always reflects current on-disk
/// state. The dir is the one resolved by `main.rs` (honoring `--dir`/config).
///
/// `watcher` (when present) owns the running filesystem watcher and the
/// broadcast sender that `/api/events` subscribes to for live-reload. It is
/// `None` only in request tests that don't exercise the watcher.
#[derive(Clone)]
struct AppState {
    // The active ADR directory. Mutable so the dashboard can switch workspaces
    // at runtime (POST /api/workspace); every AppState clone shares the lock.
    dir: Arc<RwLock<PathBuf>>,
    options: Arc<StoreOptions>,
    watcher: Option<Arc<Watcher>>,
    /// Forge config (if any), for the opt-in read-only `/api/adrs/{id}/forge`
    /// enrichment panel. `None` when no provider is configured; enrichment is
    /// also a no-op unless the binary was built with the `forge` feature.
    forge: Option<crate::config::ForgeConfig>,
    /// Required approvals (config `review_quorum`), for the dashboard's
    /// "MR approved · waiting on author" forge tile.
    review_quorum: u32,
}

impl AppState {
    /// Build state without a filesystem watcher (used by request tests).
    #[cfg(test)]
    fn new(dir: PathBuf, cfg: &Config) -> Self {
        Self {
            dir: Arc::new(RwLock::new(dir)),
            options: Arc::new(store_options(cfg)),
            watcher: None,
            forge: cfg.forge.clone(),
            review_quorum: cfg.review_quorum,
        }
    }

    /// Build state with a live-reload watcher on `dir` (used by `run`).
    fn with_watcher(dir: PathBuf, cfg: &Config) -> anyhow::Result<Self> {
        let watcher = watch::spawn(&dir)?;
        Ok(Self {
            dir: Arc::new(RwLock::new(dir)),
            options: Arc::new(store_options(cfg)),
            watcher: Some(Arc::new(watcher)),
            forge: cfg.forge.clone(),
            review_quorum: cfg.review_quorum,
        })
    }

    /// The currently active ADR directory.
    fn active_dir(&self) -> PathBuf {
        self.dir.read().expect("dir lock poisoned").clone()
    }

    /// Open the store read-only, mirroring the binary's wiring (`main.rs`).
    fn store(&self) -> Result<Store, ApiError> {
        open_store(&self.active_dir(), (*self.options).clone()).map_err(ApiError::internal)
    }
}

/// Build [`StoreOptions`] from a [`Config`], mirroring `main.rs`/`tui.rs` so the
/// dashboard opens the store identically to the other surfaces.
fn store_options(cfg: &Config) -> StoreOptions {
    let mut status_dir = BTreeMap::new();
    for status in Status::ALL {
        status_dir.insert(status, cfg.dir_for(status));
    }
    StoreOptions {
        format: cfg.format,
        layout: cfg.layout,
        status_dir,
        review_overdue_days: (cfg.review_overdue_days > 0).then_some(cfg.review_overdue_days),
        date_source: cfg.date_source,
        naming: cfg.naming,
        relink_scope: cfg.relink_scope,
    }
}

/// Open the ADR store at `dir` (read-only use). Creates the dir if missing,
/// matching the rest of the binary's behaviour.
fn open_store(dir: &FsPath, options: StoreOptions) -> Result<Store, StoreError> {
    Store::open_or_create_with(dir, options)
}

/// Errors surfaced by the API, mapped to HTTP status codes.
#[derive(Debug, thiserror::Error)]
enum ApiError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error(transparent)]
    Internal(anyhow::Error),
}

impl ApiError {
    fn internal(e: impl Into<anyhow::Error>) -> Self {
        Self::Internal(e.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            ApiError::NotFound(m) => (StatusCode::NOT_FOUND, m),
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            ApiError::Internal(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        };
        (status, Json(serde_json::json!({ "error": msg }))).into_response()
    }
}

/// Build the Axum [`Router`] for the dashboard around a prepared [`AppState`].
fn router_with_state(state: AppState) -> Router {
    Router::new()
        .route("/api/adrs", get(list_adrs))
        .route("/api/adrs/{id}", get(get_adr))
        .route("/api/adrs/{id}/forge", get(get_adr_forge))
        .route("/api/forge/summary", get(get_forge_summary))
        .route("/api/search", get(search_adrs))
        .route("/api/stats", get(get_stats))
        .route("/api/graph", get(get_graph))
        .route("/api/check", get(get_check))
        .route("/api/workspace", get(get_workspace).post(switch_workspace))
        .route("/api/browse", get(browse_dir))
        .route("/api/events", get(events))
        .fallback(static_handler)
        .with_state(state)
}

/// Build the Axum [`Router`] for the dashboard, serving ADRs from `dir` without
/// a filesystem watcher (used by request tests). `/api/events` is still routed
/// but yields an empty stream.
#[cfg(test)]
fn router(dir: PathBuf, cfg: &Config) -> Router {
    router_with_state(AppState::new(dir, cfg))
}

/// Run the web dashboard against the resolved ADR `dir`, blocking until shutdown
/// (Ctrl-C). `dir` is resolved by `main.rs` (honoring `--dir`/config).
pub fn run(config: &Config, dir: &FsPath, host: &str, port: u16) -> anyhow::Result<()> {
    let options = store_options(config);
    // Validate the store up front so a bad `--dir` fails fast with a clear error
    // rather than 500ing on the first request.
    open_store(dir, options)?;

    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid host/port {host}:{port}: {e}"))?;

    let dir = dir.to_path_buf();
    let cfg = config.clone();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let local = listener.local_addr()?;
        // Start the filesystem watcher (live-reload) before serving, so the
        // very first SSE connection already has a channel to subscribe to.
        let state = AppState::with_watcher(dir.clone(), &cfg)?;
        println!("adroit dashboard serving http://{local} (read-only, live-reload)");
        println!("ADR directory: {}", dir.display());
        println!("Press Ctrl-C to stop.");
        let app = router_with_state(state);
        // Race the server against Ctrl-C and return the moment it fires.
        // We deliberately avoid `with_graceful_shutdown`: it waits for all
        // in-flight connections to drain, but the `/api/events` SSE streams are
        // long-lived and never close on their own, so a graceful drain hangs
        // until every browser tab is closed. Dropping open connections on exit
        // is fine for a local, read-only dashboard.
        tokio::select! {
            res = axum::serve(listener, app).into_future() => res?,
            _ = shutdown_signal() => println!("Shutting down."),
        }
        Ok::<_, anyhow::Error>(())
    })
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

// ---- API handlers (thin wrappers over query::*) ----

/// Query params for `GET /api/adrs`.
#[derive(Debug, Deserialize)]
struct ListParams {
    status: Option<String>,
    sort: Option<String>,
}

/// `GET /api/adrs?status=&sort=` → `Vec<AdrSummary>`.
async fn list_adrs(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Result<impl IntoResponse, ApiError> {
    let store = state.store()?;
    let status = parse_status(params.status.as_deref())?;
    let sort = parse_sort(params.sort.as_deref())?;
    let filter = Filter { status, sort };
    let items = query::summaries(&store, &filter).map_err(ApiError::internal)?;
    Ok(Json(items))
}

/// `GET /api/adrs/{id}` → `AdrDetail` with `body_html` filled. `id` is the
/// scheme addressing token (number / slug / uuid prefix / `category/NNNN`).
async fn get_adr(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let store = state.store()?;
    // Resolve the addressing token through the configured naming scheme, so
    // date/uuid ADRs (which have no number) are reachable too.
    let r = store
        .options()
        .naming
        .parse_ref(&id)
        .ok_or_else(|| ApiError::NotFound(format!("ADR {id} not found")))?;
    let path = store
        .find_path_by_ref(&r)
        .map_err(|_| ApiError::NotFound(format!("ADR {id} not found")))?;
    let mut detail = query::detail_at(&store, &path).map_err(ApiError::internal)?;
    // Server-side markdown rendering (web-only concern). Cross-links are exposed
    // structurally via `detail.related` for SPA navigation; rewriting in-body
    // relative links is deferred (noted in the README / report).
    detail.body_html = Some(render_markdown(&detail.body));
    Ok(Json(detail))
}

/// `GET /api/adrs/{id}/forge` → live forge state for one ADR (issue/PR links +
/// PR approvals/CI/merged) as a `ForgeData`, or JSON `null` when the build has
/// no `forge` feature, the repo configures no provider, or the ADR has no
/// linked issue/PR. **Read-only** — it only *reads* forge state (the panel
/// never writes; authoring stays in the CLI). The (blocking) forge HTTP call
/// runs on a blocking thread so it never stalls the async reactor.
async fn get_adr_forge(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let forge_data = tokio::task::spawn_blocking(move || forge_state_for(&state, &id))
        .await
        .map_err(ApiError::internal)??;
    Ok(Json(forge_data))
}

/// Resolve `id` to an ADR and enrich its summary with live forge state. Blocking
/// (filesystem + a forge HTTP call); call from `spawn_blocking`.
fn forge_state_for(state: &AppState, id: &str) -> Result<Option<crate::view::ForgeData>, ApiError> {
    let store = state.store()?;
    let r = store
        .options()
        .naming
        .parse_ref(id)
        .ok_or_else(|| ApiError::NotFound(format!("ADR {id} not found")))?;
    let path = store
        .find_path_by_ref(&r)
        .map_err(|_| ApiError::NotFound(format!("ADR {id} not found")))?;
    let mut summary = query::detail_at(&store, &path)
        .map_err(ApiError::internal)?
        .summary;
    crate::forge_hook::enrich_one(state.forge.as_ref(), &store, &mut summary)
        .map_err(ApiError::internal)?;
    Ok(summary.forge_data)
}

/// Dashboard forge tiles. `null` when no provider is configured / built.
#[derive(Debug, Serialize)]
struct ForgeSummary {
    /// Proposed ADRs with no PR recorded in `## References`.
    proposed_without_pr: u32,
    /// PRs with the required approvals that haven't been merged yet.
    approved_unmerged: u32,
}

/// `GET /api/forge/summary` → aggregate counts for the dashboard's forge tiles,
/// or JSON `null` when there's no active forge. **Read-only**; the (blocking)
/// live PR reads run on a blocking thread.
async fn get_forge_summary(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    let summary = tokio::task::spawn_blocking(move || forge_summary_for(&state))
        .await
        .map_err(ApiError::internal)??;
    Ok(Json(summary))
}

/// Compute the dashboard forge counts. Blocking (filesystem + live PR reads);
/// call from `spawn_blocking`.
fn forge_summary_for(state: &AppState) -> Result<Option<ForgeSummary>, ApiError> {
    let store = state.store()?;
    let summaries = query::summaries(&store, &Filter::default()).map_err(ApiError::internal)?;
    let counts = crate::forge_hook::dashboard_summary(
        state.forge.as_ref(),
        &store,
        &summaries,
        state.review_quorum,
    )
    .map_err(ApiError::internal)?;
    Ok(
        counts.map(|(proposed_without_pr, approved_unmerged)| ForgeSummary {
            proposed_without_pr,
            approved_unmerged,
        }),
    )
}

/// `GET /api/search?q=` → `Vec<AdrSummary>`.
async fn search_adrs(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<impl IntoResponse, ApiError> {
    let store = state.store()?;
    let items = query::search(&store, &params.q).map_err(ApiError::internal)?;
    Ok(Json(items))
}

/// Query params for `GET /api/search`.
#[derive(Debug, Deserialize)]
struct SearchParams {
    #[serde(default)]
    q: String,
}

/// `GET /api/stats` → `Stats`.
async fn get_stats(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    let store = state.store()?;
    let stats = query::stats(&store).map_err(ApiError::internal)?;
    Ok(Json(stats))
}

/// `GET /api/graph` → `Graph`.
async fn get_graph(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    let store = state.store()?;
    let graph = query::graph(&store).map_err(ApiError::internal)?;
    Ok(Json(graph))
}

/// `GET /api/check` → `CheckReport`: structured repo validation (mirrors
/// `adroit check`), powering the dashboard's repo-health panel. Read-only.
async fn get_check(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    let store = state.store()?;
    let report = query::check(&store).map_err(ApiError::internal)?;
    Ok(Json(report))
}

// ---- workspace / directory browsing ----
//
// The dashboard runs on the user's own machine against their own filesystem, so
// these endpoints simply read local directories and switch which one is active.
// They are deliberately not a hardened remote API — there is still no write path
// into the ADRs themselves.

/// A subdirectory entry for the front-end directory picker.
#[derive(Serialize)]
struct BrowseEntry {
    name: String,
    path: String,
}

/// A directory listing returned to the picker: the directory itself, its parent
/// (for an "up" control), its subdirectories, and how many ADRs it holds.
#[derive(Serialize)]
struct BrowseListing {
    path: String,
    parent: Option<String>,
    entries: Vec<BrowseEntry>,
    adr_count: usize,
}

/// Query params for `GET /api/browse`.
#[derive(Debug, Deserialize)]
struct BrowseParams {
    path: Option<String>,
}

/// Request body for `POST /api/workspace`.
#[derive(Debug, Deserialize)]
struct SwitchBody {
    path: String,
}

/// `GET /api/workspace` → the active ADR directory.
async fn get_workspace(State(state): State<AppState>) -> impl IntoResponse {
    Json(serde_json::json!({ "dir": state.active_dir().display().to_string() }))
}

/// `GET /api/browse?path=` → the subdirectories of `path` (default: the active
/// dir) plus that directory's ADR count, powering the directory picker.
async fn browse_dir(
    State(state): State<AppState>,
    Query(params): Query<BrowseParams>,
) -> Result<impl IntoResponse, ApiError> {
    let requested = match params.path.as_deref().map(str::trim) {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => state.active_dir(),
    };
    let dir = require_dir(&requested)?;

    let mut entries = Vec::new();
    let read = std::fs::read_dir(&dir)
        .map_err(|e| ApiError::BadRequest(format!("cannot read {}: {e}", dir.display())))?;
    for entry in read.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue; // hide dot-directories to keep the picker focused
        }
        entries.push(BrowseEntry {
            name,
            path: entry.path().display().to_string(),
        });
    }
    entries.sort_by_key(|e| e.name.to_lowercase());

    Ok(Json(BrowseListing {
        parent: dir.parent().map(|p| p.display().to_string()),
        adr_count: adr_count_at(&dir, &state.options),
        path: dir.display().to_string(),
        entries,
    }))
}

/// `POST /api/workspace { path }` → switch the active ADR directory, re-point
/// the live-reload watcher at it, and nudge open tabs to re-fetch.
async fn switch_workspace(
    State(state): State<AppState>,
    Json(body): Json<SwitchBody>,
) -> Result<impl IntoResponse, ApiError> {
    let dir = require_dir(&PathBuf::from(body.path.trim()))?;
    *state.dir.write().expect("dir lock poisoned") = dir.clone();
    if let Some(watcher) = &state.watcher {
        // Best-effort: a failed re-watch must not fail the switch itself —
        // live-reload simply won't track the new dir until a later success.
        let _ = watcher.retarget(&dir);
        watcher.notify_now();
    }
    Ok(Json(serde_json::json!({
        "dir": dir.display().to_string(),
        "adr_count": adr_count_at(&dir, &state.options),
    })))
}

/// Canonicalize a requested path and require it to be an existing directory.
fn require_dir(path: &FsPath) -> Result<PathBuf, ApiError> {
    let canonical = std::fs::canonicalize(path)
        .map_err(|e| ApiError::BadRequest(format!("cannot open {}: {e}", path.display())))?;
    if !canonical.is_dir() {
        return Err(ApiError::BadRequest(format!(
            "not a directory: {}",
            canonical.display()
        )));
    }
    Ok(canonical)
}

/// Best-effort ADR count for `dir` (read-only; 0 when it can't be opened).
fn adr_count_at(dir: &FsPath, options: &StoreOptions) -> usize {
    open_store(dir, options.clone())
        .ok()
        .and_then(|store| query::summaries(&store, &Filter::default()).ok())
        .map(|rows| rows.len())
        .unwrap_or(0)
}

/// `GET /api/events` → Server-Sent Events stream of live-reload ticks.
///
/// Each connection subscribes to the broadcast channel fed by the filesystem
/// watcher and forwards one `event: change` per coalesced filesystem change.
/// Periodic keep-alive comments hold the connection open through proxies; the
/// browser's native `EventSource` auto-reconnects if the stream drops. When the
/// server has no watcher (request tests) this yields an empty (keep-alive only)
/// stream so the endpoint still responds with the SSE content type.
async fn events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // Box so both arms (watcher / no-watcher) have the same stream type.
    let stream: std::pin::Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>> =
        match state.watcher {
            Some(watcher) => {
                let rx = watcher.subscribe();
                // `BroadcastStream` yields `Err(Lagged)` if a client falls
                // behind; either way the right action is "re-fetch", so we map
                // every item (Ok tick or Lagged) to a single `change` event.
                Box::pin(BroadcastStream::new(rx).map(|_| Ok(Event::default().event("change"))))
            }
            None => Box::pin(tokio_stream::empty()),
        };

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

// ---- markdown + param helpers ----

/// Render an ADR markdown body to HTML.
fn render_markdown(md: &str) -> String {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, html};
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    // CommonMark only autolinks angle-bracketed `<url>`, so bare URLs (e.g. the
    // `## References` issue/PR links) render as plain text. Walk the event stream
    // and turn bare `http(s)://` URLs in text into links — skipping code blocks
    // and the inside of existing links so we never double-wrap or touch code.
    let mut events: Vec<Event> = Vec::new();
    let mut in_code = 0u32;
    let mut in_link = 0u32;
    for ev in Parser::new_ext(md, options) {
        match &ev {
            Event::Start(Tag::CodeBlock(_)) => in_code += 1,
            Event::End(TagEnd::CodeBlock) => in_code = in_code.saturating_sub(1),
            Event::Start(Tag::Link { .. }) => in_link += 1,
            Event::End(TagEnd::Link) => in_link = in_link.saturating_sub(1),
            Event::Text(t) if in_code == 0 && in_link == 0 => {
                push_autolinked(&mut events, t);
                continue;
            }
            _ => {}
        }
        events.push(ev);
    }

    let mut out = String::new();
    html::push_html(&mut out, events.into_iter());
    out
}

/// Split a text run into plain-text + autolink events for bare `http(s)://`
/// URLs. Text with no URL is pushed unchanged.
fn push_autolinked<'a>(
    events: &mut Vec<pulldown_cmark::Event<'a>>,
    text: &pulldown_cmark::CowStr<'a>,
) {
    use pulldown_cmark::{CowStr, Event, LinkType, Tag, TagEnd};
    let s: &str = text;
    let mut last = 0;
    let mut i = 0;
    while i < s.len() {
        let Some((rel_start, rel_end)) = next_url(&s[i..]) else {
            break;
        };
        let (start, end) = (i + rel_start, i + rel_end);
        if start > last {
            events.push(Event::Text(CowStr::from(s[last..start].to_string())));
        }
        let url = s[start..end].to_string();
        events.push(Event::Start(Tag::Link {
            link_type: LinkType::Inline,
            dest_url: CowStr::from(url.clone()),
            title: CowStr::from(""),
            id: CowStr::from(""),
        }));
        events.push(Event::Text(CowStr::from(url)));
        events.push(Event::End(TagEnd::Link));
        last = end;
        i = end;
    }
    if last < s.len() {
        events.push(Event::Text(CowStr::from(s[last..].to_string())));
    }
}

/// Byte span `(start, end)` of the first bare `http(s)://` URL in `s`, with
/// trailing sentence punctuation / closing brackets trimmed off.
fn next_url(s: &str) -> Option<(usize, usize)> {
    let http = s.find("http://");
    let https = s.find("https://");
    let start = match (http, https) {
        (Some(a), Some(b)) => a.min(b),
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => return None,
    };
    let bytes = s.as_bytes();
    let mut end = start;
    while end < s.len()
        && !bytes[end].is_ascii_whitespace()
        && !matches!(bytes[end], b'<' | b'>' | b'"' | b'`')
    {
        end += 1;
    }
    while end > start
        && matches!(
            bytes[end - 1],
            b'.' | b',' | b';' | b':' | b'!' | b'?' | b')' | b']' | b'}' | b'\'' | b'"'
        )
    {
        end -= 1;
    }
    Some((start, end))
}

/// Map a `?status=` string to `Option<Status>`, rejecting unknown values (400).
/// Empty / missing means "all statuses". Parsing is case-insensitive (strum).
fn parse_status(s: Option<&str>) -> Result<Option<Status>, ApiError> {
    match s.map(str::trim) {
        None | Some("") => Ok(None),
        Some(raw) => Status::from_str(raw)
            .map(Some)
            .map_err(|_| ApiError::BadRequest(format!("unknown status: {raw}"))),
    }
}

/// Map a `?sort=` string to [`Sort`], rejecting unknown values (400). Accepts
/// the surface-friendly aliases `number`/`date`/`title` plus the canonical
/// variant names.
fn parse_sort(s: Option<&str>) -> Result<Sort, ApiError> {
    Ok(match s.map(str::trim) {
        None | Some("") | Some("number") | Some("number_asc") => Sort::NumberAsc,
        Some("number_desc") => Sort::NumberDesc,
        Some("date") | Some("created") | Some("created_desc") => Sort::CreatedDesc,
        Some("title") | Some("title_asc") => Sort::TitleAsc,
        Some(other) => return Err(ApiError::BadRequest(format!("unknown sort: {other}"))),
    })
}

// ---- static SPA serving ----

/// Serve embedded SPA assets, falling back to `index.html` for client-side
/// routes (SPA history mode). API routes are matched before this fallback.
async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    if let Some(content) = Assets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return (
            [(header::CONTENT_TYPE, mime.as_ref())],
            content.data.into_owned(),
        )
            .into_response();
    }

    // SPA fallback: serve index.html for unknown non-asset paths; if the Vue app
    // hasn't been built, show a friendly hint (the JSON API is still live).
    match Assets::get("index.html") {
        Some(content) => Html(content.data.into_owned()).into_response(),
        None => (StatusCode::OK, Html(MISSING_DIST_HTML.to_string())).into_response(),
    }
}

/// Shown when the Vue app has not been built into `web/dist`.
const MISSING_DIST_HTML: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>adroit dashboard</title>
<style>body{font-family:system-ui,sans-serif;max-width:42rem;margin:4rem auto;padding:0 1rem;line-height:1.5}code{background:#f0f0f0;padding:.1rem .3rem;border-radius:3px}</style>
</head><body>
<h1>adroit dashboard</h1>
<p>The web UI has not been built yet. The JSON API is live at
<code>/api/adrs</code>, <code>/api/stats</code>, <code>/api/graph</code>,
<code>/api/search?q=</code>.</p>
<p>To build the SPA, run <code>just web-build</code> (or <code>cd web &amp;&amp; npm install &amp;&amp; npm run build</code>),
then restart <code>adroit serve</code>.</p>
</body></html>"#;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use std::fs;
    use tempfile::TempDir;
    use tower::ServiceExt;

    /// A markdown / by_status store seeded with two ADRs under a tempdir,
    /// returning the tempdir and the ADR root it created.
    fn seed() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("adrs");
        let accepted = root.join("accepted");
        let proposed = root.join("proposed");
        fs::create_dir_all(&accepted).unwrap();
        fs::create_dir_all(&proposed).unwrap();
        fs::write(
            accepted.join("0001-first.md"),
            "# ADR-0001: First decision\n\n## Status\n\nAccepted\n\n## Context\n\nThis is the **first** ADR with some markdown.\n",
        )
        .unwrap();
        fs::write(
            proposed.join("0002-second.md"),
            "# ADR-0002: Second decision\n\n## Status\n\nProposed\n\n## Context\n\nSupersede the first one. See [ADR-0001](../accepted/0001-first.md).\n",
        )
        .unwrap();
        (tmp, root)
    }

    fn app(root: &FsPath) -> Router {
        router(root.to_path_buf(), &Config::default())
    }

    async fn body_string(resp: Response) -> String {
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    async fn get(root: &FsPath, uri: &str) -> Response {
        app(root)
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn lists_adrs_as_json_array() {
        let (_tmp, root) = seed();
        let resp = get(&root, "/api/adrs").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
        assert!(v.is_array());
        assert_eq!(v.as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn forge_endpoint_returns_null_without_provider() {
        // No `forge:` configured (and the default test build lacks the feature),
        // so the read-only panel endpoint resolves the ADR but reports no state.
        let (_tmp, root) = seed();
        let resp = get(&root, "/api/adrs/1/forge").await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_string(resp).await, "null");
    }

    #[test]
    fn render_markdown_autolinks_bare_urls() {
        let html = render_markdown("See https://example.com/issues/7 for details.");
        assert!(
            html.contains(r#"<a href="https://example.com/issues/7""#),
            "{html}"
        );
        // The trailing period stays as text, not part of the link.
        assert!(html.contains("for details."), "{html}");
    }

    #[test]
    fn render_markdown_leaves_code_and_existing_links_alone() {
        // A URL in a fenced code block is not linkified.
        let code = render_markdown("```\nhttps://example.com/x\n```");
        assert!(!code.contains("<a "), "{code}");
        // An existing markdown link isn't double-wrapped.
        let linked = render_markdown("[label](https://example.com/y)");
        assert_eq!(linked.matches("<a ").count(), 1, "{linked}");
    }

    #[tokio::test]
    async fn forge_summary_is_null_without_provider() {
        let (_tmp, root) = seed();
        let resp = get(&root, "/api/forge/summary").await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_string(resp).await, "null");
    }

    #[tokio::test]
    async fn forge_endpoint_404s_for_unknown_adr() {
        let (_tmp, root) = seed();
        let resp = get(&root, "/api/adrs/999/forge").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn filters_by_status_case_insensitively() {
        let (_tmp, root) = seed();
        let resp = get(&root, "/api/adrs?status=proposed").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert_eq!(v[0]["number"], 2);
    }

    #[tokio::test]
    async fn bad_status_is_400() {
        let (_tmp, root) = seed();
        let resp = get(&root, "/api/adrs?status=bogus").await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn bad_sort_is_400() {
        let (_tmp, root) = seed();
        let resp = get(&root, "/api/adrs?sort=bogus").await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn detail_has_rendered_html() {
        let (_tmp, root) = seed();
        let resp = get(&root, "/api/adrs/1").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
        assert_eq!(v["number"], 1);
        let html = v["body_html"].as_str().unwrap();
        assert!(!html.is_empty());
        assert!(html.contains("<strong>first</strong>"));
        assert!(html.contains("<h2>"));
    }

    #[tokio::test]
    async fn detail_exposes_related_links() {
        let (_tmp, root) = seed();
        let resp = get(&root, "/api/adrs/2").await;
        let v: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
        // ADR 2 links to ADR 1 in its body -> a related edge for navigation.
        // RelatedLink is now scheme-agnostic: `reference` (display) + `address`.
        let related = v["related"].as_array().unwrap();
        assert!(
            related
                .iter()
                .any(|r| r["reference"] == "ADR-0001" && r["address"] == "1")
        );
    }

    #[tokio::test]
    async fn missing_adr_is_404() {
        let (_tmp, root) = seed();
        let resp = get(&root, "/api/adrs/999").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn search_finds_matches() {
        let (_tmp, root) = seed();
        let resp = get(&root, "/api/search?q=supersede").await;
        let v: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert_eq!(v[0]["number"], 2);
    }

    #[tokio::test]
    async fn stats_shape() {
        let (_tmp, root) = seed();
        let resp = get(&root, "/api/stats").await;
        let v: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
        assert_eq!(v["total"], 2);
        assert!(v["by_status"].is_array());
        assert!(v["proposed_age"].is_array());
    }

    #[tokio::test]
    async fn graph_shape() {
        let (_tmp, root) = seed();
        let resp = get(&root, "/api/graph").await;
        let v: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
        assert_eq!(v["nodes"].as_array().unwrap().len(), 2);
        // ADR 2 links to ADR 1 -> at least one edge.
        assert!(!v["edges"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn check_endpoint_reports_clean_repo() {
        let (_tmp, root) = seed();
        let resp = get(&root, "/api/check").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
        // The seed repo (ADR-0001 accepted, ADR-0002 proposed with a valid link)
        // has no problems.
        assert_eq!(v["checked"], 2);
        assert!(v["problems"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn spa_fallback_serves_ok() {
        let (_tmp, root) = seed();
        let resp = get(&root, "/browse").await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn events_responds_with_sse_content_type() {
        let (_tmp, root) = seed();
        // The test router has no watcher; the endpoint must still respond as a
        // text/event-stream so the SSE wiring is exercised without hanging on a
        // never-closing body (we only inspect headers, not the stream).
        let resp = get(&root, "/api/events").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        assert!(
            ct.starts_with("text/event-stream"),
            "expected SSE content-type, got {ct:?}"
        );
    }

    #[tokio::test]
    async fn workspace_reports_active_dir() {
        let (_tmp, root) = seed();
        let resp = get(&root, "/api/workspace").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
        assert_eq!(v["dir"].as_str().unwrap(), root.to_string_lossy());
    }

    #[tokio::test]
    async fn browse_lists_subdirectories() {
        let (tmp, root) = seed();
        // Browse the tempdir that contains the seeded "adrs" store directory.
        let uri = format!("/api/browse?path={}", tmp.path().display());
        let resp = get(&root, &uri).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
        let names: Vec<&str> = v["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["name"].as_str().unwrap())
            .collect();
        assert!(
            names.contains(&"adrs"),
            "expected 'adrs' subdir, got {names:?}"
        );
    }

    #[tokio::test]
    async fn switch_changes_active_dir() {
        // Two independent stores; start on A (2 ADRs), switch to B (1 ADR).
        let (_tmp_a, root_a) = seed();
        let tmp_b = TempDir::new().unwrap();
        let root_b = tmp_b.path().join("adrs");
        let accepted_b = root_b.join("accepted");
        fs::create_dir_all(&accepted_b).unwrap();
        fs::write(
            accepted_b.join("0001-only.md"),
            "# ADR-0001: Only in B\n\n## Status\n\nAccepted\n",
        )
        .unwrap();

        let app = app(&root_a);

        let r = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/adrs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&body_string(r).await).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 2);

        let switch = Request::builder()
            .method("POST")
            .uri("/api/workspace")
            .header("content-type", "application/json")
            .body(Body::from(format!(r#"{{"path":"{}"}}"#, root_b.display())))
            .unwrap();
        let r = app.clone().oneshot(switch).await.unwrap();
        assert_eq!(r.status(), StatusCode::OK);

        // Subsequent reads (sharing the same AppState) reflect B.
        let r = app
            .oneshot(
                Request::builder()
                    .uri("/api/adrs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&body_string(r).await).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert_eq!(v[0]["title"].as_str().unwrap(), "Only in B");
    }

    #[tokio::test]
    async fn switch_rejects_missing_dir() {
        let (_tmp, root) = seed();
        let req = Request::builder()
            .method("POST")
            .uri("/api/workspace")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"path":"/no/such/dir/adroit-xyz-404"}"#))
            .unwrap();
        let resp = app(&root).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
