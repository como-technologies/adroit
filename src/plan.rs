//! Persisted implementation plans (ADR-0008).
//!
//! `adroit plan <ID> --save` persists the generated implementation plan
//! *inside* the ADR document, as a marker-bracketed `## Implementation`
//! section:
//!
//! ```text
//! ## Implementation
//!
//! <!-- adroit:plan -->
//!
//! 1. The plan, verbatim markdown.
//!
//! <!-- /adroit:plan -->
//! ```
//!
//! The pure splice/extract engine lives here, terminal- and filesystem-free:
//! [`splice`] replaces an existing managed section (or appends one), and
//! [`extract`] reads the stored plan back — the deterministic, provider-free
//! read path `plan <ID>` and `show -o json` use. The begin/end marker pair
//! brackets the section so the plan stays **opaque free-form markdown**: a
//! plan containing its own `## ` sub-headings round-trips verbatim (a single
//! begin marker with a stop-at-next-heading rule would truncate it).
//!
//! Only marker lines are adroit's. The default templates ship a placeholder
//! `## Implementation` section ("draft it later with `adroit plan`") whose
//! content is nothing but italic `_…_` prompt lines — [`splice`] replaces that
//! placeholder in place. A genuinely hand-written section is never touched:
//! [`has_hand_written_section`] lets the CLI refuse to manage a plan alongside
//! one. Known limitation (documented, not defended): a plan whose own text
//! carries an `<!-- /adroit:plan -->` line is not representable verbatim —
//! extraction stops at the first end-marker line.

/// Marks the start of the adroit-managed plan section (alone on a line).
pub const PLAN_MARKER: &str = "<!-- adroit:plan -->";

/// Marks the end of the adroit-managed plan section (alone on a line).
pub const PLAN_END_MARKER: &str = "<!-- /adroit:plan -->";

/// The heading of the managed section.
const HEADING: &str = "## Implementation";

/// Byte span of one line within `body`, including its trailing newline.
struct Line<'a> {
    text: &'a str,
    start: usize,
    /// Offset just past the line's `\n` (or `body.len()` on the last line).
    next: usize,
}

/// Iterate `body` line by line with byte offsets (split on `\n`; a trailing
/// `\r` stays on `text` and is handled by callers via `trim`).
fn lines_with_spans(body: &str) -> impl Iterator<Item = Line<'_>> {
    let mut start = 0;
    body.split_inclusive('\n').map(move |raw| {
        let s = start;
        start += raw.len();
        Line {
            text: raw.trim_end_matches(['\n', '\r']),
            start: s,
            next: start,
        }
    })
}

/// The begin-marker line: `(span_start, content_start)` where `span_start`
/// includes the `## Implementation` heading when it is the nearest non-blank
/// line above the marker, and `content_start` is the offset just past the
/// marker line.
fn begin_of(body: &str) -> Option<(usize, usize)> {
    let mut heading_above: Option<usize> = None; // nearest non-blank line start
    for line in lines_with_spans(body) {
        let t = line.text.trim();
        if t == PLAN_MARKER {
            return Some((heading_above.unwrap_or(line.start), line.next));
        }
        if !t.is_empty() {
            heading_above = t.eq_ignore_ascii_case(HEADING).then_some(line.start);
        }
    }
    None
}

/// Where the managed section's content ends, scanning from `content_start`:
/// `(content_end, span_end)`. The end marker is authoritative (so a plan's own
/// `## ` sub-headings stay inside the section); only when it is lost
/// (hand-edited) does the section degrade to ending at the next `## ` heading,
/// or the end of the body.
fn end_of(body: &str, content_start: usize) -> (usize, usize) {
    let rest = &body[content_start..];
    let mut first_heading: Option<usize> = None;
    for line in lines_with_spans(rest) {
        let t = line.text.trim();
        if t == PLAN_END_MARKER {
            return (content_start + line.start, content_start + line.next);
        }
        if first_heading.is_none() && t.starts_with("## ") {
            first_heading = Some(content_start + line.start);
        }
    }
    match first_heading {
        Some(h) => (h, h),
        None => (body.len(), body.len()),
    }
}

/// The byte span of the managed section: from the `## Implementation` heading
/// (when it is the nearest non-blank line above the begin marker) or the begin
/// marker line, through the end-marker line inclusive. Without an end marker
/// (hand-edited), the span runs to the next `## ` heading or the end of the
/// body. `None` when the body has no begin-marker line.
fn marked_span(body: &str) -> Option<(usize, usize)> {
    let (span_start, content_start) = begin_of(body)?;
    let (_, span_end) = end_of(body, content_start);
    Some((span_start, span_end))
}

/// The stored plan, when the body carries a managed section: the verbatim
/// markdown between the begin and end markers, trimmed. `None` when there is
/// no begin-marker line or the section is empty.
pub fn extract(body: &str) -> Option<&str> {
    let (_, content_start) = begin_of(body)?;
    let (content_end, _) = end_of(body, content_start);
    let content = body[content_start..content_end].trim();
    if content.is_empty() {
        None
    } else {
        Some(content)
    }
}

/// The first unmarked `## Implementation` section:
/// `(span_start, content_start, span_end)`, the content running to the next
/// `## ` heading or the end of the body. Callers use it only when there is no
/// marked span (the marked section's heading would match here too).
fn unmarked_section_span(body: &str) -> Option<(usize, usize, usize)> {
    let mut start: Option<(usize, usize)> = None;
    for line in lines_with_spans(body) {
        let t = line.text.trim();
        match start {
            None => {
                if t.eq_ignore_ascii_case(HEADING) {
                    start = Some((line.start, line.next));
                }
            }
            Some((s, cs)) => {
                if t.starts_with("## ") {
                    return Some((s, cs, line.start));
                }
            }
        }
    }
    start.map(|(s, cs)| (s, cs, body.len()))
}

/// A section content adroit may replace: empty, or nothing but the template's
/// italic `_…_` authoring prompt (the placeholder the default templates ship
/// with the instruction to "draft it later with `adroit plan`").
fn replaceable(content: &str) -> bool {
    content.trim().is_empty() || crate::lint::prompt_only(content)
}

/// A mechanical supersession banner (`supersede` appends `> Supersedes […]` at
/// the very end of the markdown document, which lands inside whatever section
/// is last). Such lines are Status-region content adroit owns — never part of
/// a placeholder judgement, and always preserved across a splice.
fn is_banner(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("> Supersedes") || t.starts_with("> Superseded by")
}

/// Where a placeholder replacement must stop inside `content` (offsets relative
/// to `content`): the start of the trailing run of supersession-banner lines
/// (and blanks), or `content.len()` when there is none.
fn before_trailing_banners(content: &str) -> usize {
    let mut cut = content.len();
    let mut run_start: Option<usize> = None;
    for line in lines_with_spans(content) {
        let t = line.text.trim();
        if t.is_empty() {
            continue;
        }
        if is_banner(line.text) {
            run_start.get_or_insert(line.start);
        } else {
            run_start = None;
        }
    }
    if let Some(s) = run_start {
        cut = s;
    }
    cut
}

/// True when the body carries a genuinely hand-written `## Implementation`
/// section: an unmarked heading (outside the managed, marker-bracketed span)
/// whose content is more than the template's placeholder prompt. The CLI
/// refuses `--save` on such a document — adroit never overwrites or shadows
/// hand-written content.
pub fn has_hand_written_section(body: &str) -> bool {
    let span = marked_span(body);
    // Hand-written = more than the placeholder prompt, ignoring any trailing
    // mechanical supersession banners (see [`is_banner`]).
    let hand_written = |cs: usize, ce: usize| {
        let content = &body[cs..ce];
        !replaceable(&content[..before_trailing_banners(content)])
    };
    let mut open: Option<usize> = None; // content start of an open unmarked section
    for line in lines_with_spans(body) {
        let t = line.text.trim();
        if !t.starts_with("## ") {
            continue;
        }
        if let Some(cs) = open.take()
            && hand_written(cs, line.start)
        {
            return true;
        }
        let in_span = span.is_some_and(|(s, e)| line.start >= s && line.start < e);
        if t.eq_ignore_ascii_case(HEADING) && !in_span {
            open = Some(line.next);
        }
    }
    open.is_some_and(|cs| hand_written(cs, body.len()))
}

/// Splice `plan` into `body` as the managed `## Implementation` section:
/// replaces the existing managed span, else replaces a [`replaceable`]
/// placeholder `## Implementation` section in place, else appends the section
/// at the end. Pure and idempotent for a marker-free `plan`
/// (`splice(splice(b, p), p) == splice(b, p)`); the caller enforces the
/// overwrite (`--force`) and [`has_hand_written_section`] guards.
pub fn splice(body: &str, plan: &str) -> String {
    let section = format!(
        "{HEADING}\n\n{PLAN_MARKER}\n\n{}\n\n{PLAN_END_MARKER}",
        plan.trim()
    );
    let span = marked_span(body).or_else(|| {
        unmarked_section_span(body).and_then(|(s, cs, e)| {
            // A trailing run of mechanical supersession banners is not part of
            // the placeholder: judge replaceability without it and keep it
            // after the spliced section.
            let cut = cs + before_trailing_banners(&body[cs..e]);
            replaceable(&body[cs..cut]).then_some((s, cut))
        })
    });
    let mut out = String::with_capacity(body.len() + section.len() + 4);
    match span {
        Some((start, end)) => {
            let before = body[..start].trim_end();
            let after = body[end..].trim_start();
            if !before.is_empty() {
                out.push_str(before);
                out.push_str("\n\n");
            }
            out.push_str(&section);
            if !after.is_empty() {
                out.push_str("\n\n");
                out.push_str(after);
            }
        }
        None => {
            let before = body.trim_end();
            if !before.is_empty() {
                out.push_str(before);
                out.push_str("\n\n");
            }
            out.push_str(&section);
        }
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = "# ADR-0001: Use PostgreSQL\n\n## Status\n\nAccepted\n\n\
## Context and Problem Statement\n\nWe need a database.\n";

    #[test]
    fn splice_appends_a_marked_section_and_extract_round_trips() {
        let plan = "1. Create the schema.\n2. Add tests.";
        let out = splice(BODY, plan);
        assert!(out.starts_with("# ADR-0001: Use PostgreSQL"));
        assert!(out.contains("## Implementation\n\n<!-- adroit:plan -->"));
        assert!(out.ends_with(&format!("\n\n{PLAN_END_MARKER}\n")));
        assert_eq!(extract(&out), Some(plan));
    }

    #[test]
    fn extract_preserves_plan_sub_headings_verbatim() {
        // The end marker (not a stop-at-next-heading rule) bounds the section,
        // so a plan with its own `## ` sub-headings survives byte-for-byte.
        let plan = "1. Stand up the schema.\n\n## Rollout\n\n- [ ] Staging first.";
        let out = splice(BODY, plan);
        assert_eq!(extract(&out), Some(plan));
    }

    #[test]
    fn splice_replaces_an_existing_managed_section() {
        let once = splice(BODY, "old plan");
        let twice = splice(&once, "new plan");
        assert_eq!(extract(&twice), Some("new plan"));
        assert_eq!(twice.matches(PLAN_MARKER).count(), 1);
        assert_eq!(twice.matches("## Implementation").count(), 1);
        // Surrounding content is untouched.
        assert!(twice.contains("We need a database."));
    }

    #[test]
    fn splice_is_idempotent() {
        let plan = "1. One.\n2. Two.";
        let once = splice(BODY, plan);
        assert_eq!(splice(&once, plan), once);
    }

    #[test]
    fn splice_keeps_trailing_sections_after_the_managed_span() {
        let body = format!(
            "{}\n## Links\n\n- [x](https://example.com)\n",
            splice(BODY, "p1")
        );
        // Move the managed section is not required — replacing in place keeps
        // the trailing `## Links` section. (Build a body with content after
        // the span by splicing into a doc that ends with another section.)
        let with_links = splice(&body, "p2");
        assert_eq!(extract(&with_links), Some("p2"));
        assert!(with_links.contains("## Links"));
        let links_pos = with_links.find("## Links").unwrap();
        let end_pos = with_links.find(PLAN_END_MARKER).unwrap();
        assert!(links_pos > end_pos, "trailing section stays after the span");
    }

    #[test]
    fn extract_returns_none_without_a_marker_or_with_an_empty_section() {
        assert_eq!(extract(BODY), None);
        // An unmarked `## Implementation` section is NOT a stored plan.
        let hand = format!("{BODY}\n## Implementation\n\nBy hand.\n");
        assert_eq!(extract(&hand), None);
        let empty = format!("{BODY}\n## Implementation\n\n{PLAN_MARKER}\n\n{PLAN_END_MARKER}\n");
        assert_eq!(extract(&empty), None);
    }

    #[test]
    fn extract_degrades_to_next_heading_when_the_end_marker_is_lost() {
        let body = format!(
            "{BODY}\n## Implementation\n\n{PLAN_MARKER}\n\n1. Step.\n\n## Links\n\n- none\n"
        );
        assert_eq!(extract(&body), Some("1. Step."));
    }

    #[test]
    fn an_inline_marker_mention_is_not_a_section() {
        // Prose discussing the `<!-- adroit:plan -->` marker (e.g. ADR-0008
        // itself) must not read as a stored plan: only a line consisting
        // solely of the marker counts.
        let body = format!("{BODY}\nThe `{PLAN_MARKER}`-marked section.\n");
        assert_eq!(extract(&body), None);
        assert!(!has_hand_written_section(&body));
    }

    #[test]
    fn splice_replaces_the_template_placeholder_section_in_place() {
        // The default templates ship `## Implementation` with an italic `_…_`
        // prompt ("draft it later with `adroit plan`") — that placeholder is
        // replaceable, in place, keeping any later sections after it.
        let body = format!(
            "{BODY}\n## Implementation\n\n_How will the decision be carried out — draft it \
             later with `adroit plan`._\n\n## Links\n\n- none\n"
        );
        let out = splice(&body, "1. Step.");
        assert_eq!(extract(&out), Some("1. Step."));
        assert!(!out.contains("_How will the decision"), "{out}");
        assert_eq!(out.matches("## Implementation").count(), 1, "{out}");
        let links = out.find("## Links").unwrap();
        let end = out.find(PLAN_END_MARKER).unwrap();
        assert!(
            links > end,
            "section replaced in place, `## Links` after it"
        );
    }

    #[test]
    fn has_hand_written_section_flags_real_content_only() {
        assert!(!has_hand_written_section(BODY));
        // The template placeholder is not hand-written …
        let placeholder = format!("{BODY}\n## Implementation\n\n_Draft it later._\n");
        assert!(!has_hand_written_section(&placeholder));
        // … an empty section is not either …
        let empty = format!("{BODY}\n## Implementation\n");
        assert!(!has_hand_written_section(&empty));
        // … but real prose is.
        let hand = format!("{BODY}\n## Implementation\n\nBy hand.\n");
        assert!(has_hand_written_section(&hand));
        // The managed section itself never flags.
        let managed = splice(BODY, "1. Step.");
        assert!(!has_hand_written_section(&managed));
        // Both at once: the hand-written one still flags.
        let both = format!("{}\n## Implementation\n\nBy hand.\n", managed.trim_end());
        assert!(has_hand_written_section(&both));
    }

    #[test]
    fn trailing_supersession_banners_are_kept_and_never_block_a_save() {
        // `supersede` appends `> Supersedes […]` at the very end of the
        // document — inside the last section. That banner is mechanical Status
        // content: it must not make the placeholder look hand-written, and it
        // must survive the splice (after the managed section).
        let banner = "> Supersedes [ADR-0001](../superseded/0001-one.md)";
        let body = format!("{BODY}\n## Implementation\n\n_Draft it later._\n\n{banner}\n");
        assert!(!has_hand_written_section(&body));
        let out = splice(&body, "1. Step.");
        assert_eq!(extract(&out), Some("1. Step."));
        assert!(out.contains(banner), "{out}");
        let banner_pos = out.find(banner).unwrap();
        let end_pos = out.find(PLAN_END_MARKER).unwrap();
        assert!(
            banner_pos > end_pos,
            "banner stays after the section: {out}"
        );
        // Real prose above the banner still refuses.
        let hand = format!("{BODY}\n## Implementation\n\nBy hand.\n\n{banner}\n");
        assert!(has_hand_written_section(&hand));
    }

    #[test]
    fn splice_on_an_empty_body_emits_just_the_section() {
        let out = splice("", "1. Step.");
        assert!(out.starts_with(HEADING));
        assert_eq!(extract(&out), Some("1. Step."));
    }
}
