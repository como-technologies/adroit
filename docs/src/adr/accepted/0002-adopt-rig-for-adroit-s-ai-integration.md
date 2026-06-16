# ADR-0002: Adopt rig for adroit's AI integration

> State: Accepted

## Status

Accepted

## Stakeholders

- Maintainer — Brett Fowle
- Contributor — @n8behavior

## Context and Problem Statement

*Recorded retroactively: this decision shipped with the AI-authoring work
(issue 5) and was originally recorded as the sample ADR-0001 in the corpus
removed by commit `136a2bc`; see
[ADR-0001](./0001-reinstate-the-in-repo-adr-corpus.md).*

The AI-authoring RFC (issue 5) proposed a family of AI-assisted verbs:
`new --interview`, `draft`, `compose`, `lint --ai`, `plan`, `summarize`, and
`ask`. These need an LLM-provider abstraction (Anthropic + Ollama for phase 1,
more later).

adroit's core CLI is deliberately **synchronous**: `tokio` is otherwise a
`web`-only dependency, and the `forge` feature uses a blocking `ureq` client
*specifically* to keep the core async-free. Any AI layer must deliver provider
abstraction without compromising that invariant.

## Decision Drivers

- Cover both phase-1 providers (Anthropic + Ollama) with minimal bespoke code.
- Preserve the sync-core invariant — `--no-default-features`, `tui`, and
  `forge` builds must never gain `tokio`.
- Keep the provider layer swappable; the Rust LLM ecosystem is young and
  churns.
- Match adroit's existing feature-gating + facade patterns (`forge_hook`, the
  `HttpTransport` test seam).

## Considered Options

1. **Adopt `rig` (`rig-core`)**, gated behind an `ai` Cargo feature that brings
   `tokio`; bridge with a single `block_on` at the CLI boundary; own an
   `AiProvider` facade over rig.
2. **Hand-roll blocking adapters on `ureq`** (mirror `forge`): no async, but
   re-implement provider abstraction and every provider's wire protocol by
   hand.
3. **Hybrid** — `ureq` for simple completions, `rig` only for retrieval-heavy
   verbs.

## Decision Outcome

Chosen: **Option 1 — adopt `rig` behind an `ai` feature**, because `rig-core`
ships native **Anthropic** and **Ollama** providers (both phase-1 targets) plus
embeddings, RAG, and agentic surfaces that later verbs can grow into.
Hand-rolling (option 2) would duplicate a large, maintained surface; the hybrid
(option 3) carries two HTTP stacks for marginal benefit.

The sync-core invariant is preserved because async is confined to the `ai`
feature and bridged with a single `block_on` at the CLI boundary
(`src/ai/rig_provider.rs`, current-thread runtime), so verb handlers stay
synchronous. The always-compiled layer (`src/ai/mod.rs`, `src/ai_hook.rs`)
keeps the `AiProvider` trait, the interview flow, and the `FakeProvider`
testable with no network and no `ai` feature.

How it stands today: `ai` is a **default** feature (so `just lint` / `just
test` cover it), `--no-default-features` still builds the tokio-free core, and
rig's embeddings/RAG surface is deliberately **unused** — retrieval stayed
mechanical (see
[ADR-0009](./0009-defer-embeddings-behind-a-measured-retrieval-miss-criterion.md)).

### Positive Consequences

- Both phase-1 providers come from one maintained dependency, behind one owned
  facade.
- The sync core is untouched; async lands only with the `ai` feature — the
  same bargain `web` already makes.
- The `AiProvider` facade keeps rig types out of verb signatures, so rig stays
  swappable (mirrors how `forge` owns its trait over the HTTP client).
- A single `block_on` boundary keeps verb handlers synchronous and testable;
  `ADROIT_AI_FAKE` + `FakeProvider` make the flows CI-testable offline.

### Negative Consequences

- The `ai` build pulls `tokio` + rig's `reqwest` — heavier deps, and a second
  async stack alongside `web`; now in the **default** build.
- `rig` is 0.x and moves fast; pinning + periodic upgrades are a maintenance
  cost the facade must absorb.
- Two HTTP clients exist across features (`ureq` for `forge`, `reqwest` via
  rig for `ai`); contributors must know which feature owns which.
- The embeddings/RAG capability we partly chose rig for remains unused; we
  carry that surface without exercising it.

## Implementation

Shipped: the `ai` Cargo feature; `src/ai/mod.rs` (trait, `CompletionRequest`,
`AiError`, interview, `FakeProvider`); `src/ai_hook.rs` (always-compiled
facade, no `#[cfg]` in verb handlers); `src/ai/rig_provider.rs` (rig-backed
Anthropic/Ollama); `config::AiConfig` with `ADROIT_AI_*` env overrides and the
credential store for the key. Verbs on this foundation: `new --interview`,
`draft`, `compose`, `plan`, `summarize`, `ask`, `lint --ai`, and the TUI AI
palette.
