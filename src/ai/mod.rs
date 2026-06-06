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
    fn draft_body_wraps_provider_output_with_marker() {
        let fake = FakeProvider {
            canned: "## Context and Problem Statement\n\nBecause reasons.".into(),
        };
        let body = draft_body(&fake, &sample(), &[]).unwrap();
        assert!(body.starts_with(AI_MARKER));
        assert!(body.contains("Because reasons."));
    }
}
