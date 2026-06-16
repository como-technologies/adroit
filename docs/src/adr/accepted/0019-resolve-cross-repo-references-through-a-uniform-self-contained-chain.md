# ADR-0019: Resolve cross-repo references through a uniform self-contained chain

> State: Accepted

## Status

Accepted
Created: 2026-06-12

## Stakeholders

- Maintainer — Brett Fowle
- Maintainers of every suite repo that consumes adroit (assessments, conduit,
  tuesday, pulse, playbook, portfolio, general-business) or that adroit's own
  contract fixture is regenerated from (assessments)

## Context and Problem Statement

Cross-repo references across the Como TAPS suite assumed a fixed sibling
workspace layout. For adroit that cuts both ways: every consumer's gate
script hardcoded some variant of `../adroit/target/{release,debug}/adroit`
(orders differing per repo), and adroit's own vendored ingest-contract
fixture (`tests/fixtures/golden-assessment.yaml`) said only "regenerate
THERE" — assuming a sibling assessments checkout with no stated alternative.
That breaks every single-repo clone and hides which binary or source a gate
actually used. Meanwhile some suite repos have no public remote at all
(conduit, playbook; the docs evidence repo is local-only by explicit policy)
and adroit's public remote currently lags the local checkout — so any
remote-based resolution must verify what it fetched and degrade cleanly
rather than assume the network has what the workspace has.

## Decision Drivers

- Consumers' gates must work in a single-repo clone, not only in the full sibling workspace
- Repos without a public remote today must keep exactly their current skip/notice behavior
- No repo may source resolver code from a sibling — each copy self-contained
- Contract-grade pins stay explicit, reviewed edits (conduit's adroit.rev; ADR-0012's tag discipline)
- Secrets and live-forge artifacts must never resolve via git
- Resolvers must be read-only: no push, no remote registration, no credentials in URLs
- Offline runs (COMO_OFFLINE=1, or any fetch failure) must degrade, not hang or fail advisory gates

## Considered Options

1. **Status quo** — every consumer keeps its own hardcoded `../adroit` path
   (or ad-hoc PATH probe); works only in the curated workspace and silently
   skips or fails everywhere else.
2. **Shared resolver helper** — one resolver script maintained in a single
   repo and sourced by the others; creates the very cross-repo bootstrap
   dependency the resolver exists to remove.
3. **Uniform self-contained chain per repo** — every repo embeds the same
   small resolver: env override → sibling → PATH → gitignored clone cache →
   skip/fail with the knobs named.

## Decision Outcome

Chosen: **option 3, the uniform self-contained resolution chain**, because
it keeps the sibling workspace fast while making every repo resolvable (or
honestly degraded) on its own.

Cross-repo references in the Como TAPS suite resolve through one uniform,
self-contained chain instead of assuming sibling checkouts: (1) an explicit
environment override (ADROIT_BIN for the adroit binary, `COMO_<REPO>_DIR`
for a checkout directory), (2) the sibling checkout `../<repo>`, (3) for
binaries, an installed binary on PATH, (4) a gitignored git-clone cache
under `.como/` in the consuming repo, populated read-only from
`${COMO_GIT_BASE:-https://github.com/como-technologies}/<repo>.git`, and
(5) the existing skip-with-notice for advisory gates or an actionable error
naming the knobs for hard dependencies. Each repo embeds its own copy of
the resolver — no repo ever sources helper code from a sibling.
Contract-grade dependencies stay pinned: conduit installs adroit at the
exact rev in adroit.rev (remote URL by default, sibling file:// only as the
local-dev override), and any script that reads another repo's source as a
contract (portfolio's verify-claims) declares the rev it clones and prints
which source it actually resolved. Runtime secrets and live-forge artifacts
are never resolved via git — they are env-first with documented local-path
fallbacks — and the docs evidence repo is local-only by policy, so
references to it stop at skip-with-notice. Resolvers only clone and fetch:
they never push, never add the cache as a remote, and never carry
credentials in URLs. Repos without a public remote today (conduit,
playbook, docs) degrade to exactly the skip-with-notice behavior they
produce now, so nothing breaks before the owner pushes them.

### Positive Consequences

- Every consumer resolves the same adroit the same way, with one set of
  knob names suite-wide, and a single-repo clone of a consumer can still
  gate its ADR corpus
- adroit's own cross-repo input — the vendored assessments golden — now
  names a clone-based regeneration path instead of assuming the sibling
- The convention reinforces ADR-0012/ADR-0013: distribution stays
  source-served (`cargo install --git`, tag-anchored), with no new publish
  surface

### Negative Consequences

- Several self-contained copies of the same resolver exist suite-wide and
  can drift; the canonical snippet lives in the suite ADR and drift has to
  be policed by review
- Consumers' clone legs are untested-by-default while adroit's public
  remote lags local work (the pinned tag is not on the remote yet) — they
  must verify the install and degrade with a notice, and can rot silently
  behind the sibling fallback
- The first cache install turns a previously-instant skip into a network
  `cargo install --git` build on consumer machines
- Standardizing env → sibling → PATH changes precedence on dev machines
  that had a PATH-installed adroit: a fresh sibling build now wins

## Implementation

For adroit this is a docs/provenance sweep only — no behavior change. The
vendored fixture header (`tests/fixtures/golden-assessment.yaml`) and the
cross-repo ingest-contract row in Development → Testing now say the golden
is regenerated in an assessments *checkout* (sibling `../assessments` or a
clone of `${COMO_GIT_BASE:-…}/assessments.git`) via `just golden` and
re-copied verbatim; the `assessments` links in the import docs point at the
canonical repo URL; ADR-0010/ADR-0013 prose names the convention where it
described sibling gate scripts and the fixture's provenance. The resolver
itself lives in each consuming repo's justfile/scripts, recorded by that
repo's own copy of this ADR.
