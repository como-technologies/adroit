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
  and `adroit index --check` (SUMMARY.md is in sync). Either failing fails the
  build, so a malformed ADR or a stale index can't merge.
- **On a decision PR/MR** — generate the review-kickoff doc with
  `adroit review <n>` and post it as the PR/MR description, so reviewers get a
  consistent "here's what you're deciding" brief.

## Files

- `github/adr.yml` → copy to `.github/workflows/adr.yml`
- `gitlab/.gitlab-ci.yml` → copy to your repo root (or `include:` it)

## Two knobs (top of each file)

- **`ADROIT_DIR`** — path to your ADR tree, e.g. `docs/adrs` or `src/adrs`.
- **How `adroit` is installed** — the templates show `cargo install --git`
  (simplest), but pin to a tag, vendor a release binary, or use a prebuilt
  image as you prefer. adroit isn't published to crates.io yet.

These are starting points, not a framework — read them and make them yours.
