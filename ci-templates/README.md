# adroit CI templates

Drop-in CI starters that bake the ADR process into your pipeline. Copy the one
for your platform into **your ADR repo** (not this repo) and adjust the two
knobs at the top: the ADR directory and how `adroit` is obtained.

They encode the two-stage workflow adroit is built around:

1. **Propose on `main`.** ADR content is written and iterated directly on the
   default branch in `proposed/`. No gate — the goal is low friction.
2. **Decide via PR/MR.** The decision (Proposed → Accepted / Rejected) is the
   PR/MR: it moves the file between status directories and flips the `## Status`.
   That's where the team reviews.

What the pipelines do:

- **On every push/PR to `main`** — run `adroit check` (status ↔ directory
  consistency, duplicate numbers, unparseable ADRs, broken supersession links)
  and `adroit index --check` (SUMMARY.md is in sync). `adroit check` exits
  non-zero only on **errors**; a **stale link** (an inbound link a deferred
  relink hasn't canonicalized yet) is a warning and does not fail the build.
- **In the merge queue / merge train** — `adroit check` also runs on the
  *speculative merge* (`merge_group` on GitHub; merged-results pipelines on
  GitLab). This is what catches an ADR-number collision **between** branches:
  two PRs that each add `0009-*.md` pass on their own branch but fail once both
  land in the merge group, so the second is ejected. Resolve with
  `adroit renumber <dup> <next-free>`.
- **On a decision PR/MR** — generate the review-kickoff doc with
  `adroit review <n>` and post it as the PR/MR description, so reviewers get a
  consistent "here's what you're deciding" brief.

## Branching teams: propose-on-branch, heal-on-main

When many people work the same ADR set on branches, set `relink_scope = self`
(or `none`) in your ADR repo (`adroit config set relink_scope self`, or
`ADROIT_RELINK_SCOPE=self`). A status-change PR then moves only its own ADR and
fixes only that file's links — it does **not** rewrite the inbound links in
other ADRs, so two concurrent decision PRs never touch the same neighbor file
and never produce a false merge conflict. The trade-off is that those inbound
links are transiently stale (a warning) until the **relink workflow** runs on
`main` after merge and commits the canonicalized links. That post-merge
`adroit relink` is the single, serialized place links are reconciled.

(For a team that would rather avoid collisions by construction, `adroit` also
supports the `date`/`uuid` naming schemes and the `by_category` layout — see the
user manual's "Concurrent contributors" page.)

## Files

- `github/adr.yml` → copy to `.github/workflows/adr.yml` (validate + review brief)
- `github/relink.yml` → copy to `.github/workflows/adr-relink.yml` (heal-on-main)
- `gitlab/.gitlab-ci.yml` → copy to your repo root (or `include:` it) — includes
  the `adr:relink` job

## Two knobs (top of each file)

- **`ADROIT_DIR`** — path to your ADR tree, e.g. `docs/adrs` or `src/adrs`.
- **How `adroit` is installed** — the templates show `cargo install --git`
  (simplest), but pin to a tag, vendor a release binary, or use a prebuilt
  image as you prefer. adroit isn't published to crates.io yet.

These are starting points, not a framework — read them and make them yours.
