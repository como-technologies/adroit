//! Opt-in AI-assisted ADR authoring.
//!
//! [`AiProvider`] is a small **synchronous** trait so the verb handlers stay
//! sync (no `async` in the CLI). The real, network-backed adapter
//! ([`rig_provider`]) is gated behind the `ai` Cargo feature and bridges to
//! rig's async API with a single `block_on` at this boundary — so
//! `--no-default-features`, `tui`, and `forge` never pull in tokio. The trait,
//! the value types, the [`FakeProvider`] stand-in, and the prose-drafting logic
//! are **always compiled**, so the interview flow is unit-testable with no
//! network and no `ai` feature (drive it via the `ADROIT_AI_FAKE` seam in
//! [`crate::ai_hook`]).
//!
//! Determinism guard: AI only ever produces *prose*. Identity, dates, status,
//! and supersession links stay mechanical in the `Store` write path — the
//! interview drafts the body, which a human reviews before commit.

use std::fmt;

#[cfg(feature = "ai")]
pub mod rig_provider;

/// Marker prepended to every AI-suggested region, so a reviewer (and a future
/// `lint`/`check`) can tell what the model wrote from what a human edited.
pub const AI_MARKER: &str = "<!-- adroit:ai-suggested -->";

// Prompt templates live as editable files under `templates/ai/` (compiled in via
// `include_str!`, so the binary stays self-contained). Each verb has a `.system`
// (the model's role/instructions) and a `.prompt` (the per-call template with
// `{{placeholder}}` slots filled by the matching `build_*` below). Trimmed on use
// so a file's trailing newline doesn't change the request. (RFC #5: a future step
// is making these user-overridable, e.g. a repo-local `templates/ai/`.)
const INTERVIEW_SYSTEM: &str = include_str!("../../templates/ai/interview.system.md");
const INTERVIEW_PROMPT: &str = include_str!("../../templates/ai/interview.prompt.md");
const PLAN_SYSTEM: &str = include_str!("../../templates/ai/plan.system.md");
const PLAN_PROMPT: &str = include_str!("../../templates/ai/plan.prompt.md");
const LINT_SYSTEM: &str = include_str!("../../templates/ai/lint.system.md");
const LINT_PROMPT: &str = include_str!("../../templates/ai/lint.prompt.md");
const SUMMARY_SYSTEM: &str = include_str!("../../templates/ai/summary.system.md");
const SUMMARY_PROMPT: &str = include_str!("../../templates/ai/summary.prompt.md");
const ASK_SYSTEM: &str = include_str!("../../templates/ai/ask.system.md");
const ASK_PROMPT: &str = include_str!("../../templates/ai/ask.prompt.md");
const COMPOSE_SYSTEM: &str = include_str!("../../templates/ai/compose.system.md");
const COMPOSE_PROMPT: &str = include_str!("../../templates/ai/compose.prompt.md");

/// Render the empty-or-joined corpus block (the same fallback every prompt uses).
fn corpus_block(corpus: &[String], empty_label: &str) -> String {
    if corpus.is_empty() {
        empty_label.to_string()
    } else {
        corpus.join("\n")
    }
}

/// One completion request. Framework-free, so the trait carries no rig types.
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    /// System / preamble — house style + the drafting instructions.
    pub system: String,
    /// User content — the interview answers + corpus context.
    pub prompt: String,
    /// Upper bound on generated tokens (Anthropic requires it).
    pub max_tokens: u32,
}

impl CompletionRequest {
    /// A rough input-token estimate (~4 chars/token) for the one-line cost notice
    /// shown before a call. Deliberately approximate — it's a heads-up, not billing.
    pub fn estimate_input_tokens(&self) -> usize {
        (self.system.len() + self.prompt.len()).div_ceil(4)
    }
}

/// What can go wrong talking to a provider. Mirrors `forge::ForgeError`'s
/// offline / auth / api split so callers can warn-and-continue vs. surface.
#[derive(Debug)]
pub enum AiError {
    /// No provider is available (feature off, or not configured).
    NotConfigured(String),
    /// Network / connectivity failure.
    Offline(String),
    /// Authentication failed (missing/invalid key).
    Auth(String),
    /// The provider API returned an error.
    Api(String),
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AiError::NotConfigured(m) => write!(f, "AI not configured: {m}"),
            AiError::Offline(m) => write!(f, "AI provider unreachable: {m}"),
            AiError::Auth(m) => write!(f, "AI auth failed (check your key): {m}"),
            AiError::Api(m) => write!(f, "AI API error: {m}"),
        }
    }
}
impl std::error::Error for AiError {}

/// A synchronous LLM completion provider. The real adapter blocks on rig
/// internally; fakes are trivial.
pub trait AiProvider {
    /// Human-readable id for logs/messages (e.g. `anthropic:claude-…`).
    fn id(&self) -> String;
    /// Run one completion, returning the model's text.
    fn complete(&self, req: &CompletionRequest) -> Result<String, AiError>;
}

/// The Socratic-interview answers gathered for `new --interview`.
#[derive(Debug, Clone, Default)]
pub struct Interview {
    pub title: String,
    pub context: String,
    pub drivers: String,
    pub options: String,
    pub risks: String,
}

/// The fixed Socratic questions, in order. Kept here (not in `main`) so the set
/// is testable and overridable later.
pub const INTERVIEW_QUESTIONS: [&str; 4] = [
    "What problem or decision does this ADR address?",
    "What's driving this now — the forces, constraints, or deadlines?",
    "What options are you considering (including ones you'll reject)?",
    "What could go wrong — the risks and negative consequences?",
];

/// Build the completion request from the interview + a corpus summary (existing
/// `reference — title` lines, so the draft matches the team's vocabulary).
pub fn build_request(iv: &Interview, corpus: &[String]) -> CompletionRequest {
    let prompt = INTERVIEW_PROMPT
        .trim()
        .replace("{{title}}", &iv.title)
        .replace(
            "{{corpus_block}}",
            &corpus_block(corpus, "(no existing ADRs yet)"),
        )
        .replace("{{context}}", &iv.context)
        .replace("{{drivers}}", &iv.drivers)
        .replace("{{options}}", &iv.options)
        .replace("{{risks}}", &iv.risks);
    CompletionRequest {
        system: INTERVIEW_SYSTEM.trim().to_string(),
        prompt,
        max_tokens: 1500,
    }
}

/// Draft the ADR body via the provider, tagged with [`AI_MARKER`]. The caller
/// writes it through `Store::set_body` (which preserves the mechanical heading /
/// status), then opens the editor for review.
pub fn draft_body(
    provider: &dyn AiProvider,
    iv: &Interview,
    corpus: &[String],
) -> Result<String, AiError> {
    let req = build_request(iv, corpus);
    let body = sanitize_draft(&provider.complete(&req)?);
    Ok(format!("{AI_MARKER}\n\n{}\n", body.trim()))
}

/// Mechanically sanitize a model-returned draft body before it is marked and
/// spliced (shared by `draft_body` + `draft_compose`, so it covers `new
/// --interview` / `draft` / `compose` / `import --ai` and the TUI assists). The
/// prompts forbid every one of these shapes, but small local models re-emit
/// them anyway (observed with ollama/llama3.2 in the M5 dogfood rehearsal and
/// the iteration-1 full-loop run):
///
/// - a **leading H1** duplicates the mechanical identity heading the splice
///   preserves — dropped (a later `# ` heading is the model's own prose and
///   stays); leading `> State:` banner lines are the same identity echo;
/// - a **skeleton echo** — a `## Status` / `## Stakeholders` section re-emitted
///   from the seed template (run-1 ADR-0001/0005) — always duplicates the
///   mechanical preamble the splice preserves, so the whole section is dropped
///   wherever it appears (AI owns the prose sections, never the preamble);
/// - **echoed adroit markers** (`<!-- adroit:ai-suggested -->` /
///   `<!-- adroit:seeded-from-assessment -->`) are metadata the wrapper/seed
///   path owns — dropped;
/// - **trailing conversational residue** ("Please review this revised ADR
///   body…", run-1 ADR-0002) — recognized closer paragraphs are stripped, plus
///   the horizontal rule such a closer orphans;
/// - a bare **`## Implementation` heading** with real content would read as a
///   hand-written section and block `plan --save` forever (ADR-0008 reserves
///   that heading for the adroit-managed plan) — retitled to
///   `## Implementation notes`, content kept. Two echoes stay verbatim: a
///   heading that opens a marker-bracketed plan span (a stored plan echoed
///   back) is managed content, and an empty / prompt-only section (the
///   template placeholder echoed back) never blocks — `plan --save` replaces
///   it in place, and retitling it would strand a prompt-only "notes" section.
///
/// Everything inside a marker-bracketed plan span is adroit-managed and stays
/// verbatim.
fn sanitize_draft(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    let mut in_plan_span = false;
    let mut kept_content = false; // a non-empty line has survived (past the head)
    let mut dropped_h1 = false;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim();
        if t == crate::plan::PLAN_MARKER {
            in_plan_span = true;
        } else if t == crate::plan::PLAN_END_MARKER {
            in_plan_span = false;
        }
        if !in_plan_span {
            // Echoed adroit markers are metadata noise wherever they appear.
            if t == AI_MARKER || t == crate::import::SEED_MARKER {
                i += 1;
                continue;
            }
            // Leading identity echoes: the first H1 and any `> State:` banner
            // before real content begins.
            if !kept_content {
                if !dropped_h1 && t.starts_with("# ") {
                    dropped_h1 = true;
                    i += 1;
                    continue;
                }
                if t.starts_with("> State:") {
                    i += 1;
                    continue;
                }
            }
            // Skeleton-echo sections: the splice preserves the document's own
            // `## Status` / `## Stakeholders`, so a model-emitted one is always
            // a duplicate — drop the heading and its content (up to the next
            // heading or a plan-span marker).
            if t.eq_ignore_ascii_case("## Status") || t.eq_ignore_ascii_case("## Stakeholders") {
                i += 1;
                while i < lines.len()
                    && !lines[i].trim_start().starts_with('#')
                    && lines[i].trim() != crate::plan::PLAN_MARKER
                {
                    i += 1;
                }
                continue;
            }
            // The managed span's own heading sits just above the begin marker.
            let heads_plan_span = || {
                lines[i + 1..]
                    .iter()
                    .map(|l| l.trim())
                    .find(|l| !l.is_empty())
                    == Some(crate::plan::PLAN_MARKER)
            };
            // The section's content: up to the next `## ` heading or the end.
            let blocking_content = || {
                let n = lines[i + 1..]
                    .iter()
                    .position(|l| l.trim_start().starts_with("## "))
                    .unwrap_or(lines.len() - i - 1);
                let content = lines[i + 1..i + 1 + n].join("\n");
                !(content.trim().is_empty() || crate::lint::prompt_only(&content))
            };
            if t.eq_ignore_ascii_case("## Implementation")
                && !heads_plan_span()
                && blocking_content()
            {
                out.push("## Implementation notes");
                kept_content = true;
                i += 1;
                continue;
            }
        }
        if !t.is_empty() {
            kept_content = true;
        }
        out.push(line);
        i += 1;
    }
    strip_trailing_residue(&mut out);
    out.join("\n")
}

/// Conversational-closer openings (matched case-insensitively against the first
/// line of the draft's final paragraph). Curated from observed model output —
/// run-1's "Please review this revised ADR body for clarity and accuracy."
/// plus the classic chat sign-offs.
const RESIDUE_OPENERS: [&str; 8] = [
    "please review",
    "please let me know",
    "let me know",
    "i hope this",
    "hope this helps",
    "feel free to",
    "if you have any questions",
    "is there anything else",
];

/// Strip trailing conversational residue from a sanitized draft: drop final
/// paragraphs that are plain prose opening with a recognized closer, then any
/// horizontal rule the strip orphaned. Headings, lists, quotes, code, and
/// comments are never residue; an unrecognized final paragraph stays.
fn strip_trailing_residue(lines: &mut Vec<&str>) {
    let mut stripped = false;
    loop {
        while lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.pop();
        }
        // The final paragraph: the contiguous non-blank tail.
        let start = lines
            .iter()
            .rposition(|l| l.trim().is_empty())
            .map_or(0, |b| b + 1);
        if start >= lines.len() {
            break;
        }
        let para = &lines[start..];
        let plain_prose = para.iter().all(|l| {
            let t = l.trim_start();
            !(t.starts_with('#')
                || t.starts_with('-')
                || t.starts_with('*')
                || t.starts_with('>')
                || t.starts_with('`')
                || t.starts_with('|')
                || t.starts_with("<!--")
                || t.split_once(". ")
                    .is_some_and(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())))
        });
        let first = para[0].trim().to_lowercase();
        if plain_prose && RESIDUE_OPENERS.iter().any(|o| first.starts_with(o)) {
            lines.truncate(start);
            stripped = true;
            continue;
        }
        // A horizontal rule orphaned by a stripped closer is residue too; a
        // rule with real content after it (no strip happened) stays.
        let is_rule = para.len() == 1 && matches!(para[0].trim(), "---" | "***" | "___");
        if stripped && is_rule {
            lines.truncate(start);
            continue;
        }
        break;
    }
}

/// Build the completion request for `plan`: a concrete implementation plan for
/// an (accepted) ADR, grounded in its body + the corpus.
pub fn build_plan_request(title: &str, adr_body: &str, corpus: &[String]) -> CompletionRequest {
    let prompt = PLAN_PROMPT
        .trim()
        .replace("{{title}}", title)
        .replace("{{adr_body}}", adr_body)
        .replace("{{corpus_block}}", &corpus_block(corpus, "(no other ADRs)"));
    CompletionRequest {
        system: PLAN_SYSTEM.trim().to_string(),
        prompt,
        max_tokens: 1800,
    }
}

/// Draft an implementation plan via the provider. Read-only — the ADR is input,
/// never modified.
pub fn draft_plan(
    provider: &dyn AiProvider,
    title: &str,
    adr_body: &str,
    corpus: &[String],
) -> Result<String, AiError> {
    provider.complete(&build_plan_request(title, adr_body, corpus))
}

/// Build the completion request for `lint --ai`: a best-practices review of an
/// ADR draft against the house style.
pub fn build_lint_request(title: &str, adr_body: &str, corpus: &[String]) -> CompletionRequest {
    let prompt = LINT_PROMPT
        .trim()
        .replace("{{title}}", title)
        .replace("{{adr_body}}", adr_body)
        .replace("{{corpus_block}}", &corpus_block(corpus, "(no other ADRs)"));
    CompletionRequest {
        system: LINT_SYSTEM.trim().to_string(),
        prompt,
        max_tokens: 1000,
    }
}

/// Run an AI review of an ADR draft via the provider. Read-only.
pub fn draft_lint(
    provider: &dyn AiProvider,
    title: &str,
    adr_body: &str,
    corpus: &[String],
) -> Result<String, AiError> {
    provider.complete(&build_lint_request(title, adr_body, corpus))
}

/// Build the completion request for `summarize`: a one-paragraph TL;DR of an ADR.
pub fn build_summary_request(title: &str, adr_body: &str) -> CompletionRequest {
    let prompt = SUMMARY_PROMPT
        .trim()
        .replace("{{title}}", title)
        .replace("{{adr_body}}", adr_body);
    CompletionRequest {
        system: SUMMARY_SYSTEM.trim().to_string(),
        prompt,
        max_tokens: 400,
    }
}

/// Summarize an ADR via the provider. Read-only.
pub fn draft_summary(
    provider: &dyn AiProvider,
    title: &str,
    adr_body: &str,
) -> Result<String, AiError> {
    provider.complete(&build_summary_request(title, adr_body))
}

/// Build the completion request for `ask`: answer a question grounded ONLY in the
/// retrieved ADR excerpts, with citations.
pub fn build_ask_request(question: &str, context: &str) -> CompletionRequest {
    let prompt = ASK_PROMPT
        .trim()
        .replace("{{question}}", question)
        .replace("{{context}}", context);
    CompletionRequest {
        system: ASK_SYSTEM.trim().to_string(),
        prompt,
        max_tokens: 800,
    }
}

/// Answer a corpus question via the provider (the retrieval is done by the
/// caller). Read-only.
pub fn draft_ask(
    provider: &dyn AiProvider,
    question: &str,
    context: &str,
) -> Result<String, AiError> {
    provider.complete(&build_ask_request(question, context))
}

/// Build the completion request for `compose`: instruction-driven (re)drafting of
/// an ADR body. Given the current body + a free-form instruction + corpus, the
/// model returns a complete revised body. This powers the TUI's "AI draft" assist
/// (the free-form prompt box); like the interview it produces **prose only** — the
/// caller writes it through `Store::set_body`, so identity/status stay mechanical.
pub fn build_compose_request(
    title: &str,
    instruction: &str,
    current_body: &str,
    corpus: &[String],
) -> CompletionRequest {
    let prompt = COMPOSE_PROMPT
        .trim()
        .replace("{{title}}", title)
        .replace("{{corpus_block}}", &corpus_block(corpus, "(no other ADRs)"))
        .replace("{{body}}", current_body)
        .replace("{{instruction}}", instruction);
    CompletionRequest {
        system: COMPOSE_SYSTEM.trim().to_string(),
        prompt,
        max_tokens: 1800,
    }
}

/// (Re)draft an ADR body from a free-form instruction via the provider, tagged
/// with [`AI_MARKER`]. The caller loads it into the editor for review before
/// saving through `Store::set_body`.
pub fn draft_compose(
    provider: &dyn AiProvider,
    title: &str,
    instruction: &str,
    current_body: &str,
    corpus: &[String],
) -> Result<String, AiError> {
    let req = build_compose_request(title, instruction, current_body, corpus);
    let body = sanitize_draft(&provider.complete(&req)?);
    Ok(format!("{AI_MARKER}\n\n{}\n", body.trim()))
}

/// An offline provider for tests and the `ADROIT_AI_FAKE` seam: echoes a canned
/// response so the interview flow runs end-to-end with no network.
pub struct FakeProvider {
    pub canned: String,
}
impl AiProvider for FakeProvider {
    fn id(&self) -> String {
        "fake".to_string()
    }
    fn complete(&self, _req: &CompletionRequest) -> Result<String, AiError> {
        // Test hook: a canned value of `__ERROR__` simulates a provider failure
        // (e.g. an API credit/network error) so the degrade paths are testable.
        if self.canned == "__ERROR__" {
            return Err(AiError::Api("simulated provider failure".into()));
        }
        Ok(self.canned.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Interview {
        Interview {
            title: "Adopt feature flags".into(),
            context: "we ship risky changes".into(),
            drivers: "decouple deploy from release".into(),
            options: "LaunchDarkly vs homegrown".into(),
            risks: "flag debt".into(),
        }
    }

    #[test]
    fn request_includes_every_answer_and_the_corpus() {
        let req = build_request(&sample(), &["ADR-0001 — Use Postgres".to_string()]);
        for needle in [
            "Adopt feature flags",
            "we ship risky changes",
            "decouple deploy from release",
            "LaunchDarkly vs homegrown",
            "flag debt",
            "ADR-0001 — Use Postgres",
        ] {
            assert!(req.prompt.contains(needle), "prompt missing {needle:?}");
        }
        assert!(req.system.contains("Architecture Decision Record"));
        assert!(req.max_tokens > 0);
    }

    #[test]
    fn empty_corpus_is_labeled_not_blank() {
        let req = build_request(&sample(), &[]);
        assert!(req.prompt.contains("(no existing ADRs yet)"));
    }

    #[test]
    fn plan_request_includes_the_adr_body_and_corpus() {
        let req = build_plan_request(
            "Adopt feature flags",
            "## Decision Outcome\n\nUse LaunchDarkly.",
            &["ADR-0002 — Use Postgres".to_string()],
        );
        assert!(req.prompt.contains("Adopt feature flags"));
        assert!(req.prompt.contains("Use LaunchDarkly."));
        assert!(req.prompt.contains("ADR-0002 — Use Postgres"));
        assert!(req.system.contains("implementation plan"));
    }

    #[test]
    fn draft_plan_returns_provider_output_unwrapped() {
        let fake = FakeProvider {
            canned: "- [ ] Step one".into(),
        };
        let plan = draft_plan(&fake, "T", "body", &[]).unwrap();
        assert_eq!(plan, "- [ ] Step one");
    }

    #[test]
    fn summary_request_is_a_one_paragraph_instruction() {
        let req = build_summary_request("Adopt rig", "## Decision Outcome\n\nUse rig.");
        assert!(req.prompt.contains("Adopt rig"));
        assert!(req.prompt.contains("Use rig."));
        assert!(req.system.to_lowercase().contains("one"));
    }

    #[test]
    fn ask_request_grounds_on_context_and_demands_citations() {
        let req = build_ask_request("Why Postgres?", "### ADR-0001 — Use Postgres\nrelational.");
        assert!(req.prompt.contains("Why Postgres?"));
        assert!(req.prompt.contains("ADR-0001"));
        assert!(req.system.to_lowercase().contains("cite"));
    }

    #[test]
    fn compose_request_includes_instruction_body_and_corpus() {
        let req = build_compose_request(
            "Adopt rig",
            "Expand the negative consequences.",
            "## Decision Outcome\n\nUse rig.",
            &["ADR-0001 — Use Postgres".to_string()],
        );
        assert!(req.prompt.contains("Adopt rig"));
        assert!(req.prompt.contains("Expand the negative consequences."));
        assert!(req.prompt.contains("Use rig."));
        assert!(req.prompt.contains("ADR-0001 — Use Postgres"));
        assert!(req.system.to_lowercase().contains("complete revised body"));
    }

    #[test]
    fn draft_compose_wraps_provider_output_with_marker() {
        let fake = FakeProvider {
            canned: "## Context and Problem Statement\n\nRevised.".into(),
        };
        let body = draft_compose(&fake, "T", "tighten it", "old body", &[]).unwrap();
        assert!(body.starts_with(AI_MARKER));
        assert!(body.contains("Revised."));
    }

    #[test]
    fn draft_body_wraps_provider_output_with_marker() {
        let fake = FakeProvider {
            canned: "## Context and Problem Statement\n\nBecause reasons.".into(),
        };
        let body = draft_body(&fake, &sample(), &[]).unwrap();
        assert!(body.starts_with(AI_MARKER));
        assert!(body.contains("Because reasons."));
    }

    #[test]
    fn drafts_drop_a_reemitted_leading_h1() {
        // The prompts forbid the title H1, but small local models re-emit it
        // anyway (observed with ollama/llama3.2 in the M5 dogfood rehearsal);
        // the splice preserves the mechanical heading, so an un-dropped H1
        // would duplicate it inside the body. Only the draft's *leading* H1 is
        // identity noise — a later `# ` heading is the model's own prose.
        let fake = FakeProvider {
            canned: "# ADR-0007: Adopt feature flags\n\n\
                     ## Context and Problem Statement\n\nReal.\n\n# Appendix\n\nKept."
                .into(),
        };
        let body = draft_compose(&fake, "T", "flesh it out", "old", &[]).unwrap();
        assert!(!body.contains("# ADR-0007"), "{body}");
        assert!(body.contains("## Context and Problem Statement"), "{body}");
        assert!(body.contains("# Appendix"), "{body}");
    }

    #[test]
    fn drafts_retitle_an_unmanaged_implementation_heading() {
        // ADR-0008 reserves `## Implementation` for the adroit-managed plan; a
        // model-emitted bare one would read as hand-written and block
        // `plan --save` forever. Sanitized at the draft seam: retitled, content
        // kept. Covers `draft_body` (interview) — `draft_compose` shares the
        // sanitizer.
        let fake = FakeProvider {
            canned: "## Decision Outcome\n\nChosen.\n\n## Implementation\n\n1. Step one.".into(),
        };
        let body = draft_body(&fake, &sample(), &[]).unwrap();
        assert!(
            body.contains("## Implementation notes\n\n1. Step one."),
            "{body}"
        );
        assert!(
            !body.lines().any(|l| l.trim() == "## Implementation"),
            "{body}"
        );
    }

    #[test]
    fn drafts_keep_a_replaceable_implementation_placeholder() {
        // A model echoing the template's prompt-only placeholder section is
        // not squatting on the plan seam — `plan --save` replaces it in place,
        // so it stays untouched (retitling it would strand a prompt-only
        // "notes" section behind the later-appended managed plan).
        let fake = FakeProvider {
            canned: "## Decision Outcome\n\nChosen.\n\n## Implementation\n\n\
                     _Draft it later with `adroit plan`._"
                .into(),
        };
        let body = draft_compose(&fake, "T", "i", "old", &[]).unwrap();
        assert!(
            body.contains("## Implementation\n\n_Draft it later"),
            "{body}"
        );
        assert!(!body.contains("## Implementation notes"), "{body}");
    }

    #[test]
    fn drafts_strip_trailing_conversational_residue() {
        // Run-1 regression (iteration-1 learnings; import --ai, ADR-0002): the
        // model closed the body with a horizontal rule and "Please review this
        // revised ADR body for clarity and accuracy." — chat residue, not ADR
        // content. The sanitizer strips the trailing pleasantry and the rule it
        // orphans.
        let fake = FakeProvider {
            canned: "## Decision Outcome\n\nChosen: Jenkins.\n\n\
                     ## Implementation notes\n\n1. Set it up.\n\n---\n\n\
                     Please review this revised ADR body for clarity and accuracy."
                .into(),
        };
        let body = draft_compose(&fake, "T", "i", "old", &[]).unwrap();
        assert!(!body.contains("Please review this revised"), "{body}");
        assert!(!body.trim_end().ends_with("---"), "{body}");
        assert!(body.contains("1. Set it up."), "{body}");
    }

    #[test]
    fn drafts_strip_a_bare_trailing_pleasantry() {
        // The closer also appears without a rule ("Let me know …").
        let fake = FakeProvider {
            canned: "## Decision Outcome\n\nChosen.\n\n\
                     Let me know if you'd like any adjustments!"
                .into(),
        };
        let body = draft_compose(&fake, "T", "i", "old", &[]).unwrap();
        assert!(!body.contains("Let me know"), "{body}");
        assert!(body.contains("Chosen."), "{body}");
    }

    #[test]
    fn drafts_keep_real_trailing_content() {
        // A real final paragraph (and trailing list items) must survive — only
        // recognized conversational closers are residue.
        let fake = FakeProvider {
            canned: "## Decision Outcome\n\nChosen.\n\n\
                     ### Negative Consequences\n\n- Please review changes carefully \
                     before merging.\n\nBy following this plan, we reduce risk."
                .into(),
        };
        let body = draft_compose(&fake, "T", "i", "old", &[]).unwrap();
        assert!(body.contains("Please review changes carefully"), "{body}");
        assert!(body.contains("By following this plan"), "{body}");
    }

    #[test]
    fn drafts_drop_a_status_stakeholders_skeleton_echo() {
        // Run-1 regression (import --ai, ADR-0001): llama3.2 reproduced the seed
        // skeleton — a second `## Status` / `## Stakeholders` block plus the
        // seeded-from-assessment marker — below the ai-suggested marker instead
        // of replacing it. The splice preserves the document's own mechanical
        // preamble, so any such block in a draft is a duplicate: dropped.
        let fake = FakeProvider {
            canned: "## Status\nProposed\n\n## Stakeholders\n\
                     _Who owns this decision, and who needs to sign off?_\n\n\
                     <!-- adroit:seeded-from-assessment -->\n\n\
                     ## Context and Problem Statement\n\nReal context.\n"
                .into(),
        };
        let body = draft_body(&fake, &sample(), &[]).unwrap();
        assert!(
            !body
                .lines()
                .any(|l| l.trim().eq_ignore_ascii_case("## Status")),
            "{body}"
        );
        assert!(!body.contains("## Stakeholders"), "{body}");
        assert!(!body.contains("seeded-from-assessment"), "{body}");
        assert!(body.contains("## Context and Problem Statement"), "{body}");
        assert!(body.contains("Real context."), "{body}");
    }

    #[test]
    fn drafts_drop_a_banner_and_stakeholders_echo() {
        // Run-1 regression (import --ai, ADR-0005): the echo led with a
        // `> State:` banner and a content-bearing `## Stakeholders` block. Both
        // duplicate the preserved preamble — dropped, content and all.
        let fake = FakeProvider {
            canned: "> State: Proposed\n\n## Stakeholders\n\n\
                     * Roles: Team Lead, Engineering Manager\n\n\
                     ## Context and Problem Statement\n\nReal context.\n"
                .into(),
        };
        let body = draft_compose(&fake, "T", "i", "old", &[]).unwrap();
        assert!(!body.contains("> State:"), "{body}");
        assert!(!body.contains("## Stakeholders"), "{body}");
        assert!(!body.contains("Team Lead"), "{body}");
        assert!(body.contains("Real context."), "{body}");
    }

    #[test]
    fn drafts_drop_an_echoed_ai_marker() {
        // A re-emitted `<!-- adroit:ai-suggested -->` would duplicate the one
        // marker the draft wrapper prepends.
        let fake = FakeProvider {
            canned: format!("{AI_MARKER}\n\n## Context and Problem Statement\n\nReal."),
        };
        let body = draft_compose(&fake, "T", "i", "old", &[]).unwrap();
        assert_eq!(
            body.matches(AI_MARKER).count(),
            1,
            "exactly the wrapper's marker: {body}"
        );
        assert!(body.contains("Real."), "{body}");
    }

    #[test]
    fn drafts_keep_skeleton_lookalikes_inside_a_plan_span() {
        // Marker-bracketed plan content is adroit-managed and stays verbatim —
        // even when it contains lines that look like skeleton echoes.
        let canned = format!(
            "## Decision Outcome\n\nChosen.\n\n## Implementation\n\n{}\n\n\
             ## Status\n\ntracked in the plan\n\n{}",
            crate::plan::PLAN_MARKER,
            crate::plan::PLAN_END_MARKER
        );
        let fake = FakeProvider { canned };
        let body = draft_compose(&fake, "T", "i", "old", &[]).unwrap();
        assert!(body.contains("## Status\n\ntracked in the plan"), "{body}");
    }

    #[test]
    fn drafts_leave_a_marker_bracketed_plan_span_verbatim() {
        // A model echoing back a body that carries the stored plan section
        // (ADR-0008) must not have the *managed* `## Implementation` heading
        // renamed — only an unmanaged one is squatting on the plan seam.
        let canned = format!(
            "## Decision Outcome\n\nChosen.\n\n## Implementation\n\n{}\n\n1. Step.\n\n{}",
            crate::plan::PLAN_MARKER,
            crate::plan::PLAN_END_MARKER
        );
        let fake = FakeProvider { canned };
        let body = draft_compose(&fake, "T", "i", "old", &[]).unwrap();
        assert!(
            body.contains(&format!(
                "## Implementation\n\n{}",
                crate::plan::PLAN_MARKER
            )),
            "{body}"
        );
        assert!(!body.contains("## Implementation notes"), "{body}");
    }
}
