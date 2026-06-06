# Forge Integration

When your ADRs live in a Git repo on **GitHub** or **GitLab**, adroit can drive
the *forge* — its issue tracker and pull/merge requests — so the **PR is the
decision**: propose an ADR, open its review PR, and when the team merges, the ADR
moves `proposed/ → accepted/`. The `forge` feature is in the default build; it
stays **off until you configure a provider**, and every forge action is **opt-in
per command** (`--forge`) and **previews by default** (`--yes` to apply).

> **The ADR is the durable record.** Forge calls are best-effort: if the forge is
> unreachable or a token is missing, adroit **warns and keeps the local change** —
> it never loses your ADR to a network error.

## Setup

### 1. Configure the provider

`adroit init` is an interactive wizard that detects the forge from your `git`
remote and writes the `forge.*` config:

```sh
adroit init            # detect + configure (interactive)
adroit init --print    # show what it detected + would do, write nothing
adroit init --yes      # non-interactive: accept the detected forge + defaults
```

Or set the keys yourself (see the [config reference](../reference/cli.md#configuration)):

```yaml
forge:
  provider: github          # or: gitlab
  repo: owner/repo           # the provider slug
  base_branch: main          # PRs target this branch
  branch_prefix: adr/        # `new --forge` branches: adr/0021-…
  # host: ghe.example.com/api/v3   # GitHub Enterprise / self-managed GitLab
```

### 2. Authenticate

Tokens are **never** stored in config. Provide them via the environment (best for
CI) or the local credential store — they resolve env-first, then the `adroit auth`
store:

```sh
export ADROIT_GITHUB_TOKEN=…   # or ADROIT_GITLAB_TOKEN
adroit auth github             # …or store it locally (prompts, hidden input)
```

## The PR-is-the-decision workflow

```sh
adroit new "Use PostgreSQL" --forge          # ADR + linked tracker issue + a draft PR
adroit review 21 --forge                      # post the review-kickoff as a PR/issue comment
adroit set-status 21 accepted --forge --yes   # verify approvals + CI, merge the PR, close the issue
```

- **`new --forge`** creates the ADR, opens a linked **tracker issue** and a
  **draft PR** off an `adr/NNNN-…` branch, and records both URLs in the ADR's
  `## References` section.
- **`review N --forge`** posts the review-kickoff doc as a comment on the linked
  issue/PR.
- **`set-status N accepted --forge`** verifies the required approvals + CI, then
  merges the PR and closes the issue; `rejected` / `deprecated` close them. It
  **refuses if the PR is blocked**, and previews unless you pass `--yes`.

Every `--forge` action accepts `--dry-run` (preview) and `--yes` (apply); without
`--yes` you get a preview, mirroring `adroit migrate`.

## Keeping things in sync

| Verb | What it does |
|---|---|
| `adroit sync <ID>` | Refresh the linked PR/MR description from the ADR's current content (preview; `--yes` to apply) |
| `adroit reconcile` | Detect + fix drift when status changed on the forge out-of-band (e.g. a PR merged in the web UI). Reports by default; `--yes` applies the fixable drift |
| `adroit notify <ID>` | Post the ADR's current state to a chat webhook (Slack/Teams-compatible); `--dry-run` to preview |

## Issue trackers

By default the tracker is the forge's **native** issues. To pair a GitHub/GitLab
forge with a separate tracker, set `forge.tracker` (e.g. `jira`) plus
`forge.tracker_project` and `forge.tracker_host`, and provide that tracker's token
via `adroit auth jira` / `ADROIT_JIRA_TOKEN` (with `ADROIT_JIRA_EMAIL` for Jira
Cloud). The full key list is in the [forge config reference](../reference/cli.md#configuration).

## In CI

The same verbs run in a pipeline — `adroit check` / `adroit index --check` gate
the repo, and `adroit review --forge` / `set-status --forge` drive the decision
PR. See [CI Integration](./ci-integration.md) for ready-to-copy GitHub/GitLab
templates.
