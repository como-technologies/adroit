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

/// One authoring-quality finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "manifest", derive(schemars::JsonSchema))]
pub struct LintFinding {
    pub source: LintSource,
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
    match section(body, "### Negative Consequences") {
        None => out.push(mech(
            "no `### Negative Consequences` section — document the trade-offs honestly".into(),
        )),
        Some(c) if c.trim().is_empty() => out.push(mech(
            "`### Negative Consequences` is empty — every decision has downsides; name them".into(),
        )),
        _ => {}
    }

    // 3. More than one option considered (record the alternatives you rejected).
    //    Skip while the section is still the prompt — that's covered by (1).
    if let Some(opts) = section(body, "## Considered Options")
        && !prompt_only(&opts)
        && list_items(&opts) < 2
    {
        out.push(mech(
            "fewer than two options under `## Considered Options` — record the alternatives \
             you weighed and rejected"
                .into(),
        ));
    }

    out
}

/// True if `line` is an italic authoring prompt — `_…_` with non-empty inner
/// text — after stripping an optional leading list marker (`- `, `* `, `N. `).
fn is_prompt_line(line: &str) -> bool {
    let t = line.trim();
    let t = t
        .strip_prefix("- ")
        .or_else(|| t.strip_prefix("* "))
        .or_else(|| {
            t.split_once(". ")
                .filter(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
                .map(|(_, rest)| rest)
        })
        .unwrap_or(t)
        .trim();
    t.len() >= 2
        && t.starts_with('_')
        && t.ends_with('_')
        && !t[1..t.len() - 1].trim_matches('_').trim().is_empty()
}

/// True if a section's `content` is nothing but its prompt: at least one prompt
/// line and no other (non-blank) content. Empty sections are *not* prompt-only.
fn prompt_only(content: &str) -> bool {
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
    fn single_option_is_flagged() {
        let body = "## Considered Options\n\n1. The only option\n\n\
            ## Decision Outcome\n\nPicked it for the obvious reasons.\n\n\
            ### Negative Consequences\n\n- A real downside here.\n";
        let f = lint(body);
        assert!(f.iter().any(|x| x.message.contains("two options")));
    }
}
