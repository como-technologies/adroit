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
    let corpus_block = if corpus.is_empty() {
        "(no existing ADRs yet)".to_string()
    } else {
        corpus.join("\n")
    };
    let system = "You are an experienced architect helping draft an Architecture \
        Decision Record (ADR). Match the house voice of the existing ADRs. Write \
        crisp, concrete prose with no filler, and be honest about negative \
        consequences. Output GitHub-flavored markdown for the ADR body only: the \
        sections `## Context and Problem Statement`, `## Decision Drivers`, \
        `## Considered Options`, and `## Decision Outcome` (with \
        `### Positive Consequences` and `### Negative Consequences`). Do NOT \
        write the title H1 or a `## Status` section — those are added mechanically."
        .to_string();
    let prompt = format!(
        "Title: {title}\n\nExisting ADRs (for voice + prior decisions):\n{corpus_block}\n\n\
         Author's notes from a short interview:\n\
         - Problem / context: {context}\n\
         - Drivers: {drivers}\n\
         - Options considered: {options}\n\
         - Risks / negative consequences: {risks}\n\n\
         Draft the ADR body now.",
        title = iv.title,
        context = iv.context,
        drivers = iv.drivers,
        options = iv.options,
        risks = iv.risks,
    );
    CompletionRequest {
        system,
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
    let body = provider.complete(&req)?;
    Ok(format!("{AI_MARKER}\n\n{}\n", body.trim()))
}

/// Build the request for `draft <ID>`: complete an **existing** ADR body (a
/// template or partial draft) — no interview, just build on what's there.
pub fn build_fill_request(title: &str, current_body: &str, corpus: &[String]) -> CompletionRequest {
    let corpus_block = if corpus.is_empty() {
        "(no other ADRs)".to_string()
    } else {
        corpus.join("\n")
    };
    let system = "You are completing an Architecture Decision Record draft. The user has \
        a template or partially-filled ADR; produce the finished prose, KEEPING any real \
        content they already wrote and replacing the template placeholders. Match the \
        house voice of the existing ADRs and write honest negative consequences. Output \
        GitHub-flavored markdown for the body only: the sections `## Context and Problem \
        Statement`, `## Decision Drivers`, `## Considered Options`, and `## Decision \
        Outcome` (with `### Positive Consequences` and `### Negative Consequences`). Do \
        NOT write the title H1 or a `## Status` section — those are mechanical."
        .to_string();
    let prompt = format!(
        "ADR title: {title}\n\nCurrent ADR body (template or partial draft):\n{current_body}\n\n\
         Existing ADRs (for voice + prior decisions):\n{corpus_block}\n\n\
         Write the completed ADR body now."
    );
    CompletionRequest {
        system,
        prompt,
        max_tokens: 1500,
    }
}

/// Complete an existing ADR body via the provider, tagged with [`AI_MARKER`].
pub fn draft_fill(
    provider: &dyn AiProvider,
    title: &str,
    current_body: &str,
    corpus: &[String],
) -> Result<String, AiError> {
    let body = provider.complete(&build_fill_request(title, current_body, corpus))?;
    Ok(format!("{AI_MARKER}\n\n{}\n", body.trim()))
}

/// Build the completion request for `plan`: a concrete implementation plan for
/// an (accepted) ADR, grounded in its body + the corpus.
pub fn build_plan_request(title: &str, adr_body: &str, corpus: &[String]) -> CompletionRequest {
    let corpus_block = if corpus.is_empty() {
        "(no other ADRs)".to_string()
    } else {
        corpus.join("\n")
    };
    let system = "You are a senior engineer turning an accepted Architecture \
        Decision Record into a concrete implementation plan. Produce an ordered, \
        actionable markdown checklist: the steps to implement the decision, the \
        components/files likely touched, testing, rollout/migration, and the risks \
        to watch. Be specific and concise, reference the decision, and don't \
        restate the whole ADR. Output markdown only."
        .to_string();
    let prompt = format!(
        "ADR title: {title}\n\nADR body:\n{adr_body}\n\nOther ADRs (for context):\n\
         {corpus_block}\n\nWrite the implementation plan now."
    );
    CompletionRequest {
        system,
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
    let corpus_block = if corpus.is_empty() {
        "(no other ADRs)".to_string()
    } else {
        corpus.join("\n")
    };
    let system = "You are reviewing an Architecture Decision Record draft against ADR \
        best practices and the team's house style (inferred from the other ADRs). \
        Report concrete, actionable issues as a short markdown bullet list: weak or \
        missing alternatives, hand-wavy decision rationale, performative or missing \
        negative consequences, vague drivers, undefined acronyms, inconsistent voice. \
        If it's solid, say so in one line. Be specific and terse — no preamble."
        .to_string();
    let prompt = format!(
        "ADR title: {title}\n\nADR body:\n{adr_body}\n\nOther ADRs (house voice):\n\
         {corpus_block}\n\nReview the draft now."
    );
    CompletionRequest {
        system,
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
    let system = "Summarize this Architecture Decision Record in ONE tight paragraph \
        (about 2-4 sentences): what was decided and why, in plain language. No \
        heading, no preamble, no bullet list — just the paragraph, in a neutral, \
        factual voice suitable for a PR description or a decision-log entry."
        .to_string();
    let prompt = format!(
        "ADR title: {title}\n\nADR body:\n{adr_body}\n\nWrite the one-paragraph summary now."
    );
    CompletionRequest {
        system,
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
    let system = "Answer the question using ONLY the provided ADR excerpts. Cite the \
        ADRs you used by their reference (e.g. ADR-0006). If the excerpts don't \
        contain the answer, say so plainly rather than guessing. Be concise."
        .to_string();
    let prompt = format!(
        "Question: {question}\n\nRelevant ADR excerpts:\n{context}\n\nAnswer with citations."
    );
    CompletionRequest {
        system,
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
    fn fill_request_uses_the_current_body() {
        let req = build_fill_request("T", "## Context\n\nDescribe the challenge.", &[]);
        assert!(req.prompt.contains("Describe the challenge."));
        assert!(req.system.to_lowercase().contains("completing"));
    }

    #[test]
    fn ask_request_grounds_on_context_and_demands_citations() {
        let req = build_ask_request("Why Postgres?", "### ADR-0001 — Use Postgres\nrelational.");
        assert!(req.prompt.contains("Why Postgres?"));
        assert!(req.prompt.contains("ADR-0001"));
        assert!(req.system.to_lowercase().contains("cite"));
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
}
