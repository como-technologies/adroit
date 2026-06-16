# ADR-0012: Adopt a tagged-release and changelog discipline

> State: Accepted

## Status

Accepted
Created: 2026-06-12

## Stakeholders

- Maintainer — Brett Fowle

## Context and Problem Statement

adroit has consumers now. The Adopt-stage engine pins the binary by git rev
(`cargo install --git <path> --rev <sha>`), sibling repos shell out to a
locally built binary, and the portfolio's evidence gates cite specific
behavior. Yet nothing in the repo marks a stable point: `Cargo.toml` has said
`0.1.0` since the first commit, `--version` is meaningless as a contract, and
"which adroit is this?" is answered by reading git logs. There is no record of
what changed between the rev one consumer pins and the rev another built.

How should adroit mark releases and record changes, given the standing
working agreement that **nothing is pushed or published without the owner's
explicit permission**?

## Decision Drivers

- Consumers pin by rev; a pin should name an intentional, gate-green point
  with a version that the binary itself reports (`--version` == the tag).
- The one-doc-system rule: all documentation lives in the mdbook — a
  standalone `CHANGELOG.md` would be a second documentation surface.
- The no-publish working agreement: tags must work as *local* coordination
  points; pushing them is a separate, owner-only act.
- Cross-repo sequencing: the portfolio's pin-advance step needs a single
  named sha ("the v0.2.0 tag") rather than "whatever main was that day".

## Considered Options

1. **Local annotated tags + version bumps + a changelog chapter in the
   mdbook**: a release is an annotated `vX.Y.Z` tag on merged main whose
   commit bumps `Cargo.toml` so `--version` matches; each release gets an
   entry in a Changelog chapter wired into `SUMMARY.md`.
2. **Continuous main, rev pins only** (the status quo): no versions, no
   changelog; consumers pin shas and diff logs.
3. **Standalone `CHANGELOG.md` + forge releases**: the conventional public
   pattern (GitHub Releases, pushed tags).

## Decision Outcome

Chosen: **Option 1 — local annotated tags, version bumps, and a changelog
chapter in the book**, because it gives consumers a named, self-identifying
contract point without violating either house rule: the changelog lives in
the one documentation system (a book chapter, never a standalone file), and
tags stay **local until the owner publishes** — tagging is bookkeeping,
pushing is a decision. The status quo (option 2) is what made the current
pin skew possible ("the pin predates the fixes" was discoverable only by
archaeology). Option 3 violates the no-publish agreement and splits docs.

The discipline, concretely:

- A release = one commit on merged main that bumps `Cargo.toml` (semver:
  breaking CLI/JSON contract → major once 1.0, feature → minor, fix → patch
  pre-1.0 minor conventions apply) and updates the changelog chapter; the
  annotated tag `vX.Y.Z` lands on that commit.
- `adroit --version` (clap reads `Cargo.toml`) therefore reports the tag's
  version — a consumer can verify its pin mechanically.
- The changelog chapter records, per release: the headline changes, contract
  changes (`-o json` shapes, exit semantics), and the ADRs accepted in the
  release window.
- Tags are pushed only by the owner, explicitly, like every other publish.

### Positive Consequences

- The pin-advance contract becomes "the v0.2.0 tag sha": one name, verified
  by `--version`, no archaeology.
- Changes between releases are recorded where every other doc lives — the
  book builds, links, and ships them together.
- Local annotated tags survive clones of the local repo path (the consumers
  here are sibling checkouts) and carry their own message/date.

### Negative Consequences

- Local-only tags do not exist for anyone who can't see this working tree —
  this is deliberate, but it means "release" here is a coordination point,
  not distribution (distribution is ADR-0013's question).
- Version bumps and changelog entries are manual ceremony on every release;
  forgetting the bump makes `--version` lie until caught (the release
  checklist in the changelog chapter mitigates).
- Semver judgment calls (is a lint-severity addition breaking?) are on the
  maintainer; pre-1.0 the bar is "additive JSON = minor".

## Implementation

Landed with this decision (fix-train M6):

- Changelog chapter at `docs/src/reference/changelog.md`, wired into
  `SUMMARY.md`, covering 0.1.0 → 0.2.0.
- `Cargo.toml` bumped to 0.2.0 in the same change (`--version` reports it).
- The v0.2.0 annotated tag lands on merged main when this branch merges (the
  release event itself), after which the Adopt-engine pin advances to the
  tag sha.
