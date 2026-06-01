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

## The review brief

On a decision PR/MR, generate the kickoff document and post it as the
description so reviewers get a consistent "here's what you're deciding" brief:

```sh
adroit review <number> --output kickoff.md
```

It includes the decision summary, key-docs links, the review timeline and
quorum, and what the merge changes — see
[`adroit review`](../reference/cli.md#adroit-review-number).

## Templates

Copy-and-customize starters live in the repo under
[`ci-templates/`](https://github.com/como-technologies/adroit/tree/main/ci-templates):

- **GitHub Actions** → `ci-templates/github/adr.yml` → `.github/workflows/adr.yml`
- **GitLab CI** → `ci-templates/gitlab/.gitlab-ci.yml`

Each has two knobs at the top: `ADROIT_DIR` (your ADR tree) and how `adroit` is
installed (it isn't on crates.io yet — pin to a tag, vendor a binary, or use a
prebuilt image). They're starting points, not a framework — read them and make
them yours.
