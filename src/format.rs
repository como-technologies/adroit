//! Pluggable on-disk ADR format profiles.
//!
//! adroit supports two serialization profiles:
//!
//! - [`Format::Frontmatter`] — the original YAML-frontmatter + body layout.
//!   See [`crate::frontmatter`].
//! - [`Format::Markdown`] — the MADR-style status-by-directory format where the number
//!   and title live in the H1 (`# ADR-NNNN: Title`) and the status lives in a
//!   `## Status` section (plus an optional `> State:` banner). The status is
//!   *also* derivable from the directory the file lives in.
//!
//! Markdown writes are **format-preserving**: when only the status changes we
//! rewrite just the `## Status` line/banner and leave everything else
//! byte-identical, so round-tripping an unchanged real ADR is a no-op.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

use crate::adr::{Adr, Number, ReviewBy, Status};

/// Which on-disk serialization profile a store uses.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    Display,
    EnumString,
    Serialize,
    Deserialize,
    clap::ValueEnum,
)]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum Format {
    /// MADR-style markdown, status encoded by directory. The default.
    #[default]
    Markdown,
    /// YAML frontmatter + body.
    Frontmatter,
}

/// Errors raised while parsing the markdown profile.
#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("missing H1 heading `# ADR-NNNN: Title`")]
    MissingHeading,
    #[error("could not parse ADR number from heading: {0}")]
    BadNumber(String),
}

/// Serialize an ADR for a fresh write in the given format.
///
/// For [`Format::Markdown`] this renders a full document from the ADR's body
/// (the body is expected to already be the markdown after the H1/banner/status,
/// e.g. produced by a template). For round-trip-stable status edits on an
/// existing file, prefer [`rewrite_status`].
pub fn serialize(adr: &Adr, format: Format) -> anyhow::Result<String> {
    match format {
        Format::Frontmatter => crate::frontmatter::serialize(adr),
        Format::Markdown => serialize_markdown(adr),
    }
}

/// Parse an ADR from a document. `dir_status` is the status implied by the
/// directory the file lives in (used by the markdown profile as the source of
/// truth, falling back to the `## Status` section when absent). `naming` resolves
/// the markdown supersession links into scheme-aware [`AdrRef`]s.
pub fn deserialize(
    input: &str,
    format: Format,
    dir_status: Option<Status>,
    naming: crate::naming::NamingScheme,
) -> anyhow::Result<Adr> {
    match format {
        Format::Frontmatter => crate::frontmatter::deserialize(input),
        Format::Markdown => parse_markdown(input, dir_status, naming),
    }
}

// ---------------------------------------------------------------------------
// Markdown profile
// ---------------------------------------------------------------------------

/// Render a brand-new markdown ADR. The body already contains the H1, banner,
/// and section scaffolding (from a template), so this is mostly a passthrough
/// that guarantees a single trailing newline.
fn serialize_markdown(adr: &Adr) -> anyhow::Result<String> {
    let mut out = adr.body.clone();
    if out.is_empty() {
        anyhow::bail!("markdown ADR body must not be empty");
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

/// Parse the H1 heading into `(number, title)`.
///
/// Accepts `# ADR-0006: Title`, `# ADR 0006: Title`, and `# 0006. Title`
/// (Nygard-style). Also tolerates a plain `# Title` with no number — used by the
/// slug-based naming schemes (date/uuid), whose identity is the filename — in
/// which case `number` is `None` and the whole heading text is the title.
fn parse_heading(line: &str) -> (Option<Number>, String) {
    let h = line.trim_start_matches('#').trim();
    // Strip an optional "ADR" prefix and separators.
    let rest = h
        .strip_prefix("ADR-")
        .or_else(|| h.strip_prefix("ADR "))
        .or_else(|| h.strip_prefix("ADR"))
        .unwrap_or(h)
        .trim_start();
    // Number is the leading run of digits, if any.
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    let Ok(n) = digits.parse::<u32>() else {
        // No leading number → the heading itself is the title (slug scheme).
        return (None, h.to_string());
    };
    let after = rest[digits.len()..].trim_start();
    // Title follows a `:` or `.` separator.
    let title = after
        .strip_prefix(':')
        .or_else(|| after.strip_prefix('.'))
        .unwrap_or(after)
        .trim()
        .to_string();
    (Some(Number::new(n)), title)
}

fn is_status_heading(line: &str) -> bool {
    let t = line.trim();
    t.eq_ignore_ascii_case("## Status")
}

fn is_heading(line: &str) -> bool {
    line.trim_start().starts_with('#')
}

fn is_references_heading(line: &str) -> bool {
    line.trim().eq_ignore_ascii_case("## References")
}

/// What the parser extracts from the `## Status` region of a markdown ADR.
///
/// Supersession is captured as the **raw fragment** after the `Superseded by` /
/// `Supersedes` keyword (the `[label](target)` or bare token). Resolving it to a
/// scheme-aware [`AdrRef`] happens in [`parse_markdown`] /
/// [`parse_markdown_section_supersession`], so this stays naming-free.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct StatusRegion {
    status: Option<Status>,
    supersedes: Option<String>,
    superseded_by: Option<String>,
    review_by: Option<ReviewBy>,
}

/// Parse the whole `## Status` region (the lines between the `## Status` heading
/// and the next heading) for the status word, both supersession directions, and
/// an optional `Review by:` line.
///
/// Supersession wording supported (tolerant of a `[ADR-NNNN](path)` link or a
/// bare `ADR-NNNN`, and of an optional leading `>` banner marker):
/// - `Superseded by [ADR-NNNN](...)` -> `superseded_by`
/// - `Supersedes [ADR-NNNN](...)` -> `supersedes`
/// - `Review by: YYYY-MM-DD` -> `review_by`
fn parse_status_region(input: &str) -> StatusRegion {
    let mut region = StatusRegion::default();
    let mut lines = input.lines();
    // Advance to the `## Status` heading.
    for line in lines.by_ref() {
        if is_status_heading(line) {
            break;
        }
    }
    for line in lines {
        if is_heading(line) {
            break; // left the region
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        parse_status_line(trimmed, &mut region);
    }
    region
}

/// Parse a single non-blank line from the status region, filling `region`.
fn parse_status_line(line: &str, region: &mut StatusRegion) {
    // Strip an optional leading banner marker (`>` and bold markers).
    let v = line
        .trim_start_matches('>')
        .trim()
        .trim_start_matches("**")
        .trim();

    if let Some(rest) = strip_prefix_ci(v, "Superseded by") {
        region.status.get_or_insert(Status::Superseded);
        if region.superseded_by.is_none() {
            region.superseded_by = Some(rest.trim().to_string());
        }
        return;
    }
    if let Some(rest) = strip_prefix_ci(v, "Supersedes") {
        if region.supersedes.is_none() {
            region.supersedes = Some(rest.trim().to_string());
        }
        return;
    }
    if let Some(rest) = strip_prefix_ci(v, "Review by:").or_else(|| strip_prefix_ci(v, "Review by"))
    {
        if region.review_by.is_none() {
            let date = rest.trim().trim_start_matches(':').trim();
            region.review_by = date.parse::<ReviewBy>().ok();
        }
        return;
    }
    // A bare status word (e.g. "Accepted", "Proposed") sets the status if we
    // have not already inferred one from a supersession note. Try the whole line
    // first, then its first word — so a qualified status line like
    // "Proposed — implementation approach evolving based on spike" still resolves
    // to Proposed instead of falling through to the default.
    if region.status.is_none() {
        let status = v.parse::<Status>().ok().or_else(|| {
            v.split_whitespace()
                .next()
                .and_then(|word| word.parse::<Status>().ok())
        });
        if let Some(status) = status {
            region.status = Some(status);
        }
    }
}

/// Case-insensitive `strip_prefix`, returning the remainder after `prefix`.
fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    if head.eq_ignore_ascii_case(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

/// Parse a full markdown ADR document. Directory status wins for `status`
/// when supplied; otherwise the `## Status` section is used. `naming` resolves
/// the supersession links/tokens into scheme-aware [`AdrRef`]s.
pub fn parse_markdown(
    input: &str,
    dir_status: Option<Status>,
    naming: crate::naming::NamingScheme,
) -> anyhow::Result<Adr> {
    let heading = input
        .lines()
        .find(|l| l.trim_start().starts_with("# "))
        .ok_or(FormatError::MissingHeading)?;
    let (number, title) = parse_heading(heading);

    let region = parse_status_region(input);

    let status = dir_status.or(region.status).unwrap_or(Status::Proposed);

    Ok(Adr {
        id: crate::adr::AdrId::new(),
        number,
        slug: None,
        category: None,
        title,
        status,
        created: crate::adr::Created::now(),
        body: input.trim_end_matches('\n').to_string(),
        git_sha: None,
        supersedes: region
            .supersedes
            .as_deref()
            .and_then(|f| naming.ref_in_note(f)),
        superseded_by: region
            .superseded_by
            .as_deref()
            .and_then(|f| naming.ref_in_note(f)),
        // Typed relational links are a frontmatter-profile feature.
        relates_to: Vec::new(),
        depends_on: Vec::new(),
        refines: Vec::new(),
        review_by: region.review_by,
    })
}

/// The status declared in a markdown ADR's `## Status` section, if one is
/// stated explicitly (a bare status word or a supersession note). Returns
/// `None` when the section has no status word — callers (e.g. `adroit check`)
/// treat that as "directory is the source of truth", not a mismatch.
///
/// Unlike [`parse_markdown`], this does NOT consult the directory, so it can be
/// compared against the directory-implied status to surface disagreements.
pub fn parse_markdown_section_status(input: &str) -> Option<Status> {
    parse_status_region(input).status
}

/// The supersession references declared in a markdown ADR's `## Status` section:
/// `(supersedes, superseded_by)` as scheme-aware [`AdrRef`]s, either of which may
/// be `None`.
///
/// Exposed for `adroit check` so it can verify the referenced ADRs exist.
///
/// `source_category` is the category of the ADR whose section this is (only the
/// `per_category` scheme uses it, to resolve a same-category link with no category
/// segment); pass `None` for every other scheme.
pub fn parse_markdown_section_supersession(
    input: &str,
    naming: crate::naming::NamingScheme,
    source_category: Option<&str>,
) -> (Option<crate::naming::AdrRef>, Option<crate::naming::AdrRef>) {
    let region = parse_status_region(input);
    (
        region
            .supersedes
            .as_deref()
            .and_then(|f| naming.ref_in_note_from(f, source_category)),
        region
            .superseded_by
            .as_deref()
            .and_then(|f| naming.ref_in_note_from(f, source_category)),
    )
}

/// Rewrite (in place, minimal-diff) the `## Status` value line and the
/// `> State:` banner of a markdown ADR.
///
/// `supersede` carries the superseding ADR's display `label` and the relative
/// markdown link `target` (e.g. `("ADR-0006", "../accepted/0006-adopt-adrs.md")`,
/// or a slug label/target under date/uuid), used when the new status is
/// [`Status::Superseded`].
///
/// Only the status value line and banner change — all other bytes (including
/// the original trailing newline) are preserved exactly. If the document has
/// no `## Status` section, one is appended after the H1/banner.
/// Replace a *lone* `\r` (a carriage return not part of `\r\n`) with `\n`, leaving
/// `\r\n` and `\n` untouched. adroit never writes lone-CR files, but an imported or
/// hand-edited one would otherwise defeat the rewriters' newline detection (the
/// lone `\r` fuses with a joined `\n` into `\r\n` on the next pass) and make
/// `rewrite_status` / `rewrite_review_by` / `upsert_reference` non-idempotent.
/// Returns the input unchanged (borrowed) when there is no lone CR, so a
/// consistent-newline document round-trips byte-for-byte.
fn normalize_lone_cr(s: &str) -> Cow<'_, str> {
    let bytes = s.as_bytes();
    let has_lone_cr = bytes
        .iter()
        .enumerate()
        .any(|(i, &b)| b == b'\r' && bytes.get(i + 1) != Some(&b'\n'));
    if !has_lone_cr {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' && chars.peek() != Some(&'\n') {
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

pub fn rewrite_status(
    original: &str,
    new_status: Status,
    supersede: Option<(&str, &str)>,
) -> String {
    let original = normalize_lone_cr(original);
    let value = match (new_status, supersede) {
        (Status::Superseded, Some((label, link))) => {
            format!("Superseded by [{label}]({link})")
        }
        _ => new_status.to_string(),
    };

    let newline = if original.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut lines: Vec<String> = original.split(newline).map(|s| s.to_string()).collect();

    // Rewrite the banner line if present.
    for line in lines.iter_mut() {
        if line.trim_start().starts_with("> State:") {
            let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
            *line = format!("{indent}> State: {new_status}");
            break;
        }
    }

    // Rewrite the `## Status` value line if the section exists.
    let mut rewrote = false;
    let mut i = 0;
    while i < lines.len() {
        if is_status_heading(&lines[i]) {
            // Find the first non-blank content line after the heading.
            let mut j = i + 1;
            while j < lines.len() && lines[j].trim().is_empty() {
                j += 1;
            }
            if j < lines.len() {
                lines[j] = value.clone();
            } else {
                lines.push(String::new());
                lines.push(value.clone());
            }
            rewrote = true;
            break;
        }
        i += 1;
    }

    if !rewrote {
        // No `## Status` section: insert one after the heading/banner block.
        let insert_at = lines
            .iter()
            .position(|l| l.trim_start().starts_with("# "))
            .map(|h| h + 1)
            .unwrap_or(0);
        let block = vec![
            String::new(),
            "## Status".to_string(),
            String::new(),
            value.clone(),
        ];
        for (k, b) in block.into_iter().enumerate() {
            lines.insert(insert_at + k, b);
        }
    }

    lines.join(newline)
}

/// Rewrite (in place, minimal-diff) the `Review by: YYYY-MM-DD` line in the
/// `## Status` region of a markdown ADR.
///
/// - `Some(date)` upserts a `Review by: <date>` line: replaces an existing one,
///   or inserts a new one immediately after the status value line.
/// - `None` removes the line if present.
///
/// All other bytes are preserved exactly. When nothing changes (e.g. removing a
/// line that does not exist), the input is returned unchanged so unedited
/// round-trips stay byte-identical.
pub fn rewrite_review_by(original: &str, review_by: Option<ReviewBy>) -> String {
    let original = normalize_lone_cr(original);
    let newline = if original.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut lines: Vec<String> = original.split(newline).map(|s| s.to_string()).collect();

    let existing = lines
        .iter()
        .position(|l| strip_prefix_ci(l.trim(), "Review by").is_some());

    match (review_by, existing) {
        (Some(date), Some(idx)) => lines[idx] = format!("Review by: {date}"),
        (Some(date), None) => {
            // Insert after the status value line (first non-blank line after the
            // `## Status` heading); fall back to after the heading.
            if let Some(insert_at) = review_insert_point(&lines) {
                lines.insert(insert_at, format!("Review by: {date}"));
            } else {
                // No `## Status` section: nothing sensible to anchor to; append.
                lines.push(format!("Review by: {date}"));
            }
        }
        (None, Some(idx)) => {
            lines.remove(idx);
        }
        (None, None) => {}
    }

    lines.join(newline)
}

/// Index at which to insert a new `Review by:` line: right after the status
/// value line inside the `## Status` section.
fn review_insert_point(lines: &[String]) -> Option<usize> {
    let heading = lines.iter().position(|l| is_status_heading(l))?;
    // Skip blanks to the status value line.
    let mut j = heading + 1;
    while j < lines.len() && lines[j].trim().is_empty() {
        j += 1;
    }
    // Insert after the status value line (or after the heading block if none).
    Some(if j < lines.len() { j + 1 } else { j })
}

/// Upsert a `- <label>: <url>` bullet into the ADR's `## References` section,
/// preserving the rest of the document byte-for-byte. The section is created at
/// the end of the file if absent. **Idempotent per label**: re-running with the
/// same label replaces only that line's URL, and a no-change write is
/// byte-identical — so the forge integration can call it repeatedly.
pub fn upsert_reference(original: &str, label: &str, url: &str) -> String {
    let original = normalize_lone_cr(original);
    let newline = if original.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut lines: Vec<String> = original.split(newline).map(str::to_string).collect();
    let entry = format!("- {label}: {url}");
    let label_prefix = format!("- {label}:").to_ascii_lowercase();

    match lines.iter().position(|l| is_references_heading(l)) {
        Some(h) => {
            // Section runs from the heading to the next heading (or EOF).
            let end = lines[h + 1..]
                .iter()
                .position(|l| is_heading(l))
                .map_or(lines.len(), |rel| h + 1 + rel);
            let existing = lines[h + 1..end]
                .iter()
                .position(|l| {
                    l.trim_start()
                        .to_ascii_lowercase()
                        .starts_with(&label_prefix)
                })
                .map(|rel| h + 1 + rel);
            match existing {
                Some(idx) => lines[idx] = entry,
                None => {
                    // Append after the section's last non-blank line.
                    let mut at = end;
                    while at > h + 1 && lines[at - 1].trim().is_empty() {
                        at -= 1;
                    }
                    lines.insert(at, entry);
                }
            }
        }
        None => {
            while lines.last().is_some_and(|l| l.trim().is_empty()) {
                lines.pop();
            }
            lines.push(String::new());
            lines.push("## References".to_string());
            lines.push(String::new());
            lines.push(entry);
        }
    }

    let mut out = lines.join(newline);
    if original.ends_with('\n') && !out.ends_with('\n') {
        out.push_str(newline);
    }
    out
}

/// Parse the `- label: url` bullets from an ADR's `## References` section, in
/// order. The forge integration writes these (issue / pull request URLs) and
/// reads them back on `set-status` / `supersede`.
pub fn parse_references(original: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut in_section = false;
    for line in original.lines() {
        if is_references_heading(line) {
            in_section = true;
            continue;
        }
        if in_section && is_heading(line) {
            break;
        }
        if in_section
            && let Some(rest) = line.trim().strip_prefix("- ")
            && let Some((label, url)) = rest.split_once(':')
        {
            out.push((label.trim().to_string(), url.trim().to_string()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::naming::{AdrRef, NamingScheme};

    /// Parse a sequential markdown ADR (the scheme used by these format tests).
    fn pm(input: &str, dir_status: Option<Status>) -> Adr {
        parse_markdown(input, dir_status, NamingScheme::Sequential).unwrap()
    }

    const SAMPLE: &str = "# ADR-0006: Adopt ADRs as Team Decision Process\n\
\n\
> State: Accepted\n\
\n\
## Status\n\
\n\
Accepted\n\
\n\
## Context and Problem Statement\n\
\n\
We need a consistent way to capture architectural decisions.\n";

    #[test]
    fn parse_heading_adr_dash() {
        let (n, t) = parse_heading("# ADR-0006: Adopt ADRs as Team Decision Process");
        assert_eq!(n, Some(Number::new(6)));
        assert_eq!(t, "Adopt ADRs as Team Decision Process");
    }

    #[test]
    fn parse_heading_nygard_dot() {
        let (n, t) = parse_heading("# 0042. Use PostgreSQL");
        assert_eq!(n, Some(Number::new(42)));
        assert_eq!(t, "Use PostgreSQL");
    }

    #[test]
    fn parse_heading_plain_title_has_no_number() {
        // Slug-scheme heading: no ADR-NNNN, the whole heading is the title.
        let (n, t) = parse_heading("# Adopt Crossplane");
        assert_eq!(n, None);
        assert_eq!(t, "Adopt Crossplane");
    }

    #[test]
    fn parse_markdown_uses_dir_status() {
        let adr = pm(SAMPLE, Some(Status::Accepted));
        assert_eq!(adr.number, Some(Number::new(6)));
        assert_eq!(adr.title, "Adopt ADRs as Team Decision Process");
        assert_eq!(adr.status, Status::Accepted);
    }

    #[test]
    fn parse_markdown_falls_back_to_section_status() {
        let adr = pm(SAMPLE, None);
        assert_eq!(adr.status, Status::Accepted);
    }

    #[test]
    fn parse_superseded_link() {
        let doc = "# ADR-0002: Adopt ADRs\n\n## Status\n\nSuperseded by [ADR-0006](../accepted/0006-adopt-adrs.md)\n";
        let adr = pm(doc, Some(Status::Superseded));
        assert_eq!(adr.status, Status::Superseded);
        assert_eq!(adr.superseded_by, Some(AdrRef::Number(6)));
    }

    #[test]
    fn rewrite_status_is_minimal_diff() {
        let out = rewrite_status(SAMPLE, Status::Rejected, None);
        // Banner + status value lines changed; everything else identical.
        assert!(out.contains("> State: Rejected"));
        assert!(out.contains("\n## Status\n\nRejected\n"));
        assert!(out.contains("We need a consistent way to capture architectural decisions."));
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn rewrite_status_superseded_writes_link() {
        let out = rewrite_status(
            SAMPLE,
            Status::Superseded,
            Some(("ADR-0009", "../accepted/0009-thing.md")),
        );
        assert!(out.contains("Superseded by [ADR-0009](../accepted/0009-thing.md)"));
        assert!(out.contains("> State: Superseded"));
    }

    #[test]
    fn rewrite_then_reparse_round_trips_unchanged_status() {
        // Rewriting to the same status must be byte-identical.
        let out = rewrite_status(SAMPLE, Status::Accepted, None);
        assert_eq!(out, SAMPLE);
    }

    // --- supersession: both directions out of the `## Status` region ---------

    #[test]
    fn parse_supersedes_forward_note() {
        // The newer ADR carries a "Supersedes [ADR-NNNN]" note in `## Status`.
        let doc = "# ADR-0006: Adopt ADRs\n\n## Status\n\nAccepted\n\nSupersedes [ADR-0002](../superseded/0002-adopt-adrs.md)\n\n## Context\n\nBody.\n";
        let adr = pm(doc, Some(Status::Accepted));
        assert_eq!(adr.status, Status::Accepted);
        assert_eq!(adr.supersedes, Some(AdrRef::Number(2)));
        assert_eq!(adr.superseded_by, None);
    }

    #[test]
    fn parse_superseded_by_note() {
        // Mirrors superseded/0002-adopt-adrs.md.
        let doc = "# ADR-0002: Adopt ADRs\n\n## Status\n\nSuperseded by [ADR-0006](../accepted/0006-adopt-adrs.md)\n";
        let adr = pm(doc, Some(Status::Superseded));
        assert_eq!(adr.superseded_by, Some(AdrRef::Number(6)));
        assert_eq!(adr.supersedes, None);
    }

    #[test]
    fn parse_supersedes_bare_adr_reference() {
        let doc = "# ADR-0006: Adopt ADRs\n\n## Status\n\nAccepted\n\nSupersedes ADR-0002\n";
        let adr = pm(doc, Some(Status::Accepted));
        assert_eq!(adr.supersedes, Some(AdrRef::Number(2)));
    }

    #[test]
    fn parse_superseded_link_date_scheme_slug() {
        // Under the date scheme the supersession link carries a slug, not a
        // number — the seam resolves it to a Slug ref.
        let doc = "# New approach\n\n## Status\n\nSuperseded by [20260601-old](../accepted/20260601-old.md)\n";
        let adr = parse_markdown(doc, Some(Status::Superseded), NamingScheme::Date).unwrap();
        assert_eq!(adr.superseded_by, Some(AdrRef::Slug("20260601-old".into())));
    }

    #[test]
    fn parse_review_by_line() {
        let doc = "# ADR-0003: Use Redis\n\n## Status\n\nProposed\n\nReview by: 2026-07-15\n";
        let adr = pm(doc, Some(Status::Proposed));
        assert_eq!(adr.review_by, Some("2026-07-15".parse().unwrap()));
    }

    #[test]
    fn markdown_supersession_round_trips_both_directions() {
        // parse -> the body is the document; re-parsing yields the same fields.
        let doc = "# ADR-0006: Adopt ADRs\n\n## Status\n\nAccepted\n\nSupersedes [ADR-0002](../superseded/0002.md)\n";
        let adr = pm(doc, Some(Status::Accepted));
        let again = pm(&adr.body, Some(Status::Accepted));
        assert_eq!(again.supersedes, Some(AdrRef::Number(2)));
    }

    // --- review_by upsert/remove ---------------------------------------------

    #[test]
    fn rewrite_review_by_inserts_after_status_value() {
        let rb: ReviewBy = "2026-07-01".parse().unwrap();
        let out = rewrite_review_by(SAMPLE, Some(rb));
        assert!(out.contains("\n## Status\n\nAccepted\nReview by: 2026-07-01\n"));
        // Everything else preserved.
        assert!(out.contains("We need a consistent way to capture architectural decisions."));
        assert!(out.ends_with('\n'));
        // Re-parses back.
        let adr = pm(&out, Some(Status::Accepted));
        assert_eq!(adr.review_by, Some(rb));
    }

    #[test]
    fn rewrite_review_by_replaces_existing() {
        let rb1: ReviewBy = "2026-07-01".parse().unwrap();
        let rb2: ReviewBy = "2026-08-15".parse().unwrap();
        let with = rewrite_review_by(SAMPLE, Some(rb1));
        let updated = rewrite_review_by(&with, Some(rb2));
        assert!(updated.contains("Review by: 2026-08-15"));
        assert!(!updated.contains("2026-07-01"));
        // Exactly one Review by line.
        assert_eq!(updated.matches("Review by:").count(), 1);
    }

    #[test]
    fn rewrite_review_by_removes_when_none() {
        let rb: ReviewBy = "2026-07-01".parse().unwrap();
        let with = rewrite_review_by(SAMPLE, Some(rb));
        let removed = rewrite_review_by(&with, None);
        assert!(!removed.contains("Review by:"));
        // Removing maps back to the original document byte-for-byte.
        assert_eq!(removed, SAMPLE);
    }

    #[test]
    fn rewrite_review_by_none_on_clean_doc_is_byte_identical() {
        assert_eq!(rewrite_review_by(SAMPLE, None), SAMPLE);
    }

    #[test]
    fn status_region_tolerates_multibyte_chars_at_prefix_boundary() {
        // Regression: "Proposed — note" has an em-dash starting at byte 9, which
        // sits inside the byte range of the 10-char prefixes "Supersedes" /
        // "Review by:". `strip_prefix_ci` must not panic on a non-char-boundary.
        let doc = "# ADR-0003: Sample\n\n## Status\n\nProposed — implementation evolving\n";
        let region = parse_status_region(doc);
        assert!(region.supersedes.is_none());
        assert!(region.superseded_by.is_none());
        assert!(region.review_by.is_none());
    }

    #[test]
    fn upsert_reference_creates_section_then_upserts_idempotently() {
        let base = "# ADR-0001: X\n\n## Status\n\nProposed\n";
        // First write creates the section.
        let a = upsert_reference(base, "Issue", "https://x/issues/7");
        assert!(a.contains("## References"));
        assert!(a.contains("- Issue: https://x/issues/7"));
        assert!(a.ends_with('\n'));
        // Re-writing the same label+url is byte-identical (idempotent).
        assert_eq!(upsert_reference(&a, "Issue", "https://x/issues/7"), a);
        // A second label appends a bullet, not a second section.
        let b = upsert_reference(&a, "Pull Request", "https://x/pull/42");
        assert_eq!(b.matches("## References").count(), 1);
        assert!(b.contains("- Issue: https://x/issues/7"));
        assert!(b.contains("- Pull Request: https://x/pull/42"));
        // Re-using an existing label replaces only its URL.
        let c = upsert_reference(&b, "Issue", "https://x/issues/9");
        assert!(c.contains("- Issue: https://x/issues/9"));
        assert!(!c.contains("issues/7"));
        assert_eq!(parse_references(&c).len(), 2);
        assert_eq!(
            parse_references(&c)[0],
            ("Issue".to_string(), "https://x/issues/9".to_string())
        );
    }

    #[test]
    fn qualified_status_line_resolves_via_first_word() {
        // A real-repo status line with trailing qualification resolves to the
        // bare status via its first word (the whole line doesn't parse).
        let doc = "# ADR-0003: X\n\n## Status\n\nProposed — implementation approach evolving based on spike\n";
        assert_eq!(parse_markdown_section_status(doc), Some(Status::Proposed));

        // A plain status word still works.
        let plain = "# ADR-0004: X\n\n## Status\n\nAccepted\n";
        assert_eq!(parse_markdown_section_status(plain), Some(Status::Accepted));

        // A section whose first word is not a status stays None (the directory
        // remains the source of truth — not a mismatch).
        let prose = "# ADR-0005: X\n\n## Status\n\nSee the discussion thread.\n";
        assert_eq!(parse_markdown_section_status(prose), None);
    }

    #[test]
    fn normalize_lone_cr_preserves_crlf_and_lf() {
        assert!(matches!(normalize_lone_cr("a\nb"), Cow::Borrowed(_)));
        assert!(matches!(normalize_lone_cr("a\r\nb"), Cow::Borrowed(_)));
        assert_eq!(normalize_lone_cr("a\rb").as_ref(), "a\nb");
        assert_eq!(normalize_lone_cr("a\r\nb\rc").as_ref(), "a\r\nb\nc");
        // Multibyte stays intact.
        assert_eq!(normalize_lone_cr("é\rx").as_ref(), "é\nx");
    }

    #[test]
    fn rewriters_are_idempotent_on_lone_cr() {
        // Regression (hardening blitz #4): a lone `\r` (classic-Mac / corrupted
        // file) used to defeat newline detection and make these non-idempotent —
        // a second `upsert_reference` duplicated `## References`. They now
        // normalize a lone `\r` to `\n` first.
        let doc = "# ADR-0001: X\r\r## Status\r\rProposed\r"; // CR-only line endings

        let once = upsert_reference(doc, "Issue", "https://x/7");
        assert_eq!(
            upsert_reference(&once, "Issue", "https://x/7"),
            once,
            "upsert not idempotent on lone CR"
        );
        assert_eq!(
            once.matches("## References").count(),
            1,
            "no duplicate section"
        );
        assert!(!once.contains('\r'), "lone CR normalized away");

        let s1 = rewrite_status(doc, Status::Accepted, None);
        assert_eq!(rewrite_status(&s1, Status::Accepted, None), s1);

        let r1 = rewrite_review_by(doc, None);
        assert_eq!(rewrite_review_by(&r1, None), r1);
    }
}
