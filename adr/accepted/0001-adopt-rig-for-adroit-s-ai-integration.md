# ADR-0001: Adopt rig for adroit's AI integration

> State: Accepted

## Status

Accepted

## Stakeholders

- Maintainer — Brett Fowle
- Contributor — @n8behavior

## Context and Problem Statement

The AI-authoring RFC (issue 5) proposes a family of AI-assisted verbs: `new --ai`
/ `new --interview`, `lint`, `plan`, `related`, and `dedupe`. These need an
LLM-provider abstraction (Anthropic + Ollama for phase 1, more later) and — for
the retrieval-grounded verbs — embeddings and RAG over the ADR corpus.

adroit's core CLI is deliberately **synchronous**: `tokio` is a `web`-only
dependency, and the `forge` feature uses a blocking `ureq` client *specifically*
to keep the core async-free. Any AI layer must deliver provider abstraction +
RAG without compromising that invariant.

## Decision Drivers

- Cover both phase-1 providers (Anthropic + Ollama) with minimal bespoke code.
- Provide embeddings + RAG for `plan` / `related` / `dedupe` without re-inventing
  a vector / retrieval stack.
- Preserve the sync-core invariant — `--no-default-features`, `tui`, and `forge`
  must never gain `tokio`.
- Keep the provider layer swappable; the Rust LLM ecosystem is young and churns.
- Match adroit's existing feature-gating + facade patterns (`forge_hook`, the
  `HttpTransport` test seam).

## Considered Options

1. **Adopt `rig` (`rig-core`)**, gated behind an `ai` Cargo feature that brings
   `tokio`; bridge with a single `block_on` at the CLI boundary; own an
   `AiProvider` facade over rig.
2. **Hand-roll blocking adapters on `ureq`** (mirror `forge`): no async, but
   re-implement provider abstraction, embeddings, and RAG by hand.
3. **Hybrid** — `ureq` for simple completions, `rig` only for the RAG-heavy verbs.

## Decision Outcome

Chosen option: **Option 1 — adopt `rig` behind an `ai` feature.** `rig-core`
(pinned at 0.38) ships native **Anthropic** and **Ollama** providers (both phase-1 targets)
plus embeddings, a vector-store abstraction, RAG, and agentic / tool-calling —
exactly the surface the later `plan` / `related` / `dedupe` verbs need.
Hand-rolling (option 2) would duplicate a large, maintained surface; the hybrid
(option 3) carries two HTTP stacks for marginal benefit.

The sync-core invariant is preserved because AI is an **opt-in feature flag**
(like `web`), not the default path: `--no-default-features`, `tui`, and `forge`
remain `tokio`-free. Async is confined to the `ai` feature and bridged with a
single `block_on` at the CLI boundary, so verb handlers stay synchronous.

### Positive Consequences

- Both phase-1 providers *and* the phase-3 RAG stack come from one maintained
  dependency.
- The sync core is untouched; async lands only when `ai` is enabled — the same
  bargain `web` already makes.
- An `AiProvider` facade keeps rig types out of verb signatures, so rig stays
  swappable (mirrors how `forge` owns its trait over the HTTP client).
- A single `block_on` boundary keeps the verb handlers synchronous and testable.

### Negative Consequences

- The `ai` build pulls `tokio` + `reqwest` (heavier deps) — acceptable for an
  opt-in feature, but it is a second async stack in the workspace alongside `web`.
- `rig` is 0.x and moves fast; pinning + periodic upgrades are a maintenance cost
  the facade must absorb.
- Two HTTP clients now exist across features (`ureq` for `forge`, `reqwest` via
  `rig` for `ai`); contributors must know which feature owns which.

## Implementation

- Add an `ai` Cargo feature: `ai = ["dep:rig-core", "dep:tokio", "dep:serde_json"]`
  with tokio's `rt-multi-thread` + `macros`, mirroring the `web` feature's gating.
- `src/ai/mod.rs`: an `AiProvider` trait (`complete` + `embed`) with value types
  and an `AiError` mirroring `forge::ForgeError`'s offline / auth / api split; a
  rig-backed adapter that constructs rig's Anthropic / Ollama clients; a single
  `Runtime::block_on` bridge.
- `src/ai_hook.rs`: an always-compiled facade (real when `ai` is on, no-op twins
  when off) so verbs carry no `#[cfg]`, exactly like `forge_hook`.
- Config: an `ai:` section (provider, model, `enabled` kill-switch) + env tokens
  (`ADROIT_ANTHROPIC_KEY`, …), reusing the credential store from `adroit auth`.
- First verbs on this foundation: `new --ai` / `new --interview` (issue 5) and
  `adroit plan <ADR#>`.
- Tests: an offline `FakeProvider` implementing `AiProvider` (no network),
  mirroring `forge`'s `FakeTransport`.

This ADR is the foundation the AI-authoring work (issue 5) builds on.
