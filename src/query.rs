//! The shared **read API** over [`Store`]. Builds the serde view types in
//! [`crate::view`] from parsed ADRs, so every surface (CLI, future TUI, future
//! web) derives list/search/stats/graph **once**, identically.
//!
//! This layer never writes; write logic stays in the `Store` write path used by
//! the CLI (and, later, the TUI). It reuses existing `Store` methods
//! (`list`, `read`, `find_path_by_number`, …) and does no file I/O of its own.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime};

use crate::adr::{Adr, Number, Status};
use crate::config::DateSource;
use crate::format::Format;
use crate::history::{self, HistoryEvent};
use crate::store::{Store, StoreError};
use crate::view::{
    AdrDetail, AdrSummary, CreatedBucket, EdgeKind, Graph, GraphEdge, GraphNode, ProposedAge,
    RelatedLink, Stats, StatusCount, TimelineEvent,
};

/// Errors from the query layer.
#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// How to filter and sort a list of [`AdrSummary`].
#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// Only include ADRs with this status. `None` means all statuses.
    pub status: Option<Status>,
    /// Sort order applied to the result.
    pub sort: Sort,
}

/// Sort order for [`summaries`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    /// Ascending by ADR number (the on-disk listing order). The default.
    #[default]
    NumberAsc,
    /// Descending by ADR number (newest first).
    NumberDesc,
    /// Newest creation date first.
    CreatedDesc,
    /// Alphabetical by title (case-insensitive).
    TitleAsc,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Today's date (local, falling back to UTC), used to evaluate review deadlines.
fn today() -> Date {
    OffsetDateTime::now_local()
        .unwrap_or_else(|_| OffsetDateTime::now_utc())
        .date()
}

/// List ADR summaries, filtered and sorted per `filter`.
pub fn summaries(store: &Store, filter: &Filter) -> Result<Vec<AdrSummary>, QueryError> {
    let resolved = load_resolved(store)?;
    let today = today();
    let overdue = store.options().review_overdue_days;
    let mut rows: Vec<AdrSummary> = resolved
        .iter()
        .filter(|r| filter.status.is_none_or(|s| r.adr.status == s))
        .map(|r| summary_of(&r.adr, r.created, today, overdue))
        .collect();
    sort_summaries(&mut rows, filter.sort);
    Ok(rows)
}

/// Full detail for a single ADR by number.
pub fn detail(store: &Store, number: u32) -> Result<AdrDetail, QueryError> {
    let path = store.find_path_by_number(Number::new(number))?;
    let adr = store.read(&path)?;
    let repo = open_history(store);
    let hist = repo
        .as_ref()
        .and_then(|r| r.history(&path, |p| store.dir_status(p)));
    let (created, last_modified, events) =
        resolve_dates(&adr, &path, store.format() == Format::Frontmatter, hist);
    let summary = summary_of(&adr, created, today(), store.options().review_overdue_days);
    let related = related_links(&adr);
    let history: Vec<TimelineEvent> = events.iter().map(timeline_event).collect();
    Ok(AdrDetail {
        summary,
        body: adr.body,
        // TODO(step4): render markdown -> HTML server-side for the web surface.
        body_html: None,
        related,
        history,
        last_modified: last_modified.and_then(|d| d.format(&Rfc3339).ok()),
    })
}

/// Case-insensitive search over title + body. Returns matching summaries in
/// the default (number-ascending) order.
pub fn search(store: &Store, term: &str) -> Result<Vec<AdrSummary>, QueryError> {
    let needle = term.to_lowercase();
    let resolved = load_resolved(store)?;
    let today = today();
    let overdue = store.options().review_overdue_days;
    let rows = resolved
        .iter()
        .filter(|r| {
            let haystack = format!("{} {}", r.adr.title, r.adr.body).to_lowercase();
            haystack.contains(&needle)
        })
        .map(|r| summary_of(&r.adr, r.created, today, overdue))
        .collect();
    Ok(rows)
}

/// Aggregate statistics across all ADRs.
pub fn stats(store: &Store) -> Result<Stats, QueryError> {
    let resolved = load_resolved(store)?;
    let now = OffsetDateTime::now_utc();
    let today = today();

    // Counts per status, every status present (including zeroes) in order.
    let by_status: Vec<StatusCount> = Status::ALL
        .into_iter()
        .map(|status| StatusCount {
            status,
            count: resolved.iter().filter(|r| r.adr.status == status).count(),
        })
        .collect();

    // Age of each still-Proposed ADR (from its git-derived creation), oldest first.
    let mut proposed_age: Vec<ProposedAge> = resolved
        .iter()
        .filter(|r| r.adr.status == Status::Proposed)
        .map(|r| ProposedAge {
            number: r.adr.number.map(Number::get),
            title: r.adr.title.clone(),
            age_days: Some((now - r.created).whole_days()),
        })
        .collect();
    proposed_age.sort_by(|a, b| b.age_days.cmp(&a.age_days));

    // Created-over-time, bucketed by calendar month (YYYY-MM), oldest first.
    let mut months: BTreeMap<String, usize> = BTreeMap::new();
    for r in &resolved {
        let d = r.created;
        let key = format!("{:04}-{:02}", d.year(), u8::from(d.month()));
        *months.entry(key).or_default() += 1;
    }
    let created_over_time: Vec<CreatedBucket> = months
        .into_iter()
        .map(|(month, count)| CreatedBucket { month, count })
        .collect();

    // ADRs flagged review-due: still Proposed and past their `review_by` date,
    // or aged past the configured staleness threshold.
    let overdue = store.options().review_overdue_days;
    let review_due: Vec<AdrSummary> = resolved
        .iter()
        .map(|r| summary_of(&r.adr, r.created, today, overdue))
        .filter(|s| s.review_due)
        .collect();

    Ok(Stats {
        total: resolved.len(),
        by_status,
        proposed_age,
        review_due,
        created_over_time,
    })
}

/// The supersession / relationship graph across all ADRs.
///
/// Nodes are every ADR. Edges are derived from `supersedes` / `superseded_by`
/// fields and from markdown links to other ADRs found in each body.
pub fn graph(store: &Store) -> Result<Graph, QueryError> {
    let adrs = store.list()?;

    let nodes: Vec<GraphNode> = adrs
        .iter()
        .map(|a| GraphNode {
            number: a.number.map(Number::get),
            title: a.title.clone(),
            status: a.status,
        })
        .collect();

    let mut edges: Vec<GraphEdge> = Vec::new();
    for a in &adrs {
        let Some(from) = a.number.map(Number::get) else {
            continue;
        };
        // Supersession from explicit fields. `from supersedes to`.
        if let Some(to) = a.supersedes.map(Number::get) {
            push_unique(&mut edges, from, to, EdgeKind::Supersedes);
        }
        // `superseded_by` means the *other* ADR supersedes this one.
        if let Some(newer) = a.superseded_by.map(Number::get) {
            push_unique(&mut edges, newer, from, EdgeKind::Supersedes);
        }
        // Markdown links to other ADRs in the body become `Related` edges,
        // unless that pair already has a supersession edge.
        for to in linked_numbers(&a.body) {
            if to == from {
                continue;
            }
            if edges
                .iter()
                .any(|e| e.kind == EdgeKind::Supersedes && pair_matches(e, from, to))
            {
                continue;
            }
            push_unique(&mut edges, from, to, EdgeKind::Related);
        }
    }

    Ok(Graph { nodes, edges })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// An ADR paired with its resolved creation date, so every list/stats path
/// reports the same git-derived date. (The full lifecycle/last-modified is only
/// needed by [`detail`], which resolves a single ADR directly.)
struct Resolved {
    adr: Adr,
    /// Best-available creation timestamp (git first-add, else fallback).
    created: OffsetDateTime,
}

/// Warn at most once per process when strict `date_source = git` can't deliver.
static GIT_STRICT_WARNED: AtomicBool = AtomicBool::new(false);

/// Open the git repo for date resolution, honoring the configured
/// [`DateSource`]: `Filesystem` never shells git; `Auto` uses git when present
/// (silent fallback); `Git` is strict — it warns once (then still falls back)
/// when git history is unavailable or the clone is shallow, so a CI
/// misconfiguration is visible rather than silently producing wrong dates.
fn open_history(store: &Store) -> Option<history::GitRepo> {
    let source = store.options().date_source;
    if source == DateSource::Filesystem {
        return None;
    }
    let repo = history::open(store.root());
    if source == DateSource::Git {
        let warning = match &repo {
            None => Some(
                "date_source=git but this isn't a git work tree (or git isn't \
                 installed) — falling back to filesystem dates",
            ),
            Some(r) if r.is_shallow() => Some(
                "date_source=git on a shallow clone — ADR creation dates may be \
                 wrong; fetch full history (e.g. actions/checkout fetch-depth: 0)",
            ),
            Some(_) => None,
        };
        if let Some(msg) = warning
            && !GIT_STRICT_WARNED.swap(true, Ordering::Relaxed)
        {
            eprintln!("adroit: {msg}");
        }
    }
    repo
}

/// Load every ADR and resolve its creation date from git (once per call).
///
/// The git repository is probed a single time; each file's history is then one
/// `git log`. Outside a git repo the per-file lookup returns `None` and the date
/// falls back (see [`resolve_dates`]).
fn load_resolved(store: &Store) -> Result<Vec<Resolved>, QueryError> {
    let repo = open_history(store);
    let is_frontmatter = store.format() == Format::Frontmatter;
    let resolved = store
        .list_with_paths()?
        .into_iter()
        .map(|(path, adr)| {
            let hist = repo
                .as_ref()
                .and_then(|r| r.history(&path, |p| store.dir_status(p)));
            let (created, _, _) = resolve_dates(&adr, &path, is_frontmatter, hist);
            Resolved { adr, created }
        })
        .collect();
    Ok(resolved)
}

/// Resolve an ADR's creation date, last-modified date, and lifecycle from its
/// git history when available, else from non-git sources.
///
/// Precedence for `created`: 1) git first-add date (the real history); 2) for
/// the frontmatter profile, the authored on-disk `created:`; 3) filesystem
/// mtime; 4) the parsed `Adr::created` (the `now()` last resort). A markdown
/// ADR in git therefore always uses git — fixing the "everything shows today"
/// symptom, since a clone resets mtime and markdown persists no date.
fn resolve_dates(
    adr: &Adr,
    path: &Path,
    is_frontmatter: bool,
    hist: Option<history::AdrHistory>,
) -> (OffsetDateTime, Option<OffsetDateTime>, Vec<HistoryEvent>) {
    match hist {
        Some(h) => (h.created, Some(h.last_modified), h.events),
        None => {
            let mtime = file_mtime(path);
            let created = if is_frontmatter {
                adr.created.get()
            } else {
                mtime.unwrap_or_else(|| adr.created.get())
            };
            (created, mtime, Vec::new())
        }
    }
}

/// Filesystem modification time of `path`, if readable.
fn file_mtime(path: &Path) -> Option<OffsetDateTime> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()
        .map(OffsetDateTime::from)
}

/// Map a git [`HistoryEvent`] to the serde [`TimelineEvent`] view type.
fn timeline_event(e: &HistoryEvent) -> TimelineEvent {
    TimelineEvent {
        date: e.date.format(&Rfc3339).unwrap_or_default(),
        status: e.status,
        label: e.status.to_string(),
        commit: e.commit.clone(),
        subject: e.subject.clone(),
    }
}

/// Build an [`AdrSummary`] from a parsed [`Adr`] and its resolved creation
/// date, evaluating review-due against `today`.
///
/// An ADR is **review-due** when it is still `Proposed` and either: it has a
/// `review_by` deadline on or before `today`; or `overdue_days` is set and the
/// ADR has been sitting (since `created`) at least that many days — so an aging
/// backlog surfaces without anyone stamping each ADR with a deadline.
fn summary_of(
    adr: &Adr,
    created: OffsetDateTime,
    today: Date,
    overdue_days: Option<u32>,
) -> AdrSummary {
    let proposed = adr.status == Status::Proposed;
    let past_deadline = adr.review_by.is_some_and(|rb| rb.get() <= today);
    let stale =
        overdue_days.is_some_and(|days| (today - created.date()).whole_days() >= i64::from(days));
    let review_due = proposed && (past_deadline || stale);
    AdrSummary {
        number: adr.number.map(Number::get),
        number_display: adr
            .number
            .map(|n| n.to_string())
            .unwrap_or_else(|| "????".to_string()),
        title: adr.title.clone(),
        status: adr.status,
        created: created.format(&Rfc3339).ok(),
        supersedes: adr.supersedes.map(Number::get).into_iter().collect(),
        superseded_by: adr.superseded_by.map(Number::get),
        review_due,
    }
}

/// Resolve related links for the detail view from fields + body links.
fn related_links(adr: &Adr) -> Vec<RelatedLink> {
    let mut out: Vec<RelatedLink> = Vec::new();
    if let Some(n) = adr.supersedes.map(Number::get) {
        push_related(&mut out, n, EdgeKind::Supersedes);
    }
    if let Some(n) = adr.superseded_by.map(Number::get) {
        push_related(&mut out, n, EdgeKind::Supersedes);
    }
    let self_number = adr.number.map(Number::get);
    for n in linked_numbers(&adr.body) {
        if Some(n) == self_number {
            continue;
        }
        if out
            .iter()
            .any(|r| r.number == n && r.kind == EdgeKind::Supersedes)
        {
            continue;
        }
        push_related(&mut out, n, EdgeKind::Related);
    }
    out
}

/// Push a [`RelatedLink`], skipping exact duplicates.
fn push_related(out: &mut Vec<RelatedLink>, number: u32, kind: EdgeKind) {
    if !out.iter().any(|r| r.number == number && r.kind == kind) {
        out.push(RelatedLink { number, kind });
    }
}

fn sort_summaries(rows: &mut [AdrSummary], sort: Sort) {
    match sort {
        Sort::NumberAsc => rows.sort_by(|a, b| a.number.cmp(&b.number)),
        Sort::NumberDesc => rows.sort_by(|a, b| b.number.cmp(&a.number)),
        Sort::CreatedDesc => rows.sort_by(|a, b| b.created.cmp(&a.created)),
        Sort::TitleAsc => rows.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
    }
}

fn push_unique(edges: &mut Vec<GraphEdge>, from: u32, to: u32, kind: EdgeKind) {
    if !edges
        .iter()
        .any(|e| e.from == from && e.to == to && e.kind == kind)
    {
        edges.push(GraphEdge { from, to, kind });
    }
}

fn pair_matches(e: &GraphEdge, a: u32, b: u32) -> bool {
    (e.from == a && e.to == b) || (e.from == b && e.to == a)
}

/// Extract ADR numbers referenced by markdown links in `body`, e.g.
/// `[ADR-0006](../accepted/0006-foo.md)` or `[link](0012-bar.md)`.
fn linked_numbers(body: &str) -> Vec<u32> {
    let mut out = Vec::new();
    // Scan each "](...)" link target for a leading/embedded ADR number.
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b']'
            && i + 1 < bytes.len()
            && bytes[i + 1] == b'('
            && let Some(end) = body[i + 2..].find(')')
        {
            let target = &body[i + 2..i + 2 + end];
            if let Some(n) = crate::links::number_in_target(target)
                && !out.contains(&n)
            {
                out.push(n);
            }
            i = i + 2 + end + 1;
            continue;
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adr::Status;
    use crate::store::{Store, StoreOptions};
    use std::path::{Path, PathBuf};

    fn md_store(root: &Path) -> Store {
        Store::open_or_create_with(root, StoreOptions::default()).unwrap()
    }

    fn write_md(store: &Store, status: Status, number: u32, title: &str, body: &str) -> PathBuf {
        let dir = store.status_dir(status);
        std::fs::create_dir_all(&dir).unwrap();
        let slug: String = title.to_lowercase().replace(' ', "-");
        let p = dir.join(format!("{number:04}-{slug}.md"));
        std::fs::write(&p, body).unwrap();
        p
    }

    fn seed(store: &Store) {
        write_md(
            store,
            Status::Accepted,
            1,
            "Use Postgres",
            "# ADR-0001: Use Postgres\n\n## Status\n\nAccepted\n\n## Context\n\nWe need a database.\n",
        );
        write_md(
            store,
            Status::Proposed,
            2,
            "Use Redis",
            "# ADR-0002: Use Redis\n\n## Status\n\nProposed\n\n## Context\n\nWe need a cache for sessions.\n",
        );
        write_md(
            store,
            Status::Proposed,
            3,
            "Adopt GraphQL",
            "# ADR-0003: Adopt GraphQL\n\n## Status\n\nProposed\n\n## Context\n\nSee [ADR-0001](../accepted/0001-use-postgres.md).\n",
        );
    }

    #[test]
    fn summaries_returns_all_by_default() {
        let tmp = tempfile::tempdir().unwrap();
        let store = md_store(tmp.path());
        seed(&store);
        let rows = summaries(&store, &Filter::default()).unwrap();
        assert_eq!(rows.len(), 3);
        // Default sort: number ascending.
        assert_eq!(rows[0].number, Some(1));
        assert_eq!(rows[2].number, Some(3));
        assert_eq!(rows[0].number_display, "0001");
    }

    #[test]
    fn summaries_filters_by_status() {
        let tmp = tempfile::tempdir().unwrap();
        let store = md_store(tmp.path());
        seed(&store);
        let filter = Filter {
            status: Some(Status::Proposed),
            ..Default::default()
        };
        let rows = summaries(&store, &filter).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.status == Status::Proposed));
    }

    #[test]
    fn summaries_sort_number_desc() {
        let tmp = tempfile::tempdir().unwrap();
        let store = md_store(tmp.path());
        seed(&store);
        let filter = Filter {
            status: None,
            sort: Sort::NumberDesc,
        };
        let rows = summaries(&store, &filter).unwrap();
        assert_eq!(rows[0].number, Some(3));
        assert_eq!(rows[2].number, Some(1));
    }

    #[test]
    fn summaries_sort_title_asc() {
        let tmp = tempfile::tempdir().unwrap();
        let store = md_store(tmp.path());
        seed(&store);
        let filter = Filter {
            status: None,
            sort: Sort::TitleAsc,
        };
        let rows = summaries(&store, &filter).unwrap();
        assert_eq!(rows[0].title, "Adopt GraphQL");
        assert_eq!(rows[1].title, "Use Postgres");
        assert_eq!(rows[2].title, "Use Redis");
    }

    #[test]
    fn search_is_case_insensitive_over_title_and_body() {
        let tmp = tempfile::tempdir().unwrap();
        let store = md_store(tmp.path());
        seed(&store);

        let by_title = search(&store, "REDIS").unwrap();
        assert_eq!(by_title.len(), 1);
        assert_eq!(by_title[0].title, "Use Redis");

        // "database" appears only in the Postgres body.
        let by_body = search(&store, "database").unwrap();
        assert_eq!(by_body.len(), 1);
        assert_eq!(by_body[0].number, Some(1));

        assert!(search(&store, "nonexistent-term").unwrap().is_empty());
    }

    #[test]
    fn detail_includes_raw_body_and_related_links() {
        let tmp = tempfile::tempdir().unwrap();
        let store = md_store(tmp.path());
        seed(&store);

        let d = detail(&store, 3).unwrap();
        assert_eq!(d.summary.number, Some(3));
        assert!(d.body.contains("See [ADR-0001]"));
        assert!(d.body_html.is_none());
        // ADR-0003 links to ADR-0001 -> a Related edge.
        assert_eq!(d.related.len(), 1);
        assert_eq!(d.related[0].number, 1);
        assert_eq!(d.related[0].kind, EdgeKind::Related);
    }

    #[test]
    fn detail_missing_number_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let store = md_store(tmp.path());
        seed(&store);
        assert!(detail(&store, 99).is_err());
    }

    #[test]
    fn stats_counts_by_status_and_total() {
        let tmp = tempfile::tempdir().unwrap();
        let store = md_store(tmp.path());
        seed(&store);

        let s = stats(&store).unwrap();
        assert_eq!(s.total, 3);
        let count = |status: Status| {
            s.by_status
                .iter()
                .find(|c| c.status == status)
                .map(|c| c.count)
                .unwrap()
        };
        assert_eq!(count(Status::Accepted), 1);
        assert_eq!(count(Status::Proposed), 2);
        assert_eq!(count(Status::Rejected), 0);
        // Two proposed ADRs -> two age rows.
        assert_eq!(s.proposed_age.len(), 2);
        // Best-effort review-due is empty for now.
        assert!(s.review_due.is_empty());
        // Every status is represented in lifecycle order.
        assert_eq!(s.by_status.len(), Status::ALL.len());
        assert_eq!(s.by_status[0].status, Status::Proposed);
    }

    #[test]
    fn graph_derives_supersedes_edge_from_markdown() {
        // A markdown ADR whose `## Status` says "Superseded by [ADR-NNNN]"
        // round-trips `superseded_by` through the parser; the graph then emits
        // a single supersession edge (newer -> older).
        let tmp = tempfile::tempdir().unwrap();
        let store = md_store(tmp.path());
        write_md(
            &store,
            Status::Superseded,
            1,
            "Old decision",
            "# ADR-0001: Old decision\n\n## Status\n\nSuperseded by [ADR-0002](../accepted/0002-new-decision.md)\n",
        );
        write_md(
            &store,
            Status::Accepted,
            2,
            "New decision",
            "# ADR-0002: New decision\n\n## Status\n\nAccepted\n",
        );

        let g = graph(&store).unwrap();
        assert_eq!(g.nodes.len(), 2);
        let supersedes: Vec<_> = g
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Supersedes)
            .collect();
        assert_eq!(supersedes.len(), 1);
        assert_eq!(supersedes[0].from, 2);
        assert_eq!(supersedes[0].to, 1);
    }

    #[test]
    fn graph_emits_forward_supersedes_edge_and_dedupes_reciprocal() {
        // Standard supersession wording: the newer ADR says "Supersedes
        // [ADR-NNNN]" and the older one says "Superseded by [ADR-NNNN]". Both
        // now round-trip through the parser, so the graph sees the relationship
        // from both ends — it must collapse to exactly one (newer -> older) edge.
        let tmp = tempfile::tempdir().unwrap();
        let store = md_store(tmp.path());
        write_md(
            &store,
            Status::Accepted,
            6,
            "Adopt ADRs",
            "# ADR-0006: Adopt ADRs\n\n## Status\n\nAccepted\n\nSupersedes [ADR-0002](../superseded/0002-adopt-adrs.md)\n",
        );
        write_md(
            &store,
            Status::Superseded,
            2,
            "Adopt ADRs",
            "# ADR-0002: Adopt ADRs\n\n## Status\n\nSuperseded by [ADR-0006](../accepted/0006-adopt-adrs.md)\n",
        );

        let g = graph(&store).unwrap();
        let supersedes: Vec<_> = g
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Supersedes)
            .collect();
        assert_eq!(supersedes.len(), 1, "one logical supersession -> one edge");
        assert_eq!(supersedes[0].from, 6);
        assert_eq!(supersedes[0].to, 2);
    }

    #[test]
    fn stats_flags_past_due_proposed_and_excludes_accepted() {
        use crate::adr::ReviewBy;
        let tmp = tempfile::tempdir().unwrap();
        let store = md_store(tmp.path());
        // Proposed + past-due review date -> review_due.
        write_md(
            &store,
            Status::Proposed,
            1,
            "Past due",
            "# ADR-0001: Past due\n\n## Status\n\nProposed\n\nReview by: 2000-01-01\n",
        );
        // Accepted + past date -> NOT review_due (only Proposed counts).
        write_md(
            &store,
            Status::Accepted,
            2,
            "Accepted old",
            "# ADR-0002: Accepted old\n\n## Status\n\nAccepted\n\nReview by: 2000-01-01\n",
        );
        // Proposed + far-future date -> NOT review_due.
        let future = ReviewBy::new(
            time::OffsetDateTime::now_utc()
                .date()
                .saturating_add(time::Duration::days(3650)),
        );
        write_md(
            &store,
            Status::Proposed,
            3,
            "Future",
            &format!("# ADR-0003: Future\n\n## Status\n\nProposed\n\nReview by: {future}\n"),
        );

        let s = stats(&store).unwrap();
        assert_eq!(s.review_due.len(), 1);
        assert_eq!(s.review_due[0].number, Some(1));
    }

    #[test]
    fn graph_derives_related_edges_from_body_links() {
        let tmp = tempfile::tempdir().unwrap();
        let store = md_store(tmp.path());
        seed(&store);
        let g = graph(&store).unwrap();
        // ADR-0003's body links to ADR-0001 -> exactly one Related edge.
        let related: Vec<_> = g
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Related)
            .collect();
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].from, 3);
        assert_eq!(related[0].to, 1);
    }

    #[test]
    fn linked_numbers_parses_forms() {
        assert_eq!(
            linked_numbers("see [x](../accepted/0006-foo.md) and [y](ADR-0012)"),
            vec![6, 12]
        );
        assert_eq!(linked_numbers("no links here"), Vec::<u32>::new());
    }

    #[test]
    fn review_due_flags_stale_proposed_even_without_a_deadline() {
        use crate::adr::Adr;
        use time::{Date, Month};

        let mut adr = Adr::new("Aging proposal").unwrap();
        adr.status = Status::Proposed;
        let today = Date::from_calendar_date(2026, Month::June, 1).unwrap();
        // 40 days old, no `review_by`.
        let old = Date::from_calendar_date(2026, Month::April, 22)
            .unwrap()
            .midnight()
            .assume_utc();
        let recent = Date::from_calendar_date(2026, Month::May, 28)
            .unwrap()
            .midnight()
            .assume_utc();

        // Aged past the 30-day threshold -> review-due, no deadline needed.
        assert!(summary_of(&adr, old, today, Some(30)).review_due);
        // Age-based flagging disabled (None) and no deadline -> not due.
        assert!(!summary_of(&adr, old, today, None).review_due);
        // A recent proposal is not stale.
        assert!(!summary_of(&adr, recent, today, Some(30)).review_due);
        // Non-proposed ADRs never count, however old.
        adr.status = Status::Accepted;
        assert!(!summary_of(&adr, old, today, Some(30)).review_due);
    }
}
