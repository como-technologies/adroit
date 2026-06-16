# ADR-0001: Reinstate the in-repo adr/ corpus

> State: Accepted

## Status

Accepted

## Stakeholders

- Maintainer — Brett Fowle

## Context and Problem Statement

adroit briefly had an in-repo `adr/` corpus: the rig-adoption decision was
recorded there with adroit's own verbs in June 2026. Days later it was
**deliberately removed** ("Remove the redundant in-repo adr/ sample corpus",
commit `136a2bc`): the corpus held only that one sample record, dogfooding
happened against an external repo pointed at by `ADROIT_DIR`, so the in-repo
copy looked redundant. `/adr/` was added to `.gitignore` so a stray dogfood run
could not re-commit it. The `.claude/skills/adr/` skill — which describes
recording adroit's own decisions in `adr/` — was deliberately kept, leaving the
repo in an odd state: a skill (and book page) describing a corpus that did not
exist in any branch.

The portfolio mandate has since changed. The iteration-1 direction requires
**every repo in the portfolio to carry its own adroit-managed ADR corpus**,
validated in CI. adroit is the spine tool whose corpus discipline every other
repo is about to copy — it must be proven here first, and adroit's own
load-bearing decisions (the AI provider seam, the statelessness invariant, the
manifest semantics table, the MCP projection) currently live only in commit
messages and CLAUDE.md prose, with no validated, addressable record.

## Decision Drivers

- Portfolio-wide mandate: every repo records its decisions in its own
  adroit-managed corpus, gated in CI.
- Credibility: adroit is the Prescribe-stage decision engine; it should visibly
  dogfood itself, and the existing `/adr` skill and book page already claim it
  does.
- Discoverability: load-bearing decisions should be addressable
  (`adroit show`, `list -o json`) and validated (`check`, `lint`), not scattered
  across commit messages.
- The reversal must be honest: this re-reverses a decision that was made
  deliberately, so the record has to carry both the old reasoning and the new.

## Considered Options

1. **Reinstate a top-level `adr/` corpus** (markdown / by-status), authored
   with the built `adroit` binary, with `adroit check --dir adr` wired into
   `just ci`.
2. **Keep the status quo** — decisions dogfooded only against the external
   `ADROIT_DIR` repo, no in-repo corpus, `/adr/` gitignored.
3. **Record decisions in the mdbook** (`docs/src/`) instead of an ADR corpus.

## Decision Outcome

Chosen: **Option 1 — reinstate the in-repo `adr/` corpus**, because the
portfolio now requires every repo to carry its own corpus and adroit, as the
tool that defines the discipline, must prove it on itself first.

This reverses commit `136a2bc`. The external dogfood repo remains valuable for
exercising adroit against a *foreign* corpus; it was never a home for adroit's
own decisions, and "redundant sample" no longer describes a corpus that records
the real architecture. Option 3 would fight the established doc rule — ADRs are
decision records, deliberately separate from the user manual — and would leave
the records unmanaged by adroit's own verbs.

Concretely: `/adr/` comes out of `.gitignore`, this ADR and retroactive records
of the standing load-bearing decisions are authored with the built binary
(always `--dir adr`), and `just ci` gains a self-hosted gate where the freshly
built adroit checks its own corpus.

### Positive Consequences

- adroit's decisions become addressable, machine-readable, and CI-validated —
  the same seam (`manifest` / `-o json`) downstream agents consume.
- The corpus discipline every other portfolio repo copies is proven on the
  spine tool first.
- The `/adr` skill and the book's Development section describe something that
  actually exists.
- Retroactive ADRs give future contributors the "why" for the invariants the
  codebase enforces.

### Negative Consequences

- The `ADROIT_DIR` foot-gun is real: in this workspace it points at the
  external dogfood repo, so a bare `adroit new` writes into the **wrong**
  corpus. Every own-corpus command must pass `--dir adr`; the skill warns, but
  the discipline is on the author.
- With `/adr/` no longer gitignored, a stray dogfood run that forgets `--dir`
  in the *other* direction can now commit junk into adroit's corpus — review
  must watch for it.
- Decision churn: this re-reverses a days-old deliberate removal. The cost is
  accepted because the portfolio mandate changed, but it is churn.
- The retroactive records (ADR-0002 through ADR-0005) are reconstructions:
  their git dates reflect when they were recorded, not when the decisions were
  made.

## Implementation

- Remove `/adr/` (and its comment) from `.gitignore`.
- Author this ADR plus the retroactive and standing-direction ADRs with
  `./target/debug/adroit … --dir adr`, using real `proposed → accepted`
  transitions (no hand-edited statuses).
- Add an `adr-check` recipe to the justfile — build, then
  `./target/debug/adroit check --dir adr` — and add it to the `ci` recipe.
- Doc-sync: CLAUDE.md project layout and the book's Development section gain
  the in-repo corpus.
