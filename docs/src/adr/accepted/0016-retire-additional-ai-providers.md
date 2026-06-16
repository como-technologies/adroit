# ADR-0016: Retire additional AI providers

> State: Accepted

## Status

Accepted
Created: 2026-06-12

## Stakeholders

- Maintainer — Brett Fowle

## Context and Problem Statement

The roadmap lists "more providers" as a standing AI lane: rig ships ~two
dozen adapters (OpenAI, Gemini, Mistral, OpenRouter, …) and each is a
one-line `AiProviderKind` variant plus one client arm in `rig_provider.rs`.
The list is cheap to extend — which is exactly why it needs a decision: an
open provider lane invites additions nobody consumes, each one carrying
prompt-behavior variance (the run-1 learnings are a catalog of small-model
quirks that had to be sanitized per shape), credential-handling surface, and
doc/test obligations.

adroit's two providers — **anthropic** (hosted) and **ollama** (local) —
cover both lanes the suite actually runs: the dogfood mandate is local
ollama, and the hosted lane is anthropic. Should the provider list stay an
open invitation?

## Decision Drivers

- No consumer needs a third provider: every suite lane (dogfood, live
  rehearsals, run-1) ran on ollama or anthropic.
- The seam keeps extension a one-line additive change (`AiProviderKind` +
  one factory arm — the `/extend` checklist) — retiring costs no
  architecture.
- Each provider added is a real maintenance surface: model-output quirks
  feed the sanitizer (M5 rehearsal, run-1 skeleton echoes), keys feed the
  credential store, and docs/tests must follow.
- The mandate's built-or-retired bar for roadmap lanes.

## Considered Options

1. **Retire the lane**: anthropic + ollama are the supported providers;
   reopen per-provider on consumer demand.
2. **Add the "high-value picks" now** (OpenAI, Gemini, OpenRouter, per the
   roadmap).
3. **Leave the lane open** as roadmap prose.

## Decision Outcome

Chosen: **Option 1 — retire additional providers for suite-done**, because
provider count is not the property anyone consumes — the suite needs one
local and one hosted lane, and has both. The seam's extensibility (proven by
the pattern: every provider is a variant + an arm, with `/extend` as the
checklist) is the durable asset; speculative adapters (option 2) would ship
untested-by-use code whose model quirks nobody would catch until a consumer
hit them. Option 3 is the hedge the mandate disallows.

**Reopen criterion:** a consumer with a concrete provider requirement — a
client whose models live behind a specific API, or a suite lane that adopts
one (e.g. an OpenRouter-routed evaluation). Each reopen is one provider, one
`/extend` pass (variant + factory arm + env key + docs + a fake-backed
test), not a reopened lane.

### Positive Consequences

- The AI surface stays exactly as tested: two providers, both exercised by
  real runs, both covered by the sanitizer's quirk regressions.
- No credential-handling growth (each provider is another key shape in the
  store) without a consumer attached.
- The reopen path is documented and cheap — the retirement gives up no
  optionality.

### Negative Consequences

- An evaluator whose only key is, say, OpenAI cannot try adroit's AI lane
  without the (small) `/extend` change — friction exactly at first contact.
- rig upgrades may someday reshape the adapter API; with only two arms, the
  migration sample is small (less code to fix, but also fewer examples).

## Implementation

Nothing to build — this ADR records the disposition. The roadmap's "more
providers" entry now cites it; the `/extend` skill remains the reopen
checklist.
