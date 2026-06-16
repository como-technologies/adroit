//! `adroit lint`: authoring-quality checks on a single ADR's prose — read-only,
//! and distinct from `check` (which validates *structural* repo integrity).
//!
//! The mechanical checks here need no AI: they catch the ways an ADR draft is
//! obviously unfinished — sections left as nothing but their italic `_…_`
//! prompt, no honest negative consequences, only one option considered. The
//! prompt check is template-agnostic (any section that's still just its shipped
//! prompt), so it tracks `template::MADR` without a hardcoded list. `adroit lint
//! --ai` layers a model review on top (handled in `main`); these stay
//! deterministic so `lint` is usable in CI without a provider.

use serde::Serialize;

use crate::view::Severity;

/// Where a finding came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, strum::Display)]
#[cfg_attr(feature = "manifest", derive(schemars::JsonSchema))]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum LintSource {
    /// A deterministic structural/content check.
    Mechanical,
    /// The optional AI review (`--ai`).
    Ai,
}

/// One authoring-quality finding. `severity` mirrors `check`'s split: an
/// `Error` gates the exit code (an unfinished draft), a `Warning` advises
/// (visible, but a CI lint gate stays green).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "manifest", derive(schemars::JsonSchema))]
pub struct LintFinding {
    pub source: LintSource,
    pub severity: Severity,
    pub message: String,
}

/// Run the mechanical authoring checks over an ADR body, returning findings
/// (empty = clean). Pure and deterministic — no network, no `ai` feature.
pub fn lint(body: &str) -> Vec<LintFinding> {
    let mut out = Vec::new();

    // 1. Sections left as nothing but their italic `_…_` prompt → unfilled.
    //    This is template-agnostic: any section whose only content is the
    //    shipped prompt (see `template::MADR`) is the author's to write.
    for (heading, content) in sections(body) {
        if prompt_only(&content) {
            let name = heading.trim_start_matches('#').trim();
            out.push(mech(format!(
                "`{name}` still holds only its prompt — replace it with real content"
            )));
        }
    }

    // 2. Honest negative consequences (people skip these). A prompt-only section
    //    is already caught above, so only flag a missing or genuinely empty one.
    //    Depth-tolerant: MADR nests `### Negative Consequences` under the
    //    Decision Outcome, but models (and humans) routinely record it at `##`
    //    — depth is shape, not substance, so both count (the same
    //    reconciliation as counting `###`-recorded options below; run-1 of the
    //    full loop failed 2 of 11 seeded ADRs on this).
    match section(body, "### Negative Consequences")
        .or_else(|| section(body, "## Negative Consequences"))
    {
        None => out.push(mech(
            "no `Negative Consequences` section — document the trade-offs honestly".into(),
        )),
        Some(c) if c.trim().is_empty() => out.push(mech(
            "`Negative Consequences` is empty — every decision has downsides; name them".into(),
        )),
        _ => {}
    }

    // 3. More than one option considered (record the alternatives you rejected).
    //    Skip while the section is still the prompt — that's covered by (1).
    if let Some(opts) = section(body, "## Considered Options")
        && !prompt_only(&opts)
        && list_items(&opts) + option_headings(&opts) < 2
    {
        out.push(mech(
            "fewer than two options under `## Considered Options` — record the alternatives \
             you weighed and rejected"
                .into(),
        ));
    }

    // 4. Repeated top-level sections (run-1: a model echoed the seed skeleton,
    //    duplicating `## Status` / `## Stakeholders`, and lint was silent). A
    //    Warning — a duplicate reads as an echo/merge artifact to clean up,
    //    not an unfinished draft, so it advises without gating CI.
    for (name, count) in repeated_top_level_sections(body) {
        out.push(warn(format!(
            "`## {name}` appears {count} times — duplicated top-level section \
             (often a model echo of the template); keep one"
        )));
    }

    // 5. Whole-line bracket placeholders (run-2: the model closed playbook
    //    ADR-0010 with "[Insert implementation plan or other details as
    //    needed]" — a NOVEL placeholder the template never contained, so the
    //    prompt check (1) was silent). A Warning, like the skeleton echo:
    //    filler to delete or replace, not an unfinished-draft gate. Fenced
    //    code is exempt — an example config legitimately shows
    //    `[insert API key]`-style lines.
    let mut in_fence = false;
    for line in body.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence && bracket_placeholder(line) {
            out.push(warn(format!(
                "`{}` is a bracket placeholder — model-shaped filler, not content; \
                 replace it or delete the line",
                line.trim()
            )));
        }
    }

    out
}

/// Openers that mark a whole-line `[…]` span as an unfilled placeholder.
/// Curated from observed model output (run-2's "[Insert implementation plan or
/// other details as needed]", the M5 rehearsal's "[Your Name]") plus the
/// classic template-filler shapes — matched case-insensitively on a word
/// boundary, so `[insertion order matters]` never matches `insert`. Like
/// [`RESIDUE_OPENERS`](crate::ai), a curated list over a clever heuristic:
/// novel entries get added when observed, and legitimate prose is never
/// guessed at.
const PLACEHOLDER_OPENERS: [&str; 23] = [
    // imperative template-filler verbs
    "insert",
    "add",
    "describe",
    "list",
    "enter",
    "provide",
    "specify",
    "replace",
    "fill",
    "include",
    "write",
    "outline",
    "summarize",
    "attach",
    // possessive / nominal placeholder shapes
    "your",
    "name of",
    // classic unfilled-value tokens
    "to be",
    "todo",
    "tbd",
    "tba",
    "fixme",
    "placeholder",
    "optional",
];

/// True if `line` is nothing but a **bracket-placeholder span** — a whole line
/// (optionally behind a list marker) of the form `[Insert …]` / `[Your Name]` /
/// `[TBD]`: model-shaped filler, not content (the run-2 wart class — novel
/// placeholders the template never contained, so the prompt-echo check is
/// silent on them).
///
/// Conservative by design; legitimate bracket constructs never match:
/// - links / images / reference definitions / footnotes continue past the
///   closing `]` (`[t](url)`, `[t][ref]`, `[ref]: url`), so the whole-line
///   requirement excludes them;
/// - checkboxes (`[ ]`, `[x]`) and citations (`[1]`, `[^1]`) have empty or
///   single-token inner text that's not in the curated opener list — as do
///   TOML-style `[section]` lines;
/// - the opener must end on a word boundary (end / space / `:`), so
///   `[insertion order matters]` is not `insert`;
/// - a 4-space- or tab-indented line is an indented code block, never flagged
///   (callers additionally skip fenced code).
pub fn bracket_placeholder(line: &str) -> bool {
    if line.starts_with("    ") || line.starts_with('\t') {
        return false; // indented code block
    }
    let t = strip_list_marker(line.trim());
    let Some(inner) = t.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) else {
        return false;
    };
    if inner.contains('[') || inner.contains(']') {
        return false; // composite construct (reference link, nested spans)
    }
    let inner = inner.trim().to_lowercase();
    PLACEHOLDER_OPENERS.iter().any(|o| {
        inner == *o
            || inner
                .strip_prefix(o)
                .is_some_and(|rest| rest.starts_with([' ', ':']))
    })
}

/// Strip an optional leading list marker (`- `, `* `, `N. `) from a trimmed
/// line, returning the rest (shared by the prompt and placeholder detectors).
fn strip_list_marker(t: &str) -> &str {
    t.strip_prefix("- ")
        .or_else(|| t.strip_prefix("* "))
        .or_else(|| {
            t.split_once(". ")
                .filter(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
                .map(|(_, rest)| rest)
        })
        .unwrap_or(t)
        .trim()
}

/// Top-level (`## `) section names appearing more than once, with their counts,
/// in first-appearance order. Matching is case-insensitive on the heading text.
fn repeated_top_level_sections(body: &str) -> Vec<(String, usize)> {
    let mut seen: Vec<(String, usize)> = Vec::new();
    for line in body.lines() {
        let t = line.trim_start();
        if let Some(name) = t.strip_prefix("## ") {
            let name = name.trim();
            match seen.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
                Some((_, c)) => *c += 1,
                None => seen.push((name.to_string(), 1)),
            }
        }
    }
    seen.retain(|(_, c)| *c > 1);
    seen
}

/// True if `line` is an italic authoring prompt — `_…_` with non-empty inner
/// text — after stripping an optional leading list marker (`- `, `* `, `N. `).
fn is_prompt_line(line: &str) -> bool {
    let t = strip_list_marker(line.trim());
    t.len() >= 2
        && t.starts_with('_')
        && t.ends_with('_')
        && !t[1..t.len() - 1].trim_matches('_').trim().is_empty()
}

/// True if a section's `content` is nothing but its prompt: at least one prompt
/// line and no other (non-blank) content. Empty sections are *not* prompt-only.
/// Shared with `crate::plan`, which treats a prompt-only `## Implementation`
/// section as a replaceable template placeholder (ADR-0008).
pub(crate) fn prompt_only(content: &str) -> bool {
    let mut saw_prompt = false;
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if is_prompt_line(line) {
            saw_prompt = true;
        } else {
            return false;
        }
    }
    saw_prompt
}

/// Split `body` into `(heading_line, content)` pairs — each heading's text runs
/// up to the next heading of any level. Lines before the first heading are
/// dropped (there's no section to attribute them to).
fn sections(body: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in body.lines() {
        if line.trim_start().starts_with('#') {
            out.push((line.trim_start().to_string(), String::new()));
        } else if let Some((_, content)) = out.last_mut() {
            content.push_str(line);
            content.push('\n');
        }
    }
    out
}

fn mech(message: String) -> LintFinding {
    LintFinding {
        source: LintSource::Mechanical,
        severity: Severity::Error,
        message,
    }
}

fn warn(message: String) -> LintFinding {
    LintFinding {
        source: LintSource::Mechanical,
        severity: Severity::Warning,
        message,
    }
}

/// The text under `heading`, up to the next heading of the same-or-higher level.
/// `None` if the heading is absent.
fn section(body: &str, heading: &str) -> Option<String> {
    let level = heading.bytes().take_while(|b| *b == b'#').count();
    let mut lines = body.lines();
    lines.by_ref().find(|l| l.trim() == heading)?;
    let mut content = String::new();
    for line in lines {
        let t = line.trim_start();
        if t.starts_with('#') && t.bytes().take_while(|b| *b == b'#').count() <= level {
            break;
        }
        content.push_str(line);
        content.push('\n');
    }
    Some(content)
}

/// Count `### …` sub-headings in a block — MADR's long form (and most models)
/// record each considered option as its own `###` heading rather than a list
/// item, and both styles record an option.
fn option_headings(block: &str) -> usize {
    block
        .lines()
        .map(str::trim_start)
        .filter(|l| l.starts_with("### "))
        .count()
}

/// Count markdown list items (`- …` or `N. …`) in a block.
fn list_items(block: &str) -> usize {
    block
        .lines()
        .map(str::trim_start)
        .filter(|l| {
            l.starts_with("- ")
                || l.starts_with("* ")
                || l.split_once(". ")
                    .is_some_and(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::adr::Status;
    use crate::naming::{AdrRef, NamingScheme};
    use crate::template::{self, MADR};

    /// The real shipped MADR template, rendered — so this test can't drift from
    /// `template::MADR`'s actual prompts.
    fn fresh_madr() -> String {
        template::render(
            MADR,
            NamingScheme::Sequential,
            &AdrRef::Number(1),
            "X",
            Status::Proposed,
            "2026-01-01",
        )
    }

    const FINISHED: &str = "# ADR-0001: Adopt feature flags\n\n## Status\n\nProposed\n\n\
        ## Context and Problem Statement\n\nWe ship risky changes and want to decouple deploy from release.\n\n\
        ## Considered Options\n\n1. Feature flags\n2. Long-lived branches\n\n\
        ## Decision Outcome\n\nChosen option: feature flags, because they decouple deploy from release.\n\n\
        ### Negative Consequences\n\n- Flag debt accumulates and needs periodic cleanup.\n";

    #[test]
    fn fresh_template_is_flagged_unfinished() {
        let f = lint(&fresh_madr());
        assert!(!f.is_empty());
        assert!(f.iter().all(|x| x.source == LintSource::Mechanical));
        assert!(
            f.iter()
                .any(|x| x.message.contains("still holds only its prompt")),
            "should flag sections left as their prompt, got: {f:?}"
        );
        // Every prose section the template ships a prompt for should be caught.
        assert!(
            f.iter()
                .any(|x| x.message.contains("Context and Problem Statement")),
            "context prompt should be flagged, got: {f:?}"
        );
    }

    #[test]
    fn prompt_only_detects_list_and_prose_prompts() {
        assert!(is_prompt_line("_a prose prompt_"));
        assert!(is_prompt_line("  - _a bulleted prompt_"));
        assert!(is_prompt_line("1. _a numbered prompt_"));
        assert!(!is_prompt_line("- a real bullet"));
        assert!(!is_prompt_line("real prose"));
        assert!(!is_prompt_line("_emphasis_ inside real prose")); // not the whole line
        assert!(prompt_only("\n_just the prompt_\n"));
        assert!(!prompt_only("\nreal content\n"));
        assert!(!prompt_only("\n")); // empty is not prompt-only
    }

    #[test]
    fn finished_adr_is_clean() {
        assert_eq!(lint(FINISHED), Vec::new());
    }

    #[test]
    fn missing_negative_consequences_is_flagged() {
        let body = "## Context and Problem Statement\n\nReal context.\n\n\
            ## Considered Options\n\n1. A real option\n2. Another real option\n\n\
            ## Decision Outcome\n\nWe picked the first one for cost reasons.\n";
        let f = lint(body);
        assert!(
            f.iter()
                .any(|x| x.message.contains("Negative Consequences"))
        );
    }

    #[test]
    fn options_recorded_as_subheadings_are_counted() {
        // MADR's long form (and most models — observed with ollama/llama3.2 in
        // the M5 dogfood rehearsal) record each option as its own `###` heading
        // under `## Considered Options` rather than a list item. Two such
        // headings are two recorded options, not a "fewer than two" finding.
        let body = "## Considered Options\n\n### Option 1: Vault\n\nManaged secrets.\n\n\
            ### Option 2: SOPS\n\nIn-repo encryption.\n\n\
            ## Decision Outcome\n\nChosen: Vault, for the obvious reasons.\n\n\
            ### Negative Consequences\n\n- New infrastructure to run.\n";
        assert_eq!(lint(body), Vec::new());
    }

    #[test]
    fn single_option_is_flagged() {
        let body = "## Considered Options\n\n1. The only option\n\n\
            ## Decision Outcome\n\nPicked it for the obvious reasons.\n\n\
            ### Negative Consequences\n\n- A real downside here.\n";
        let f = lint(body);
        assert!(f.iter().any(|x| x.message.contains("two options")));
    }

    #[test]
    fn negative_consequences_at_h2_is_accepted() {
        // Run-1 regression (iteration-1 full loop): models record the
        // consequences sections at `##` depth where MADR nests them as `###`
        // under `## Decision Outcome`. Both depths are honest documentation —
        // 2 of 11 seeded ADRs failed lint on shape, not substance.
        let body = "## Context and Problem Statement\n\nReal context here.\n\n\
            ## Considered Options\n\n1. A real option\n2. Another real option\n\n\
            ## Decision Outcome\n\nChosen: the first one, because reasons.\n\n\
            ## Positive Consequences\n\n* Faster feedback loops.\n\n\
            ## Negative Consequences\n\n* Initial investment required.\n";
        assert_eq!(lint(body), Vec::new());
    }

    #[test]
    fn empty_h2_negative_consequences_is_flagged() {
        // Depth tolerance must not weaken the honesty check: an empty `##`
        // section is still flagged.
        let body = "## Considered Options\n\n1. A\n2. B\n\n\
            ## Decision Outcome\n\nChosen: A, for reasons.\n\n\
            ## Negative Consequences\n\n## References\n\n- none\n";
        let f = lint(body);
        assert!(
            f.iter().any(|x| x.message.contains("Negative Consequences")
                && x.message.contains("empty")),
            "{f:?}"
        );
    }

    #[test]
    fn repeated_top_level_sections_warn() {
        // Run-1 regression: ADR-0001/0005 carried a duplicated `## Status` /
        // `## Stakeholders` skeleton echo below the ai-suggested marker, and
        // lint was silent. A repeated top-level section is now a Warning
        // finding (visible, but not a CI failure).
        let body = "## Status\n\nProposed\n\n## Stakeholders\n\n- Team\n\n\
            ## Status\nProposed\n\n## Stakeholders\n\n- Team\n\n\
            ## Context and Problem Statement\n\nReal context.\n\n\
            ## Considered Options\n\n1. A\n2. B\n\n\
            ## Decision Outcome\n\nChosen: A, because reasons.\n\n\
            ### Negative Consequences\n\n- A real downside.\n";
        let f = lint(body);
        let warnings: Vec<_> = f
            .iter()
            .filter(|x| x.severity == Severity::Warning)
            .collect();
        assert!(
            warnings.iter().any(|x| x.message.contains("## Status")),
            "{f:?}"
        );
        assert!(
            warnings
                .iter()
                .any(|x| x.message.contains("## Stakeholders")),
            "{f:?}"
        );
        // Nothing else is wrong with this body — every finding is a warning.
        assert!(f.iter().all(|x| x.severity == Severity::Warning), "{f:?}");
    }

    #[test]
    fn bracket_placeholder_lines_warn() {
        // Run-2 regression (iteration-2 full loop, playbook ADR-0010): the
        // model closed the body with "[Insert implementation plan or other
        // details as needed]" — a novel bracket placeholder the template never
        // contained — and lint was silent. A whole-line bracket placeholder is
        // now a Warning finding (visible, but a CI lint gate stays green).
        let body =
            format!("{FINISHED}\n---\n\n[Insert implementation plan or other details as needed]\n");
        let f = lint(&body);
        assert!(
            f.iter().any(|x| x.severity == Severity::Warning
                && x.message.contains("placeholder")
                && x.message
                    .contains("[Insert implementation plan or other details as needed]")),
            "{f:?}"
        );
        // Nothing else is wrong with this body — every finding is a warning.
        assert!(f.iter().all(|x| x.severity == Severity::Warning), "{f:?}");
    }

    #[test]
    fn bracket_placeholders_in_fenced_code_are_not_flagged() {
        // A fenced example legitimately shows where a value goes.
        let body = format!("{FINISHED}\n```yaml\n[Insert API key here]\n```\n");
        assert_eq!(lint(&body), Vec::new());
    }

    #[test]
    fn bracket_placeholder_detection_is_conservative() {
        for line in [
            "[Insert implementation plan or other details as needed]",
            "[Your Name]",
            "[your name]",
            "[TBD]",
            "[TODO: add the rollout diagram]",
            "[To be determined]",
            "[Describe the rollout]",
            "[Name of the approver]",
            "[Optional: include metrics]",
            "- [Insert step]",
            "* [List the stakeholders]",
            "3. [Add a step here]",
            "  [Fill in the dates]",
        ] {
            assert!(bracket_placeholder(line), "should flag {line:?}");
        }
        for line in [
            "- [ ] a real task",                   // checkbox
            "- [x] done task",                     // checked checkbox
            "[ ]",                                 // bare empty checkbox
            "[x]",                                 // bare checked checkbox
            "[1]",                                 // citation
            "[^1]: a footnote definition",         // footnote
            "[MADR](https://adr.github.io/madr/)", // whole-line link
            "[madr]: https://adr.github.io/madr/", // reference definition
            "[the MADR spec][madr]",               // reference-style link
            "![diagram](./diagram.png)",           // image
            "[dependencies]",                      // TOML section: single token
            "[insertion-order]",                   // single token, curated-word prefix
            "[insertion order matters]",           // word boundary: insertion != insert
            "See [above] for details",             // span is not the whole line
            "    [Insert anything]",               // 4-space indented code
            "\t[Insert anything]",                 // tab-indented code
            "",                                    // empty
        ] {
            assert!(!bracket_placeholder(line), "should keep {line:?}");
        }
    }

    #[test]
    fn mechanical_findings_are_errors() {
        // The pre-existing mechanical checks keep gating CI: they are
        // Severity::Error (lint exits non-zero on them; warnings don't gate).
        let f = lint(&fresh_madr());
        assert!(!f.is_empty());
        assert!(f.iter().all(|x| x.severity == Severity::Error), "{f:?}");
    }
}
