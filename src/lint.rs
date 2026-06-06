//! `adroit lint`: authoring-quality checks on a single ADR's prose — read-only,
//! and distinct from `check` (which validates *structural* repo integrity).
//!
//! The mechanical checks here need no AI: they catch the ways an ADR draft is
//! obviously unfinished — leftover template placeholders, no honest negative
//! consequences, only one option considered. `adroit lint --ai` layers a model
//! review on top (handled in `main`); these stay deterministic so `lint` is
//! usable in CI without a provider. Tuned for the default MADR template.

use serde::Serialize;

/// Where a finding came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, strum::Display)]
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
pub struct LintFinding {
    pub source: LintSource,
    pub message: String,
}

/// The default MADR template's placeholder lines — their presence means the ADR
/// is unfinished.
const PLACEHOLDERS: &[&str] = &[
    "Describe the architectural challenge or decision to be made.",
    "- Driver 1",
    "- Driver 2",
    "1. Option A",
    "2. Option B",
    "Chosen option: **Option A**, because justification.",
    "- Benefit 1",
    "- Trade-off 1",
    "- Role / Person",
    "Notes on how the decision is being carried out.",
];

/// Run the mechanical authoring checks over an ADR body, returning findings
/// (empty = clean). Pure and deterministic — no network, no `ai` feature.
pub fn lint(body: &str) -> Vec<LintFinding> {
    let mut out = Vec::new();

    // 1. Leftover template placeholders → the draft isn't filled in.
    for ph in PLACEHOLDERS {
        if body.contains(ph) {
            out.push(mech(format!(
                "template placeholder still present: \"{ph}\""
            )));
        }
    }

    // 2. Honest negative consequences (people skip these).
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
    if let Some(opts) = section(body, "## Considered Options")
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

    const FRESH_TEMPLATE: &str = "# ADR-0001: X\n\n## Status\n\nProposed\n\n\
        ## Context and Problem Statement\n\nDescribe the architectural challenge or decision to be made.\n\n\
        ## Decision Drivers\n\n- Driver 1\n- Driver 2\n\n\
        ## Considered Options\n\n1. Option A\n2. Option B\n\n\
        ## Decision Outcome\n\nChosen option: **Option A**, because justification.\n\n\
        ### Negative Consequences\n\n- Trade-off 1\n";

    const FINISHED: &str = "# ADR-0001: Adopt feature flags\n\n## Status\n\nProposed\n\n\
        ## Context and Problem Statement\n\nWe ship risky changes and want to decouple deploy from release.\n\n\
        ## Considered Options\n\n1. Feature flags\n2. Long-lived branches\n\n\
        ## Decision Outcome\n\nChosen option: feature flags, because they decouple deploy from release.\n\n\
        ### Negative Consequences\n\n- Flag debt accumulates and needs periodic cleanup.\n";

    #[test]
    fn fresh_template_is_flagged_unfinished() {
        let f = lint(FRESH_TEMPLATE);
        assert!(!f.is_empty());
        assert!(f.iter().all(|x| x.source == LintSource::Mechanical));
        assert!(
            f.iter().any(|x| x.message.contains("placeholder")),
            "should flag leftover placeholders"
        );
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
