# CI Integration

adroit is built to bake the ADR process into a GitHub/GitLab pipeline. This
page covers the workflow it assumes, the two commands that gate it, and the
ready-to-copy templates.

## The two-stage workflow

1. **Propose on `main`.** ADR content is written and iterated directly on the
   default branch, in `proposed/`. No gate — capturing a decision should be
   low-friction.
2. **Decide via PR/MR.** The decision itself *is* the pull/merge request: it
   moves the file from `proposed/` to `accepted/` or `rejected/` and flips the
   `## Status`. That PR/MR is where the team reviews and where the decision is
   recorded.

CI's job is to (a) keep every change structurally honest, and (b) give
reviewers a consistent brief on the decision PR/MR.

## The two gates

Both exit non-zero on a problem, so they drop straight into a CI job:

```sh
adroit check          # status↔directory consistency, duplicate numbers,
                      # unparseable ADRs, broken supersession links
adroit index --check  # fail if SUMMARY.md is out of date
```

Run them on every push/PR to `main`. A malformed ADR or a stale index then
can't merge. `check` prints each problem to stderr and exits non-zero;
`index --check` never writes — it just verifies `SUMMARY.md` matches what
`adroit index` would generate. (See the [CLI Reference](../reference/cli.md) for
both.)

## Concurrent ADR numbers across branches

Sequential `NNNN` numbers can **collide across branches**: if two people branch
off `main` and each run `adroit new`, both get the same next number — each branch
is internally consistent, but the duplicate only appears once both merge. This is
a known, unsolved limitation of sequential numbering across the ecosystem (see
[adr-tools #102](https://github.com/npryce/adr-tools/issues/102) and
[MADR #28](https://github.com/adr/madr/issues/28); log4brains eventually
[dropped sequential numbers entirely](https://thomvaill.github.io/log4brains/adr/adr/20201016-use-the-adr-slug-as-its-unique-id/)
to sidestep it).

`adroit check`'s duplicate-number rule is the enforcement point. The trick is to
run it on the **merged** state, and to **serialize merges** so two PRs/MRs can't
both go green and land a collision:

- **GitHub** — the `pull_request` job runs `adroit check` on the *merge ref*
  (your branch merged into the current `main`), so once one `0021` lands on
  `main`, the other PR's check sees both and fails. Make it airtight with a
  **merge queue**: the template also triggers on `merge_group`, so `check` runs
  on the queue's *speculative merge* of `main` + the PRs ahead + this PR — the
  collision surfaces there and the second PR is ejected. Mark **Validate ADRs** a
  **required status check** for the queue. (No queue? **Require branches to be up
  to date before merging** is the weaker fallback.)
- **GitLab** — a normal MR pipeline runs on the *source branch only*, so it won't
  see a number that's on `main` but not your branch. Enable **merged results
  pipelines** (runs `check` on the merged ref) and ideally **merge trains**
  (which serialize merges).
- **Safety net** — the `push` / `main` job runs `check` after every merge, so
  even if a race slips through it fails immediately on `main`. Resolve it with
  [`adroit renumber <old> <new>`](../reference/cli.md#adroit-renumber-old-new---file-path),
  which renames the file and fixes every inbound reference.

The real guarantee is serializing merges (merge queue / merge train); without it
there is always a small window where two PRs are both green and merge nearly
simultaneously — caught after the fact by the post-merge job.

## Stale links across branches

The same branch-vs-merge tension hits **cross-ADR links** in the `by_status`
layout. A status change moves an ADR between directories, which by default
(`relink_scope = all`) rewrites the relative links in *every* ADR that points at
it. Two concurrent decision PRs then edit the same neighbor files → false merge
conflicts, and whichever merges second is left pointing at a `proposed/` path
that no longer exists.

Fix it the same way you fix numbering — defer the repo-wide step to `main`:

1. **On the branch**, set `relink_scope = self` (or `none`) so a status-change
   PR fixes only its own ADR's links and never touches neighbors. Two decisions
   on two branches now edit disjoint files. The neighbors' inbound links go
   transiently stale, which `adroit check` reports as **warnings** — it still
   exits 0, so the PR isn't blocked.
2. **On `main` after merge**, the `relink.yml` workflow runs a full
   `adroit relink` (always full-scope, idempotent) and commits the canonicalized
   links. A `concurrency` group serializes it and a `[skip ci]` commit message
   keeps it from looping. This is the single deterministic place inbound links
   are reconciled.

```sh
adroit config set relink_scope self   # in your ADR repo (or ADROIT_RELINK_SCOPE=self)
```

See [Concurrent contributors](./managing-adrs.md#concurrent-contributors--branching)
for the conceptual overview and the collision-free-by-construction alternatives.

## The review brief

On a decision PR/MR, generate the kickoff document and post it as the
description so reviewers get a consistent "here's what you're deciding" brief:

```sh
adroit review <number> --out kickoff.md
```

It includes the decision summary, key-docs links, the review timeline and
quorum, and what the merge changes — see
[`adroit review`](../reference/cli.md#adroit-review-number).

## Templates

Copy-and-customize starters live in the repo under
[`ci-templates/`](https://github.com/como-technologies/adroit/tree/main/ci-templates):

- **GitHub Actions** → `ci-templates/github/adr.yml` → `.github/workflows/adr.yml`
  (validate + review brief), and `ci-templates/github/relink.yml` →
  `.github/workflows/adr-relink.yml` (the post-merge heal-on-main relink)
- **GitLab CI** → `ci-templates/gitlab/.gitlab-ci.yml` (includes the `adr:relink` job)

Each has two knobs at the top: `ADROIT_DIR` (your ADR tree) and how `adroit` is
installed (it isn't on crates.io yet — pin to a tag, vendor a binary, or use a
prebuilt image). They're starting points, not a framework — read them and make
them yours.
