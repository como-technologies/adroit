//! Shared serde view types — the single source of truth for "what a surface
//! can show". Every surface (CLI, future TUI, future web JSON API) consumes
//! these structs, so read/derive logic is written once in [`crate::query`].
//!
//! These are **pure data**: no filesystem, ratatui, or axum types here. They
//! all derive [`Serialize`] so the future web surface can emit them as JSON
//! with zero extra mapping (Step 4). HTML rendering is deliberately *not* done
//! here — that is a web-only concern deferred to Step 4; bodies stay raw.

use serde::Serialize;

use crate::adr::Status;

/// One row in a list / table of ADRs. Enough to render a list line without
/// reading the full body.
#[derive(Debug, Clone, Serialize)]
pub struct AdrSummary {
    /// Numeric ADR number (e.g. `6`). `None` for non-numeric naming schemes
    /// (date/uuid) or an ADR with no number yet.
    pub number: Option<u32>,
    /// Zero-padded display form of the number (e.g. `"0006"`, or `"????"`).
    pub number_display: String,
    /// The naming scheme's canonical display identifier — `"ADR-0006"` for the
    /// sequential scheme, the `YYYYMMDD-slug` for date, `"ADR-<short-uuid>"` for
    /// uuid. The surface-facing identity that works across all schemes.
    pub reference: String,
    /// The canonical **addressing** token — what a URL/CLI passes to reach this
    /// ADR (the bare number for numeric schemes, the slug/uuid for slug schemes).
    /// Surfaces route by this so date/uuid ADRs are reachable too.
    pub address: String,
    /// Short title describing the decision.
    pub title: String,
    /// Current lifecycle status.
    pub status: Status,
    /// Creation timestamp as an RFC 3339 string (`None` if unknown).
    ///
    /// Stored as a string so the contract carries no `time` types and
    /// serializes identically across surfaces.
    pub created: Option<String>,
    /// Display references of older ADRs this record supersedes (e.g.
    /// `["ADR-0002"]` or `["20260601-x"]`).
    pub supersedes: Vec<String>,
    /// Display reference of the newer ADR that supersedes this record, if any.
    pub superseded_by: Option<String>,
    /// "This ADR is due for review": `true` when the ADR is still `Proposed`,
    /// has a `review_by` deadline, and that deadline is on or before today.
    /// Computed by [`crate::query`] from the ADR model's `review_by` field.
    pub review_due: bool,
    /// Live forge state (issue/PR), attached only by the opt-in `--forge`
    /// enrichment; omitted from JSON when absent so the contract is unchanged
    /// for non-forge surfaces.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forge_data: Option<ForgeData>,
}

/// Live forge state for a row, attached by `--forge` enrichment. Always
/// compiled (feature-independent view contract); populated from the `forge`
/// adapters when the feature is built in and enrichment is requested.
#[derive(Debug, Clone, Serialize)]
pub struct ForgeData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_approvals: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_ci: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_merged: Option<bool>,
}

/// Full detail for a single ADR: the summary fields plus the raw markdown body
/// and resolved related links.
#[derive(Debug, Clone, Serialize)]
pub struct AdrDetail {
    /// The list-row summary for this ADR (flattened so JSON callers get the
    /// summary fields at the top level alongside the body).
    #[serde(flatten)]
    pub summary: AdrSummary,
    /// Raw markdown body (everything after the H1 / frontmatter). Not rendered.
    pub body: String,
    /// Rendered HTML body. Always `None` until Step 4 (web) wires up
    /// `pulldown-cmark` server-side. Present in the contract so the web surface
    /// can fill it without a shape change.
    pub body_html: Option<String>,
    /// Other ADRs this one links to, resolved from supersession fields and
    /// markdown links in the body.
    pub related: Vec<RelatedLink>,
    /// Git-derived lifecycle milestones (proposed → accepted / rejected /
    /// superseded …), chronological. Empty outside a git repo or in flat layout.
    pub history: Vec<TimelineEvent>,
    /// Most recent commit date touching this ADR, as an RFC 3339 string
    /// (`None` when the date is unknown — e.g. an untracked file).
    pub last_modified: Option<String>,
}

/// One milestone in an ADR's git-derived lifecycle: the ADR reached `status`
/// on `date` in the commit `commit`.
#[derive(Debug, Clone, Serialize)]
pub struct TimelineEvent {
    /// Commit date as an RFC 3339 string.
    pub date: String,
    /// The status reached at this milestone.
    pub status: Status,
    /// Human label for the milestone (the status name, e.g. "Accepted").
    pub label: String,
    /// Abbreviated commit hash that produced the change.
    pub commit: String,
    /// Commit subject line.
    pub subject: String,
}

/// A resolved link from one ADR to another.
#[derive(Debug, Clone, Serialize)]
pub struct RelatedLink {
    /// The target ADR's display reference (e.g. `"ADR-0006"` or a slug).
    pub reference: String,
    /// The target ADR's addressing token (for routing/links).
    pub address: String,
    /// The kind of relationship.
    pub kind: EdgeKind,
}

/// Aggregate statistics across all ADRs, for a stats dashboard.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Stats {
    /// Total number of ADRs.
    pub total: usize,
    /// Count of ADRs per status (every status present, including zeroes), in
    /// lifecycle order.
    pub by_status: Vec<StatusCount>,
    /// How long each still-`Proposed` ADR has been sitting, oldest first.
    pub proposed_age: Vec<ProposedAge>,
    /// ADRs flagged as due for review (still `Proposed` and past their
    /// `review_by` deadline — see [`AdrSummary::review_due`]).
    pub review_due: Vec<AdrSummary>,
    /// Number of ADRs created per calendar month (`YYYY-MM`), oldest first.
    pub created_over_time: Vec<CreatedBucket>,
}

/// Count of ADRs in a single status.
#[derive(Debug, Clone, Serialize)]
pub struct StatusCount {
    pub status: Status,
    pub count: usize,
}

/// How long a `Proposed` ADR has been waiting.
#[derive(Debug, Clone, Serialize)]
pub struct ProposedAge {
    pub number: Option<u32>,
    /// Display id and routing token (so the surface can link it under any scheme).
    pub reference: String,
    pub address: String,
    pub title: String,
    /// Whole days since creation (best-effort; `None` if the created date is
    /// unknown).
    pub age_days: Option<i64>,
    /// `true` when this still-`Proposed` ADR is also flagged review-due (past its
    /// `review_by` deadline or aged past the staleness threshold) — the same
    /// signal as [`AdrSummary::review_due`], carried here so a surface can flag
    /// the row inline without cross-referencing [`Stats::review_due`].
    pub review_due: bool,
}

/// ADRs created in a given calendar month.
#[derive(Debug, Clone, Serialize)]
pub struct CreatedBucket {
    /// Calendar month as `YYYY-MM`.
    pub month: String,
    pub count: usize,
}

/// The supersession / relationship graph across all ADRs.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Graph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

/// A node in the [`Graph`]: one ADR. Keyed by `reference` (its display id);
/// `address` is the routing token (`None` for an unassigned ADR).
#[derive(Debug, Clone, Serialize)]
pub struct GraphNode {
    pub reference: String,
    pub address: Option<String>,
    pub title: String,
    pub status: Status,
}

/// A directed edge in the [`Graph`], connecting nodes by their `reference`.
#[derive(Debug, Clone, Serialize)]
pub struct GraphEdge {
    /// Source ADR reference.
    pub from: String,
    /// Target ADR reference.
    pub to: String,
    pub kind: EdgeKind,
}

/// A structured repo-validation report — the same checks as `adroit check`,
/// surfaced through the shared query layer so every surface (CLI, web, future
/// TUI) reports identical problems.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CheckReport {
    /// Number of ADR files inspected.
    pub checked: usize,
    /// Every problem found, sorted by severity (errors first) then message.
    /// Empty when the repo is clean.
    pub problems: Vec<Problem>,
}

/// One validation problem found by [`crate::query::check`].
///
/// Carries both a flat `message` (rendered verbatim by the CLI — byte-identical
/// to historical `adroit check` output) and structured fields (`label` /
/// `summary` / `paths`) so a richer surface, like the web repo-health panel, can
/// lay it out instead of printing one line.
#[derive(Debug, Clone, Serialize)]
pub struct Problem {
    /// How serious the problem is.
    pub severity: Severity,
    /// Which category of check produced it (for grouping / filtering).
    pub kind: ProblemKind,
    /// Headline identifier: the ADR reference (`"ADR-0009"`) for a duplicate, or
    /// the offending file's relative path otherwise.
    pub label: String,
    /// Short description with neither the leading `label` nor the path list —
    /// e.g. `"duplicate number"`, `"broken link [..] — target file not found"`.
    pub summary: String,
    /// Affected files (relative to the repo root), each with its size. The
    /// duplicate check lists every colliding file here — the line/byte counts let
    /// a surface flag a header-only stub vs. a full ADR; empty when `label`
    /// already names the single file.
    pub paths: Vec<ProblemFile>,
    /// The full one-line message — byte-identical to the `adroit check` line, so
    /// the CLI renders it verbatim.
    pub message: String,
}

/// One file implicated in a [`Problem`], with its size so a surface can hint at
/// what's worth diffing — e.g. a few-line header-only stub vs. a full ADR.
#[derive(Debug, Clone, Serialize)]
pub struct ProblemFile {
    /// Path relative to the repo root.
    pub path: String,
    /// Line count (`0` if the file can't be read as text).
    pub lines: usize,
    /// Byte size on disk.
    pub bytes: u64,
}

/// Severity of a [`Problem`]. `Error` sorts before `Warning`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// A real defect (duplicate id, status/dir mismatch, unparseable file, a
    /// link to a nonexistent ADR) — `adroit check` exits non-zero when any
    /// error is present.
    Error,
    /// A fixable inconsistency (a stale cross-ADR link `adroit relink` repairs).
    /// `adroit check` reports warnings but does **not** fail on them, so a
    /// deferred-relink PR branch still passes CI.
    Warning,
}

/// The category of a validation [`Problem`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProblemKind {
    /// Two ADR files share one identity (number / slug / uuid).
    DuplicateId,
    /// Two or more ADRs share the same (case-insensitive) title — usually an
    /// accidental re-run of `new`. Advisory (a `Warning`): titles *can* repeat.
    DuplicateTitle,
    /// A markdown ADR's `## Status` disagrees with its directory (by_status).
    StatusDirMismatch,
    /// A `.md` ADR with no parseable `# ADR-NNNN: Title` heading.
    Unparseable,
    /// A `Supersedes` / `Superseded by` note references a nonexistent ADR.
    BrokenSupersession,
    /// A relative `.md` link whose target file is missing and which names no
    /// existing ADR — it points nowhere. (A missing target that *does* name a
    /// known ADR is a [`StaleLink`](ProblemKind::StaleLink) instead.)
    BrokenLink,
    /// A relative `.md` link that points somewhere other than its ADR's current
    /// home — the ADR exists, so `adroit relink` repairs it. Covers both a
    /// wrong-but-present path and a missing path whose ADR lives elsewhere.
    StaleLink,
    /// Forge state disagrees with the ADR (e.g. an accepted ADR whose PR isn't
    /// merged, or a closed issue with no matching status change). Surfaced only
    /// by the opt-in `check --forge`.
    ForgeIntegration,
}

/// The kind of relationship an edge / link represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// `from` supersedes `to` (`from` is the newer decision). Directed.
    Supersedes,
    /// `from` depends on `to` (a typed relational link). Directed.
    DependsOn,
    /// `from` refines / elaborates `to` (a typed relational link). Directed.
    Refines,
    /// `from` is related to `to` (a typed, non-directional relational link).
    RelatesTo,
    /// `from` links to `to` via a markdown link in its body (non-supersession).
    Related,
}
