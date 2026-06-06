//! ADR scaffolding templates for the markdown profile.
//!
//! A template is plain text with `{{heading}}`, `{{number}}`, `{{title}}`,
//! `{{date}}`, and `{{status}}` placeholders. `{{heading}}` is the naming
//! scheme's H1 (`# ADR-NNNN: Title` for numeric schemes, `# Title` for slug
//! schemes); `{{number}}` is the bare identifier. Built-in templates are MADR
//! and the classic Nygard layout. Custom templates may be loaded from an
//! explicit file path, a `templates_dir/<name>.md`, or an `adr-template.md`
//! discovered in the target repo.

use std::path::{Path, PathBuf};

use time::{Date, Weekday};

use crate::adr::{Number, Status};

/// The built-in MADR template.
///
/// Each author-filled section ships with an *italic prompt* (`_…_`) that says
/// what belongs there — instructive, not the usual throwaway "Option A". A
/// section left as nothing but its prompt is what `adroit lint` flags as
/// unfinished (see [`crate::lint`]), so the prompts double as the authoring
/// checklist. AI `--interview`/`draft` replace the prose from
/// `## Context and Problem Statement` onward.
pub const MADR: &str = include_str!("../templates/adr/madr.md");

/// The built-in Nygard template.
pub const NYGARD: &str = include_str!("../templates/adr/nygard.md");

/// The built-in review-kickoff template.
///
/// Generates the "review kickoff" doc the team hand-writes whenever they open
/// an ADR for formal review (mirrors the structure of the real artifact). It is
/// rendered by [`render_kickoff`] rather than [`render`] because it carries its
/// own set of placeholders (dates, quorum, the ADR's own path, etc.).
pub const REVIEW_KICKOFF: &str = include_str!("../templates/review/kickoff.md");

/// Errors raised while resolving or loading a template.
#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    #[error("template not found: {0}")]
    NotFound(String),
    #[error("failed to read template {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// Return the built-in template text for a known name, if any.
pub fn builtin(name: &str) -> Option<&'static str> {
    match name.to_lowercase().as_str() {
        "madr" => Some(MADR),
        "nygard" => Some(NYGARD),
        _ => None,
    }
}

/// Resolve a template's text by name-or-path.
///
/// Resolution order:
/// 1. An existing file at `name_or_path`.
/// 2. `templates_dir/<name>.md` if `templates_dir` is set.
/// 3. A built-in (`madr`, `nygard`).
/// 4. An `adr-template.md` discovered in `repo_dir` (the ADR root).
pub fn resolve(
    name_or_path: &str,
    templates_dir: Option<&Path>,
    repo_dir: &Path,
) -> Result<String, TemplateError> {
    // 1. Explicit path.
    let as_path = Path::new(name_or_path);
    if as_path.is_file() {
        return read(as_path);
    }

    // 2. templates_dir/<name>.md
    if let Some(dir) = templates_dir {
        let candidate = dir.join(format!("{name_or_path}.md"));
        if candidate.is_file() {
            return read(&candidate);
        }
    }

    // 3. Built-in.
    if let Some(text) = builtin(name_or_path) {
        return Ok(text.to_string());
    }

    // 4. Repo-local adr-template.md.
    let repo_template = repo_dir.join("adr-template.md");
    if repo_template.is_file() {
        return read(&repo_template);
    }

    Err(TemplateError::NotFound(name_or_path.to_string()))
}

fn read(path: &Path) -> Result<String, TemplateError> {
    std::fs::read_to_string(path).map_err(|source| TemplateError::Read {
        path: path.to_path_buf(),
        source,
    })
}

/// Render a template into a concrete ADR document under naming `scheme`.
///
/// `{{heading}}` is the scheme's H1 (`# ADR-NNNN: Title` or, for slug schemes,
/// `# Title`); `{{number}}` is the bare identifier (`0009` or the slug).
pub fn render(
    template: &str,
    scheme: crate::naming::NamingScheme,
    r: &crate::naming::AdrRef,
    title: &str,
    status: Status,
    date: &str,
) -> String {
    let bare = match r {
        crate::naming::AdrRef::Number(n) => format!("{n:04}"),
        crate::naming::AdrRef::Slug(s) => s.clone(),
    };
    let rendered = template
        .replace("{{heading}}", &scheme.heading(r, title))
        .replace("{{number}}", &bare)
        .replace("{{title}}", title)
        .replace("{{status}}", &status.to_string())
        .replace("{{date}}", date);
    if rendered.ends_with('\n') {
        rendered
    } else {
        format!("{rendered}\n")
    }
}

/// The set of dates a review kickoff doc needs, all computed from a start date
/// and a business-day review window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewWindow {
    /// First day of the review period (the start date passed in).
    pub start: Date,
    /// Last day of the review period: `start` + `days` business days.
    pub end: Date,
    /// First business day after `end`, when the decision is made.
    pub decision: Date,
}

/// Compute a review window from a start date and a number of business days.
///
/// Weekends (Saturday/Sunday) are skipped. `end` is `days` business days after
/// `start` (so `days = 0` yields `end == start`). `decision` is the next
/// business day strictly after `end`.
pub fn review_window(start: Date, days: u32) -> ReviewWindow {
    let mut end = start;
    for _ in 0..days {
        end = next_business_day(end);
    }
    let decision = next_business_day(end);
    ReviewWindow {
        start,
        end,
        decision,
    }
}

/// Return the first business day strictly after `date` (skipping weekends).
fn next_business_day(date: Date) -> Date {
    let mut next = date.next_day().expect("date is well within Date's range");
    while is_weekend(next) {
        next = next.next_day().expect("date is well within Date's range");
    }
    next
}

fn is_weekend(date: Date) -> bool {
    matches!(date.weekday(), Weekday::Saturday | Weekday::Sunday)
}

/// Format a date as `Weekday Mon DD` (e.g. `Thu May 21`), matching the
/// hand-written review kickoff docs.
pub fn format_review_date(date: Date) -> String {
    format!(
        "{} {} {:02}",
        weekday_abbr(date.weekday()),
        month_abbr(date.month()),
        date.day()
    )
}

fn weekday_abbr(day: Weekday) -> &'static str {
    match day {
        Weekday::Monday => "Mon",
        Weekday::Tuesday => "Tue",
        Weekday::Wednesday => "Wed",
        Weekday::Thursday => "Thu",
        Weekday::Friday => "Fri",
        Weekday::Saturday => "Sat",
        Weekday::Sunday => "Sun",
    }
}

fn month_abbr(month: time::Month) -> &'static str {
    use time::Month::*;
    match month {
        January => "Jan",
        February => "Feb",
        March => "Mar",
        April => "Apr",
        May => "May",
        June => "Jun",
        July => "Jul",
        August => "Aug",
        September => "Sep",
        October => "Oct",
        November => "Nov",
        December => "Dec",
    }
}

/// Parameters for rendering a review kickoff document.
#[derive(Debug, Clone)]
pub struct KickoffParams<'a> {
    pub number: Number,
    pub title: &'a str,
    /// The H1 date (ISO `YYYY-MM-DD`, the review start date).
    pub date: &'a str,
    /// Relative path to the ADR file, used as the link target.
    pub adr_path: &'a str,
    pub window: ReviewWindow,
    pub quorum: u32,
}

/// Render the review-kickoff template into a concrete document.
pub fn render_kickoff(template: &str, params: &KickoffParams<'_>) -> String {
    let rendered = template
        .replace("{{number}}", &params.number.to_string())
        .replace("{{title}}", params.title)
        .replace("{{date}}", params.date)
        .replace("{{adr_path}}", params.adr_path)
        .replace("{{review_start}}", &format_review_date(params.window.start))
        .replace("{{review_end}}", &format_review_date(params.window.end))
        .replace(
            "{{decision_date}}",
            &format_review_date(params.window.decision),
        )
        .replace("{{quorum}}", &params.quorum.to_string());
    if rendered.ends_with('\n') {
        rendered
    } else {
        format!("{rendered}\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::naming::{AdrRef, NamingScheme};

    #[test]
    fn render_fills_placeholders() {
        let out = render(
            MADR,
            NamingScheme::Sequential,
            &AdrRef::Number(7),
            "Use PostgreSQL",
            Status::Proposed,
            "2026-05-30",
        );
        assert!(out.starts_with("# ADR-0007: Use PostgreSQL\n"));
        assert!(out.contains("> State: Proposed"));
        assert!(out.contains("\n## Status\n\nProposed\n"));
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn nygard_has_three_sections() {
        let out = render(
            NYGARD,
            NamingScheme::Sequential,
            &AdrRef::Number(1),
            "T",
            Status::Proposed,
            "2026-01-01",
        );
        assert!(out.contains("## Context"));
        assert!(out.contains("## Decision"));
        assert!(out.contains("## Consequences"));
    }

    #[test]
    fn render_date_scheme_uses_plain_heading() {
        let out = render(
            NYGARD,
            NamingScheme::Date,
            &AdrRef::Slug("20260530-use-postgresql".into()),
            "Use PostgreSQL",
            Status::Proposed,
            "2026-05-30",
        );
        assert!(out.starts_with("# Use PostgreSQL\n"));
        assert!(out.contains("## Status"));
    }

    #[test]
    fn builtin_lookup() {
        assert!(builtin("madr").is_some());
        assert!(builtin("MADR").is_some());
        assert!(builtin("nygard").is_some());
        assert!(builtin("nope").is_none());
    }

    #[test]
    fn resolve_prefers_explicit_path() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("custom.md");
        std::fs::write(&p, "# ADR-{{number}}: {{title}}\n").unwrap();
        let text = resolve(p.to_str().unwrap(), None, tmp.path()).unwrap();
        assert!(text.contains("{{title}}"));
    }

    #[test]
    fn resolve_repo_template_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("adr-template.md"), "REPO TEMPLATE\n").unwrap();
        let text = resolve("does-not-exist", None, tmp.path()).unwrap();
        assert_eq!(text, "REPO TEMPLATE\n");
    }

    #[test]
    fn resolve_templates_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let tdir = tmp.path().join("templates");
        std::fs::create_dir_all(&tdir).unwrap();
        std::fs::write(tdir.join("team.md"), "TEAM\n").unwrap();
        let text = resolve("team", Some(&tdir), tmp.path()).unwrap();
        assert_eq!(text, "TEAM\n");
    }

    // ---- review kickoff ----

    use time::macros::date;

    #[test]
    fn review_window_skips_weekends() {
        // Thu 2026-05-21 + 3 business days -> Tue 2026-05-26 (skips Sat/Sun),
        // decision the next business day Wed 2026-05-27.
        let w = review_window(date!(2026 - 05 - 21), 3);
        assert_eq!(w.start, date!(2026 - 05 - 21));
        assert_eq!(w.end, date!(2026 - 05 - 26));
        assert_eq!(w.decision, date!(2026 - 05 - 27));
    }

    #[test]
    fn review_window_five_days_thu_to_thu() {
        // The real example used a 5-business-day window: Thu -> Thu.
        let w = review_window(date!(2026 - 05 - 21), 5);
        assert_eq!(w.end, date!(2026 - 05 - 28));
        assert_eq!(w.decision, date!(2026 - 05 - 29));
    }

    #[test]
    fn review_window_zero_days() {
        let w = review_window(date!(2026 - 05 - 21), 0);
        assert_eq!(w.end, w.start);
        assert_eq!(w.decision, date!(2026 - 05 - 22));
    }

    #[test]
    fn review_window_friday_start_rolls_over_weekend() {
        // Fri + 1 business day -> Mon; decision Tue.
        let w = review_window(date!(2026 - 05 - 22), 1);
        assert_eq!(w.end, date!(2026 - 05 - 25));
        assert_eq!(w.decision, date!(2026 - 05 - 26));
    }

    #[test]
    fn next_business_day_skips_saturday() {
        // Fri -> Mon.
        assert_eq!(
            next_business_day(date!(2026 - 05 - 22)),
            date!(2026 - 05 - 25)
        );
    }

    #[test]
    fn format_review_date_matches_artifact() {
        assert_eq!(format_review_date(date!(2026 - 05 - 21)), "Thu May 21");
        assert_eq!(format_review_date(date!(2026 - 05 - 28)), "Thu May 28");
        assert_eq!(format_review_date(date!(2026 - 01 - 05)), "Mon Jan 05");
    }

    #[test]
    fn render_kickoff_fills_placeholders() {
        let window = review_window(date!(2026 - 05 - 21), 3);
        let params = KickoffParams {
            number: Number::new(15),
            title: "Cluster Templates",
            date: "2026-05-21",
            adr_path: "../proposed/0015-cluster-templates.md",
            window,
            quorum: 3,
        };
        let out = render_kickoff(REVIEW_KICKOFF, &params);
        assert!(out.starts_with("# 2026-05-21 — ADR-0015 Review Kickoff\n"));
        assert!(out.contains("ADR-0015 (Cluster Templates)"));
        assert!(out.contains("## What you're being asked to do"));
        assert!(out.contains("[TODO: one-paragraph decision summary]"));
        assert!(out.contains("[Read the ADR](../proposed/0015-cluster-templates.md)"));
        assert!(out.contains("[Read the README](../README.md)"));
        assert!(out.contains("[Read the guide](../../guides/adr-review-process.md)"));
        assert!(out.contains("**Review period:** Thu May 21 – Tue May 26"));
        assert!(out.contains("**Quorum:** 3 team members must approve"));
        assert!(out.contains("### What happens on Wed May 27"));
        assert!(out.contains("<summary>What the MR changes</summary>"));
        assert!(out.contains("[TODO: tracking issue]"));
        assert!(out.contains("[TODO: decider] decides"));
        assert!(out.ends_with('\n'));
    }
}
