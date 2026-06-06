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
use crate::naming::{AdrRef, NamingScheme};
use crate::store::{Store, StoreError};
use crate::view::{
    AdrDetail, AdrSummary, CheckReport, CreatedBucket, EdgeKind, Graph, GraphEdge, GraphNode,
    Problem, ProblemFile, ProblemKind, ProposedAge, RelatedLink, Severity, Stats, StatusCount,
    TimelineEvent,
};

/// Errors from the query layer.
#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error(transparent)]
    Store(#[from] StoreError),

    #[error("could not read {0}")]
    Io(String),
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
/// An `ADROIT_TODAY` override (ISO `YYYY-MM-DD`) pins it for tests / CI.
fn today() -> Date {
    if let Some(d) = crate::config::today_override() {
        return d;
    }
    OffsetDateTime::now_local()
        .unwrap_or_else(|_| OffsetDateTime::now_utc())
        .date()
}

/// List ADR summaries, filtered and sorted per `filter`.
pub fn summaries(store: &Store, filter: &Filter) -> Result<Vec<AdrSummary>, QueryError> {
    let resolved = load_resolved(store)?;
    let today = today();
    let overdue = store.options().review_overdue_days;
    let scheme = store.options().naming;
    let mut rows: Vec<AdrSummary> = resolved
        .iter()
        .filter(|r| filter.status.is_none_or(|s| r.adr.status == s))
        .map(|r| summary_of(&r.adr, r.created, today, overdue, scheme))
        .collect();
    sort_summaries(&mut rows, filter.sort);
    Ok(rows)
}

/// Full detail for a single ADR by number.
pub fn detail(store: &Store, number: u32) -> Result<AdrDetail, QueryError> {
    let path = store.find_path_by_number(Number::new(number))?;
    detail_at(store, &path)
}

/// Full detail for a single ADR at a known path — the scheme-agnostic core, so
/// the CLI can resolve a slug/uuid ADR via the naming seam and still get detail.
pub fn detail_at(store: &Store, path: &Path) -> Result<AdrDetail, QueryError> {
    let path = path.to_path_buf();
    let adr = store.read(&path)?;
    let repo = open_history(store);
    let hist = repo
        .as_ref()
        .and_then(|r| r.history(&path, |p| store.dir_status(p)));
    let (created, last_modified, events) =
        resolve_dates(&adr, &path, store.format() == Format::Frontmatter, hist);
    let summary = summary_of(
        &adr,
        created,
        today(),
        store.options().review_overdue_days,
        store.options().naming,
    );
    let related = related_links(&adr, store.options().naming);
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
    let scheme = store.options().naming;
    let rows = resolved
        .iter()
        .filter(|r| {
            let haystack = format!("{} {}", r.adr.title, r.adr.body).to_lowercase();
            haystack.contains(&needle)
        })
        .map(|r| summary_of(&r.adr, r.created, today, overdue, scheme))
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
    let scheme = store.options().naming;
    let overdue = store.options().review_overdue_days;
    let mut proposed_age: Vec<ProposedAge> = resolved
        .iter()
        .filter(|r| r.adr.status == Status::Proposed)
        .map(|r| {
            let rf = r.adr.reference();
            ProposedAge {
                number: r.adr.number.map(Number::get),
                reference: scheme.display(&rf),
                address: rf.addr(),
                title: r.adr.title.clone(),
                age_days: Some((now - r.created).whole_days()),
                review_due: summary_of(&r.adr, r.created, today, overdue, scheme).review_due,
            }
        })
        .collect();
    proposed_age.sort_by_key(|p| std::cmp::Reverse(p.age_days));

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
    let review_due: Vec<AdrSummary> = resolved
        .iter()
        .map(|r| summary_of(&r.adr, r.created, today, overdue, scheme))
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

/// The duplicate-detection / existence key for an ADR identity, so [`check`]
/// groups and looks ADRs up uniformly across naming schemes.
fn ident_key(r: &AdrRef) -> String {
    match r {
        AdrRef::Number(n) => format!("n:{n}"),
        AdrRef::Slug(s) => format!("s:{s}"),
    }
}

/// Line and byte counts for a file, for the duplicate-check size hints. Returns
/// `(0, metadata_len_or_0)` for a file that can't be read as UTF-8 text.
fn file_stats(path: &Path) -> (usize, u64) {
    match std::fs::read_to_string(path) {
        Ok(s) => (s.lines().count(), s.len() as u64),
        Err(_) => (0, std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)),
    }
}

/// Validate the ADR repo, returning a structured [`CheckReport`].
///
/// The shared engine behind `adroit check` and the web dashboard's repo-health
/// panel. Runs the same five checks the CLI always has:
///
/// 1. Status ↔ directory mismatch (by_status, markdown only).
/// 2. Duplicate ADR identifiers (scheme-aware).
/// 3. Unparseable / missing-H1 ADR files.
/// 4. Broken supersession links (referenced ADR doesn't exist).
/// 5. Broken / stale cross-ADR relative links.
///
/// Problems are returned sorted by severity (errors first) then message; the
/// CLI renders `problem.message` verbatim, so its output is unchanged.
pub fn check(store: &Store) -> Result<CheckReport, QueryError> {
    let files = store.list_files()?;
    let mut problems: Vec<Problem> = Vec::new();

    // Group paths by the scheme's identity (to flag duplicates, and to resolve
    // cross-ADR links / supersession refs — works for every naming scheme, not
    // just the numeric ones).
    let mut by_ident: BTreeMap<String, Vec<std::path::PathBuf>> = BTreeMap::new();
    // Group by normalized title for the duplicate-title check (value: the
    // original-case title + the files that share it).
    let mut by_title: BTreeMap<String, (String, Vec<std::path::PathBuf>)> = BTreeMap::new();
    let scheme = store.options().naming;
    let markdown = store.options().format == Format::Markdown;
    // Frontmatter supersession refs (YAML fields, not markdown links), collected
    // for a broken-supersession check mirroring the markdown one below.
    let mut fm_supersession: Vec<(String, Option<AdrRef>, Option<AdrRef>)> = Vec::new();

    for path in &files {
        let rel = path
            .strip_prefix(store.root())
            .unwrap_or(path)
            .display()
            .to_string();

        // (3) Unparseable / missing H1.
        let adr = match store.read(path) {
            Ok(adr) => adr,
            Err(e) => {
                problems.push(Problem {
                    severity: Severity::Error,
                    kind: ProblemKind::Unparseable,
                    label: rel.clone(),
                    summary: format!("failed to parse ({e})"),
                    paths: Vec::new(),
                    message: format!("{rel}: failed to parse ({e})"),
                });
                continue;
            }
        };
        // Group by the scheme's identity for duplicate detection. A numeric ADR
        // with no number, or a file with no parseable identity, is skipped so
        // stray notes don't register as collisions.
        let r = adr.reference();
        let track = matches!(r, AdrRef::Slug(_)) || adr.number.is_some();
        if track {
            by_ident
                .entry(ident_key(&r))
                .or_default()
                .push(path.clone());
        }
        let norm_title = adr.title.trim().to_lowercase();
        if !norm_title.is_empty() {
            by_title
                .entry(norm_title)
                .or_insert_with(|| (adr.title.trim().to_string(), Vec::new()))
                .1
                .push(path.clone());
        }
        if !markdown {
            fm_supersession.push((
                rel.clone(),
                adr.supersedes.clone(),
                adr.superseded_by.clone(),
            ));
        }

        // Markdown-specific checks need the file's raw text and section status.
        if markdown {
            let content = std::fs::read_to_string(path)
                .map_err(|_| QueryError::Io(path.display().to_string()))?;

            // (1) Status ↔ directory mismatch (by_status only). A section with
            // no explicit status word is allowed (directory is source of truth).
            if let Some(dir_status) = store.dir_status(path)
                && let Some(section_status) = crate::format::parse_markdown_section_status(&content)
                && dir_status != section_status
            {
                let num = adr.number.map(|n| format!("ADR-{n} ")).unwrap_or_default();
                // With a number, the ADR ref is the label and the file is a path;
                // otherwise the file path is itself the label.
                let (label, paths) = match adr.number {
                    Some(n) => {
                        let (lines, bytes) = file_stats(path);
                        (
                            format!("ADR-{n}"),
                            vec![ProblemFile {
                                path: rel.clone(),
                                lines,
                                bytes,
                            }],
                        )
                    }
                    None => (rel.clone(), Vec::new()),
                };
                problems.push(Problem {
                    severity: Severity::Error,
                    kind: ProblemKind::StatusDirMismatch,
                    label,
                    summary: format!(
                        "directory says {dir_status} but ## Status says {section_status}"
                    ),
                    paths,
                    message: format!(
                        "{num}({rel}): directory says {dir_status} but ## Status says {section_status}"
                    ),
                });
            }
        }
    }

    // (4) Broken supersession links. Resolved through the naming seam and
    // checked against the full identity set, so forward/backward references in
    // any order — and slug schemes — all work.
    if markdown {
        for path in &files {
            let rel = path
                .strip_prefix(store.root())
                .unwrap_or(path)
                .display()
                .to_string();
            let Ok(content) = std::fs::read_to_string(path) else {
                continue;
            };
            // The file's parent dir is the per_category category (ignored by other
            // schemes), so a same-category supersession link resolves.
            let source_category = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str());
            let (supersedes, superseded_by) = crate::format::parse_markdown_section_supersession(
                &content,
                scheme,
                source_category,
            );
            for (kind, r) in [("Supersedes", supersedes), ("Superseded by", superseded_by)] {
                if let Some(r) = r
                    && !by_ident.contains_key(&ident_key(&r))
                {
                    let disp = scheme.display(&r);
                    problems.push(Problem {
                        severity: Severity::Error,
                        kind: ProblemKind::BrokenSupersession,
                        label: rel.clone(),
                        summary: format!("## Status says {kind} {disp} but no such ADR exists"),
                        paths: Vec::new(),
                        message: format!(
                            "{rel}: ## Status says {kind} {disp} but no such ADR exists"
                        ),
                    });
                }
            }
        }
    }

    // (4b) Broken supersession in the frontmatter profile. There the refs are
    // YAML fields (`supersedes:` / `superseded_by:`), not `## Status` links, so
    // they're checked here against the same identity set — closing the gap that
    // let a renumber strand a frontmatter supersession pointer silently.
    if !markdown {
        for (rel, supersedes, superseded_by) in &fm_supersession {
            for (kind, r) in [("Supersedes", supersedes), ("Superseded by", superseded_by)] {
                if let Some(r) = r
                    && !by_ident.contains_key(&ident_key(r))
                {
                    let disp = scheme.display(r);
                    problems.push(Problem {
                        severity: Severity::Error,
                        kind: ProblemKind::BrokenSupersession,
                        label: rel.clone(),
                        summary: format!("frontmatter says {kind} {disp} but no such ADR exists"),
                        paths: Vec::new(),
                        message: format!(
                            "{rel}: frontmatter says {kind} {disp} but no such ADR exists"
                        ),
                    });
                }
            }
        }
    }

    // (5) Cross-ADR relative links: each must resolve to an existing file, and a
    // link should point at where the ADR it names currently lives. **Scheme-aware**
    // (mirrors `relink`/check #4): the link target is resolved to an `AdrRef` and
    // looked up in the identity set, so date/uuid/per_category links classify
    // correctly. (A numeric-only resolution mis-flagged a *stale* slug-scheme link
    // — an ADR that merely moved — as a *broken* error, which broke the
    // `relink_scope = self/none` heal-on-main flow for those schemes.)
    for path in &files {
        let rel = path
            .strip_prefix(store.root())
            .unwrap_or(path)
            .display()
            .to_string();
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
        // The source file's parent dir is the per_category category (ignored by
        // other schemes), needed to resolve a same-category link.
        let source_category = dir.file_name().and_then(|n| n.to_str());
        for target in crate::links::relative_md_targets(&content) {
            let pathpart = target.split('#').next().unwrap_or(target);
            let resolved = dir.join(pathpart);
            // The ADR this link names, and where it currently lives (unambiguously).
            let link_ref = scheme.ref_in_link_from(target, source_category);
            let canon: Option<&std::path::PathBuf> = link_ref
                .as_ref()
                .and_then(|r| by_ident.get(&ident_key(r)))
                .filter(|paths| paths.len() == 1)
                .map(|paths| &paths[0]);
            // Message label: keep `ADR-N` (un-padded) for numeric schemes so the
            // output stays byte-identical; the scheme display otherwise.
            let disp = link_ref.as_ref().map(|r| match r.as_number() {
                Some(n) => format!("ADR-{n}"),
                None => scheme.display(r),
            });

            if !resolved.exists() {
                // The literal target is missing. If the link names an ADR that
                // still exists elsewhere in the repo, it's a STALE link a
                // `relink` will heal (a warning — so a deferred-relink PR branch,
                // whose inbound links haven't been canonicalized yet, still
                // passes `check`). A link that names no existing ADR is truly
                // BROKEN (an error).
                if let (Some(disp), Some(canon)) = (&disp, canon) {
                    let want = crate::links::rel_link(dir, canon);
                    problems.push(Problem {
                        severity: Severity::Warning,
                        kind: ProblemKind::StaleLink,
                        label: rel.clone(),
                        summary: format!(
                            "stale link [{target}] — {disp} is now [{want}] (run `adroit relink`)"
                        ),
                        paths: Vec::new(),
                        message: format!(
                            "{rel}: stale link [{target}] — {disp} is now [{want}] (run `adroit relink`)"
                        ),
                    });
                } else {
                    problems.push(Problem {
                        severity: Severity::Error,
                        kind: ProblemKind::BrokenLink,
                        label: rel.clone(),
                        summary: format!("broken link [{target}] — target file not found"),
                        paths: Vec::new(),
                        message: format!("{rel}: broken link [{target}] — target file not found"),
                    });
                }
                continue;
            }
            // Resolved file exists: stale only if it isn't the ADR's current home.
            if let (Some(disp), Some(canon)) = (&disp, canon)
                && let (Ok(rp), Ok(cp)) = (
                    std::fs::canonicalize(&resolved),
                    std::fs::canonicalize(canon),
                )
                && rp != cp
            {
                let want = crate::links::rel_link(dir, canon);
                problems.push(Problem {
                    severity: Severity::Warning,
                    kind: ProblemKind::StaleLink,
                    label: rel.clone(),
                    summary: format!(
                        "stale link [{target}] — {disp} is now [{want}] (run `adroit relink`)"
                    ),
                    paths: Vec::new(),
                    message: format!(
                        "{rel}: stale link [{target}] — {disp} is now [{want}] (run `adroit relink`)"
                    ),
                });
            }
        }
    }

    // (2) Duplicate identifiers (scheme-aware). The wording stays "number" for
    // numeric schemes (byte-identical message) and "identifier" otherwise.
    let noun = if scheme.is_numeric() {
        "number"
    } else {
        "identifier"
    };
    for (key, paths) in &by_ident {
        if paths.len() > 1 {
            // Numeric identity → `ADR-NNNN` (from the key, so the message is
            // byte-identical); slug identity → the scheme's display string.
            let disp = if let Some(num) = key.strip_prefix("n:") {
                format!("ADR-{:04}", num.parse::<u32>().unwrap_or(0))
            } else {
                scheme
                    .parse(&paths[0], "")
                    .map(|r| scheme.display(&r))
                    .unwrap_or_else(|| key.trim_start_matches("s:").to_string())
            };
            let files: Vec<ProblemFile> = paths
                .iter()
                .map(|p| {
                    let path = p
                        .strip_prefix(store.root())
                        .unwrap_or(p)
                        .display()
                        .to_string();
                    let (lines, bytes) = file_stats(p);
                    ProblemFile { path, lines, bytes }
                })
                .collect();
            let list = files
                .iter()
                .map(|f| f.path.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let message = format!("{disp}: duplicate {noun} used by {list}");
            problems.push(Problem {
                severity: Severity::Error,
                kind: ProblemKind::DuplicateId,
                label: disp,
                summary: format!("duplicate {noun}"),
                paths: files,
                message,
            });
        }
    }

    // (6) Duplicate titles (advisory). Titles may legitimately repeat, so this is
    // a Warning — `check` still exits 0 — but it surfaces the accidental `new`.
    for (title, paths) in by_title.values() {
        if paths.len() > 1 {
            let files: Vec<ProblemFile> = paths
                .iter()
                .map(|p| {
                    let path = p
                        .strip_prefix(store.root())
                        .unwrap_or(p)
                        .display()
                        .to_string();
                    let (lines, bytes) = file_stats(p);
                    ProblemFile { path, lines, bytes }
                })
                .collect();
            let list = files
                .iter()
                .map(|f| f.path.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            problems.push(Problem {
                severity: Severity::Warning,
                kind: ProblemKind::DuplicateTitle,
                label: title.clone(),
                summary: "duplicate title".to_string(),
                paths: files,
                message: format!("duplicate title \"{title}\" used by {list}"),
            });
        }
    }

    problems.sort_by(|a, b| {
        a.severity
            .cmp(&b.severity)
            .then_with(|| a.message.cmp(&b.message))
    });
    Ok(CheckReport {
        checked: files.len(),
        problems,
    })
}

/// The supersession / relationship graph across all ADRs.
///
/// Nodes are every ADR. Edges are derived from `supersedes` / `superseded_by`
/// fields and from markdown links to other ADRs found in each body.
pub fn graph(store: &Store) -> Result<Graph, QueryError> {
    let adrs = store.list()?;
    let scheme = store.options().naming;

    let nodes: Vec<GraphNode> = adrs
        .iter()
        .map(|a| {
            let r = a.reference();
            let addressable = a.number.is_some() || a.slug.is_some();
            GraphNode {
                reference: scheme.display(&r),
                address: addressable.then(|| r.addr()),
                title: a.title.clone(),
                status: a.status,
            }
        })
        .collect();

    let mut edges: Vec<GraphEdge> = Vec::new();
    for a in &adrs {
        let from = scheme.display(&a.reference());
        // Supersession from explicit fields. `from supersedes to`.
        if let Some(r) = &a.supersedes {
            push_unique(
                &mut edges,
                from.clone(),
                scheme.display(r),
                EdgeKind::Supersedes,
            );
        }
        // `superseded_by` means the *other* ADR supersedes this one.
        if let Some(r) = &a.superseded_by {
            push_unique(
                &mut edges,
                scheme.display(r),
                from.clone(),
                EdgeKind::Supersedes,
            );
        }
        // Typed relational links (frontmatter): one directed edge per entry.
        for (targets, kind) in typed_links(a) {
            for r in targets {
                push_unique(&mut edges, from.clone(), scheme.display(r), kind);
            }
        }
        // Markdown links to other ADRs in the body become `Related` edges,
        // unless that pair already has a more specific edge (supersession or a
        // typed link).
        for r in linked_refs(&a.body, scheme) {
            let to = scheme.display(&r);
            if to == from {
                continue;
            }
            if edges
                .iter()
                .any(|e| e.kind != EdgeKind::Related && pair_matches(e, &from, &to))
            {
                continue;
            }
            push_unique(&mut edges, from.clone(), to, EdgeKind::Related);
        }
    }

    Ok(Graph { nodes, edges })
}

/// The typed relational links of an ADR, paired with their edge kind.
fn typed_links(a: &Adr) -> [(&[crate::naming::AdrRef], EdgeKind); 3] {
    [
        (&a.depends_on, EdgeKind::DependsOn),
        (&a.refines, EdgeKind::Refines),
        (&a.relates_to, EdgeKind::RelatesTo),
    ]
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
    scheme: NamingScheme,
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
        reference: scheme.display(&adr.reference()),
        address: adr.reference().addr(),
        title: adr.title.clone(),
        status: adr.status,
        created: created.format(&Rfc3339).ok(),
        supersedes: adr
            .supersedes
            .as_ref()
            .map(|r| scheme.display(r))
            .into_iter()
            .collect(),
        superseded_by: adr.superseded_by.as_ref().map(|r| scheme.display(r)),
        review_due,
        forge_data: None,
    }
}

/// Resolve related links for the detail view from fields + body links.
fn related_links(adr: &Adr, scheme: NamingScheme) -> Vec<RelatedLink> {
    let mut out: Vec<RelatedLink> = Vec::new();
    if let Some(r) = &adr.supersedes {
        push_related(&mut out, scheme, r, EdgeKind::Supersedes);
    }
    if let Some(r) = &adr.superseded_by {
        push_related(&mut out, scheme, r, EdgeKind::Supersedes);
    }
    // Typed relational links (frontmatter).
    for (targets, kind) in typed_links(adr) {
        for r in targets {
            push_related(&mut out, scheme, r, kind);
        }
    }
    let self_ref = adr.reference();
    for r in linked_refs(&adr.body, scheme) {
        if r == self_ref {
            continue;
        }
        let address = r.addr();
        // A plain body link is the weakest edge; skip if a more specific one
        // (supersession or a typed link) already covers this target.
        if out
            .iter()
            .any(|x| x.address == address && x.kind != EdgeKind::Related)
        {
            continue;
        }
        push_related(&mut out, scheme, &r, EdgeKind::Related);
    }
    out
}

/// Push a [`RelatedLink`], skipping exact duplicates (by addressing token).
fn push_related(
    out: &mut Vec<RelatedLink>,
    scheme: NamingScheme,
    r: &crate::naming::AdrRef,
    kind: EdgeKind,
) {
    let address = r.addr();
    if !out.iter().any(|x| x.address == address && x.kind == kind) {
        out.push(RelatedLink {
            reference: scheme.display(r),
            address,
            kind,
        });
    }
}

fn sort_summaries(rows: &mut [AdrSummary], sort: Sort) {
    match sort {
        Sort::NumberAsc => rows.sort_by_key(|a| a.number),
        Sort::NumberDesc => rows.sort_by_key(|a| std::cmp::Reverse(a.number)),
        // `created` is `Option<String>` (not `Copy`); the comparator reverse
        // avoids cloning the key per element.
        Sort::CreatedDesc => rows.sort_by(|a, b| b.created.cmp(&a.created)),
        Sort::TitleAsc => rows.sort_by_key(|a| a.title.to_lowercase()),
    }
}

fn push_unique(edges: &mut Vec<GraphEdge>, from: String, to: String, kind: EdgeKind) {
    if !edges
        .iter()
        .any(|e| e.from == from && e.to == to && e.kind == kind)
    {
        edges.push(GraphEdge { from, to, kind });
    }
}

fn pair_matches(e: &GraphEdge, a: &str, b: &str) -> bool {
    (e.from == a && e.to == b) || (e.from == b && e.to == a)
}

/// Extract the ADR references targeted by markdown links in `body`, resolved
/// through the naming `scheme` (e.g. `[ADR-0006](../accepted/0006-foo.md)` →
/// `Number(6)`, or `[x](20260601-foo.md)` → `Slug(..)`).
fn linked_refs(body: &str, scheme: NamingScheme) -> Vec<crate::naming::AdrRef> {
    let mut out = Vec::new();
    // Scan each "](...)" link target for an ADR reference.
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b']'
            && i + 1 < bytes.len()
            && bytes[i + 1] == b'('
            && let Some(end) = body[i + 2..].find(')')
        {
            let target = &body[i + 2..i + 2 + end];
            if let Some(r) = scheme.ref_in_link(target)
                && !out.contains(&r)
            {
                out.push(r);
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
    fn check_clean_repo_has_no_problems() {
        let tmp = tempfile::tempdir().unwrap();
        let store = md_store(tmp.path());
        seed(&store);
        let report = check(&store).unwrap();
        assert_eq!(report.checked, 3);
        assert!(report.problems.is_empty());
    }

    #[test]
    fn check_flags_duplicate_number_as_error() {
        let tmp = tempfile::tempdir().unwrap();
        let store = md_store(tmp.path());
        // Two ADRs share number 9 across status dirs → a duplicate-id error.
        write_md(
            &store,
            Status::Proposed,
            9,
            "Alpha",
            "# ADR-0009: Alpha\n\n## Status\n\nProposed\n\n## Context\n\nx.\n",
        );
        write_md(
            &store,
            Status::Accepted,
            9,
            "Beta",
            "# ADR-0009: Beta\n\n## Status\n\nAccepted\n\n## Context\n\ny.\n",
        );
        let report = check(&store).unwrap();
        let dups: Vec<_> = report
            .problems
            .iter()
            .filter(|p| p.kind == ProblemKind::DuplicateId)
            .collect();
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].severity, Severity::Error);
        assert_eq!(dups[0].label, "ADR-0009");
        assert_eq!(dups[0].summary, "duplicate number");
        assert_eq!(dups[0].paths.len(), 2);
        // Size hints are populated so the UI can flag a stub vs. a full ADR.
        assert!(dups[0].paths.iter().all(|f| f.lines > 0 && f.bytes > 0));
        // The flat message stays byte-identical for the CLI.
        assert!(dups[0].message.contains("ADR-0009"));
        assert!(dups[0].message.contains("duplicate number used by"));
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
        assert_eq!(d.related[0].reference, "ADR-0001");
        assert_eq!(d.related[0].address, "1");
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
        assert_eq!(supersedes[0].from, "ADR-0002");
        assert_eq!(supersedes[0].to, "ADR-0001");
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
        assert_eq!(supersedes[0].from, "ADR-0006");
        assert_eq!(supersedes[0].to, "ADR-0002");
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
        assert_eq!(related[0].from, "ADR-0003");
        assert_eq!(related[0].to, "ADR-0001");
    }

    #[test]
    fn graph_emits_typed_link_edges() {
        use crate::adr::Adr;
        use crate::naming::AdrRef;
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_or_create_with(
            tmp.path(),
            StoreOptions {
                format: Format::Frontmatter,
                ..StoreOptions::default()
            },
        )
        .unwrap();
        let mut base = Adr::new("Base").unwrap();
        store.write(&mut base).unwrap(); // ADR 1
        let mut dependent = Adr::new("Dependent").unwrap();
        dependent.depends_on = vec![AdrRef::Number(1)];
        store.write(&mut dependent).unwrap(); // ADR 2 depends_on ADR 1

        let g = graph(&store).unwrap();
        let deps: Vec<_> = g
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::DependsOn)
            .collect();
        assert_eq!(deps.len(), 1, "one depends_on edge");
        assert_eq!(deps[0].from, "ADR-0002");
        assert_eq!(deps[0].to, "ADR-0001");
    }

    #[test]
    fn linked_refs_parses_forms() {
        use crate::naming::AdrRef;
        assert_eq!(
            linked_refs(
                "see [x](../accepted/0006-foo.md) and [y](0012-bar.md)",
                NamingScheme::Sequential
            ),
            vec![AdrRef::Number(6), AdrRef::Number(12)]
        );
        assert_eq!(
            linked_refs("no links here", NamingScheme::Sequential),
            Vec::<AdrRef>::new()
        );
        // Date scheme resolves slug targets.
        assert_eq!(
            linked_refs("[x](../accepted/20260601-foo.md)", NamingScheme::Date),
            vec![AdrRef::Slug("20260601-foo".into())]
        );
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

        let seq = NamingScheme::Sequential;
        // Aged past the 30-day threshold -> review-due, no deadline needed.
        assert!(summary_of(&adr, old, today, Some(30), seq).review_due);
        // Age-based flagging disabled (None) and no deadline -> not due.
        assert!(!summary_of(&adr, old, today, None, seq).review_due);
        // A recent proposal is not stale.
        assert!(!summary_of(&adr, recent, today, Some(30), seq).review_due);
        // Non-proposed ADRs never count, however old.
        adr.status = Status::Accepted;
        assert!(!summary_of(&adr, old, today, Some(30), seq).review_due);
    }
}
