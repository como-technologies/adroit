# ADR-0013: Retire published distribution for suite-done

> State: Accepted

## Status

Accepted
Created: 2026-06-12

## Stakeholders

- Maintainer — Brett Fowle

## Context and Problem Statement

The installation page has said "isn't published yet — build it from source"
since Phase 1, with crates.io publication and prebuilt binaries as implied
eventual work. The portfolio's suite-done bar for adroit is **SME-usable** —
an SME drives the Prescribe workflow with Como alongside — and every consumer
adroit actually has (the Adopt engine's rev pin, sibling repos' gate scripts —
which resolve the binary per the suite resolution convention: ADROIT_BIN →
sibling checkout → PATH → a pinned `cargo install --git` clone cache →
skip-with-notice — the dogfood corpora) builds from the local source tree.

Publishing to crates.io or attaching prebuilt binaries is a **remote,
irreversible, owner-only action** under the standing working agreement
(nothing is published without explicit permission — a crates.io version
cannot be unpublished). Should published distribution stay on the implied
roadmap, or be retired for suite-done by decision?

## Decision Drivers

- The mandate's escape hatch: every deferred item is built **or retired by
  accepted ADR** — an implied "eventually publish" satisfies neither.
- SME-usable does not require it: the rung's definition has Como alongside;
  `rustup` + `just build` (or a handed-over binary) covers every real
  consumer today.
- Publishing is owner-only and irreversible; a suite gate must never depend
  on an action the delivery process is forbidden to take.
- ADR-0012 already gives consumers a named contract point (the local
  annotated tag) without any publication.

## Considered Options

1. **Retire for suite-done**: build-from-source plus the local tagged release
   (ADR-0012) is the supported distribution at this rung; record the reopen
   criterion.
2. **Publish to crates.io now**: `cargo install adroit` for everyone.
3. **Prebuilt binaries only**: CI-built artifacts attached to forge releases,
   no crates.io.

## Decision Outcome

Chosen: **Option 1 — retire published distribution for suite-done**, because
no consumer at this rung needs it and both alternatives are owner-only
publishing acts (option 2 additionally permanent, and the `adroit` crate
name is a one-shot decision that deserves its own moment). This follows the
ADR-0009 retirement pattern: not "never", but "not until the entry criterion
fires, and the criterion is written down".

**Reopen criterion:** a declared **self-serve** direction for adroit — a
consumer who must install adroit *without* the source tree or Como alongside
(the portfolio direction naming adroit self-serve, or an external user
requesting installation). At that point the decision is *which* channel
(crates.io vs binaries vs both), made by the owner who performs it.

### Positive Consequences

- The installation page can state the actual story honestly instead of
  apologizing for a phase ("build from source — by decision, not neglect").
- No suite gate hangs on an irreversible third-party action; the conduit pin
  contract (`cargo install --git --rev <tag-sha>`) is fully served locally.
- The crate-name decision is preserved for the moment it matters.

### Negative Consequences

- Anyone outside the working tree's reach cannot install adroit at all — by
  design at this rung, but it forecloses casual adoption and the feedback it
  might bring.
- Build-from-source demands a Rust toolchain (~minutes of compile) from any
  future evaluator; "try it in 30 seconds" is impossible until reopened.
- If the reopen criterion fires suddenly (an SME wants self-serve next
  week), publication becomes a blocking step on someone else's timeline.

## Implementation

Landed with this decision (fix-train M6): the installation page states
build-from-source as the supported distribution and cites this ADR; the
roadmap carries no publication item. Nothing else to build — this ADR *is*
the disposition.
