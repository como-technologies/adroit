# ADR-0018: Retire candidate forge, tracker, and web expansion

> State: Accepted

## Status

Accepted
Created: 2026-06-12

## Stakeholders

- Maintainer — Brett Fowle

## Context and Problem Statement

The roadmap's integration tables list shipped providers next to candidates:
Gitea/Forgejo and Bitbucket on the `Forge` seam, Azure DevOps Boards and
Asana on the `Tracker` seam, plus two web-dashboard items (per-repo
branding, one-click create-MR). Shipped today: GitHub + GitLab as full
forges, five trackers (GitHub, GitLab, Jira, Linear, monday.com), six
publish targets, and a read-only dashboard.

Every candidate is a contained add behind a trait seam — which, as with the
AI providers (ADR-0016), is precisely why the open list needs a decision
rather than momentum. No suite consumer runs any candidate: the Adopt-stage
engine drives the throwaway forge itself (its own Gitea adapter is *its*
forge-neutrality proof, in its own repo, by design — the suite referee
recorded that boundary explicitly so the two retire/build lists are never
"harmonized" against each other), and no dogfood or client lane has needed
Bitbucket, Azure Boards, or Asana.

## Decision Drivers

- Shipped coverage already exceeds what any suite consumer uses; provider
  count is not the suite-done property — **proven extensibility is**, and
  the `/extend` skill + the trait seams (one module + one factory arm per
  provider) are that proof.
- Each live adapter is a real surface: API drift, auth shapes, fixture
  upkeep (`forge_faults.rs`), and per-provider semantic exceptions (the
  GitHub-vs-Gitea review-dismissal split in run-1's learnings is the
  cautionary tale).
- The dashboard's read-only-ness is a stated security property (no endpoint
  writes ADRs); one-click create-MR would end it for a convenience nobody
  has requested.
- The mandate's built-or-retired bar for candidate lists.

## Considered Options

1. **Retire the candidate lists**: shipped providers are the supported set;
   reopen per-provider on consumer demand; dashboard stays read-only.
2. **Build selected candidates now** (e.g. Gitea as a forge, since the
   ecosystem runs one).
3. **Leave the candidate tables open** as roadmap prose.

## Decision Outcome

Chosen: **Option 1 — retire the candidate forge/tracker/web items for
suite-done.** The Gitea temptation (option 2) is instructive: a Gitea
`Forge` in adroit would duplicate, in the wrong repo, the adapter the
Adopt-stage engine already owns as its own forge-neutrality proof — the
recorded portfolio boundary keeps lifecycle-governance adapters (adroit's
lane) distinct from code-orchestration adapters (the Adopt engine's lane).
Beyond that, every candidate is speculative surface: adapters that would
ship without a consumer to catch their drift. Option 3 is the hedge the
mandate disallows.

**Reopen criterion, per seam:** a consumer who actually runs the missing
system — a client team on Bitbucket/Azure Boards/Asana wanting ADR-lifecycle
governance there, or (for the web items) an SME session that demonstrably
stalls on the dashboard's default branding or its read-only-ness. Each
reopen is one `/extend` pass on its seam: module + factory arm + fault-mock
tests + docs.

### Positive Consequences

- The forge surface stays exactly as exercised by real lanes: GitHub +
  GitLab full-cycle, five trackers, each with fault-injected tests.
- The dashboard's "no endpoint writes ADRs" property survives as a
  categorical sentence.
- The portfolio boundary (governance adapters here, orchestration adapters
  in the Adopt engine) stays legible in both repos' ADR corpora.

### Negative Consequences

- A Bitbucket/Asana/Azure-Boards team cannot adopt adroit's forge
  integration today without the (contained) `/extend` work — first-contact
  friction on those stacks.
- The dashboard keeps its default look; branding-conscious demos must lean
  on the publish targets instead.
- "Retired" lists risk reading as product limits rather than sequenced
  decisions; the per-seam reopen criterion is the answer, recorded here.

## Implementation

Nothing to build — this ADR records the disposition. The roadmap's
candidate tables and web-dashboard items now cite it; the `/extend` skill
remains the per-seam reopen checklist.
