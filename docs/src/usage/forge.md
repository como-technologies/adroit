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
CI) or `adroit auth` — resolution is **env-first**, then the keychain / file store:

```sh
export ADROIT_GITHUB_TOKEN=…   # or ADROIT_GITLAB_TOKEN — best for CI
adroit auth github             # interactive login (see below)
```

`adroit auth` stores the token in your **OS keychain** when available (macOS
Keychain / Windows Credential Manager / Linux kernel keyutils), falling back to a
`0600` file next to the config. `ADROIT_CREDENTIAL_STORE=file|keychain` forces a
backend. The token value is never echoed back.

**OAuth device-flow login (no copy-paste).** Set `forge.oauth_client_id` to a
device-flow-enabled OAuth app's **public** client id and `adroit auth github` /
`gitlab` (with no `--token`) walk you through a browser login — it prints a URL +
short code, you approve in the browser, and adroit stores the granted token:

```sh
adroit config set forge.oauth_client_id <your-oauth-app-client-id>
adroit auth github          # → open the printed URL, enter the code, done
```

With no client id configured (or for `--token` / `jira`), `auth` falls back to a
hidden manual token prompt. Device flow uses a **public** client id — there is no
client secret to manage. You register the OAuth app once (steps below).

### Registering an OAuth app (one-time)

Device flow needs an OAuth application registered with the forge. It's a public
client — **no secret** — so the only value adroit needs is its **client id**.

#### GitHub (and GitHub Enterprise)

1. Go to **Settings → Developer settings → OAuth Apps → New OAuth App**
   (org-owned: the org's **Settings → Developer settings**). On GitHub Enterprise,
   the same path on your GHE host.
2. Fill in:
   - **Application name** — e.g. `adroit (your-org)`.
   - **Homepage URL** — anything (e.g. your repo URL).
   - **Authorization callback URL** — required by the form but unused by device
     flow; reuse the homepage URL.
3. **Register application**, then on the app page tick **Enable Device Flow** and
   **Update application**.
4. Copy the **Client ID** and wire it up:
   ```sh
   adroit config set forge.oauth_client_id <client-id>
   # GitHub Enterprise only — point at your host:
   adroit config set forge.host ghe.example.com/api/v3
   ```

The default scope adroit requests is `repo` (create the ADR issue + PR).

#### GitLab (and self-managed GitLab)

> Device flow needs **GitLab ≥ 17.2** (the OAuth 2.0 Device Authorization Grant).
> Older instances fall back to a manual token.

1. Create an application — for yourself: **Preferences → Applications → Add new
   application** (or a group/instance application for shared use).
2. Fill in:
   - **Name** — e.g. `adroit`.
   - **Redirect URI** — required by the form but unused by device flow; any value
     works (e.g. `https://localhost`).
   - **Confidential** — **unchecked** (device flow is a public client).
   - **Scopes** — check **`api`** (create the MR + issue).
3. **Save application**, copy the **Application ID** (that's the client id):
   ```sh
   adroit config set forge.oauth_client_id <application-id>
   # self-managed GitLab only:
   adroit config set forge.host gitlab.example.com
   ```

### Logging in + verifying (live validation)

```sh
adroit auth github        # or: adroit auth gitlab
```

adroit prints a verification URL + a short user code; open the URL, enter the
code, and approve. On success it stores the granted token (in your keychain) and
prints `Saved … token to the OS keychain …`. Confirm the token actually works
against the forge — these make a live read-only call and should not warn about
auth:

```sh
adroit check --forge      # appends any forge drift; 401 surfaces as an auth error
adroit list --forge       # rows are enriched with linked issue/PR state
```

If you see an authentication error, the token didn't have the needed scope
(`repo` / `api`) — re-register with the correct scope and re-run `adroit auth`.

`forge OAuth login failed: …` during `auth` means the device-code request was
rejected — almost always a wrong/blank `forge.oauth_client_id`, an app without
**device flow enabled**, or (self-hosted) a missing/incorrect `forge.host`. The
message includes the provider's own reason.

## The PR-is-the-decision workflow

```sh
adroit new "Use PostgreSQL" --forge          # ADR + linked tracker issue + a draft PR
adroit review 21 --forge                      # kickoff comment + @-mention reviewers + deadline label
adroit set-review 21 2026-06-20 --forge       # comment + set the tracker's native due/target date
adroit set-status 21 accepted --forge --yes   # verify approvals + CI, merge the PR, close the issue
```

- **`new --forge`** creates the ADR, opens a linked **tracker issue** and a
  **draft PR** off an `adr/NNNN-…` branch, and records both URLs in the ADR's
  `## References` section.
- **`review N --forge`** opens the ADR for formal review: it **marks the draft
  PR/MR ready for review** (un-drafts it), posts the review-kickoff doc as a
  comment, **@-mentions the reviewer pool** (`forge.reviewers`), and tags the PR/MR
  with a `review-by:<deadline>` label (the deadline is the review window's last
  day). (`set-status accepted` also un-drafts before merging, as a safety.) The
  doc's **relative links are rewritten to absolute repo URLs** (e.g.
  `https://github.com/<repo>/blob/<base>/…`) so the "Read the ADR / README / guide"
  links resolve when read in a PR or Linear comment, outside the repo file tree.
- **`set-review N <date> --forge`** comments the deadline **and** sets the
  tracker's **native due/target date** — Jira due date, GitLab issue due date,
  Linear target date, or monday's first date column (GitHub Issues have no due
  date, so it's a no-op there). `--clear` clears it.
- **`set-status N accepted --forge`** verifies the required approvals + CI, then
  merges the PR and closes the issue; `rejected` / `deprecated` close them. It
  **refuses if the PR is blocked**, and previews unless you pass `--yes`. The
  approval count comes from `review_quorum` (default 3); override it for one run
  with `--quorum N` (e.g. `--quorum 1` for a solo repo).

> **Re-running converges — it doesn't spam.** `review --forge` and `set-review
> --forge` post their comment **idempotently**: the comment carries a hidden
> marker, so a re-run *edits adroit's own comment in place* (a no-op if nothing
> changed) instead of adding a duplicate. Labels, the native due date, and the
> un-draft are likewise idempotent. (On monday, which has no edit-update API, a
> re-run still avoids a duplicate but can't refresh the body.)

Every `--forge` action accepts `--dry-run` (preview) and `--yes` (apply); without
`--yes` you get a preview, mirroring `adroit migrate`. **`--dry-run` is a true full
preview — it changes nothing, local *or* forge** (so `new --dry-run` creates no ADR
and opens no editor, and `set-status … --dry-run` doesn't move the file), even
without `--forge`.

## Keeping things in sync

| Verb | What it does |
|---|---|
| `adroit sync <ID>` | Refresh the linked PR/MR description from the ADR's current content (preview; `--yes` to apply) |
| `adroit reconcile` | Detect + fix drift when status changed on the forge out-of-band (e.g. a PR merged in the web UI). Reports by default; `--yes` applies the fixable drift |
| `adroit notify <ID>` | Post the ADR's current state to a chat webhook (Slack/Teams-compatible); `--dry-run` to preview |

## Issue trackers

By default the tracker is the forge's **native** issues (`forge.tracker = native`;
`gh_issues` / `gl_issues` are explicit aliases of the same — the forge's own
Issues, no separate adapter). To pair a GitHub/GitLab forge's **PR/MR** side with a
**separate** issue tracker, set `forge.tracker` and the tracker's location keys,
then provide its token. The decision PR still runs on the forge; only the *issue*
side moves to the chosen tracker.

| `forge.tracker` | `forge.tracker_project` | `forge.tracker_host` | Token (env / `adroit auth …`) |
|---|---|---|---|
| `jira` | project key (`OPS`) | site host (`acme.atlassian.net`, or self-hosted) | `ADROIT_JIRA_TOKEN` (+ `ADROIT_JIRA_EMAIL` for Cloud) |
| `linear` | team **key** (`ENG` — the team Identifier, **not** a Linear *Project*) | — (single host) | `ADROIT_LINEAR_TOKEN` |
| `monday` | board id (numeric) | account subdomain (`acme` → `acme.monday.com`) | `ADROIT_MONDAY_TOKEN` |

All three drive the same lifecycle — `new --forge` files an issue, `set-status
accepted` / `rejected` transitions it (Done / Won't-do), `supersede` comments on +
closes it. **Linear** and **monday** speak GraphQL; **Linear** files to a *team* and
maps status to its workflow-state types (`completed` / `canceled`), while **monday**
files an *item* to a board and matches a Status-column label. Tokens are paste-only
(`adroit auth linear` / `adroit auth monday`); device-flow login is GitHub/GitLab
only. The full key list is in the [forge config reference](../reference/cli.md#configuration).

## In CI

The same verbs run in a pipeline — `adroit check` / `adroit index --check` gate
the repo, and `adroit review --forge` / `set-status --forge` drive the decision
PR. See [CI Integration](./ci-integration.md) for ready-to-copy GitHub/GitLab
templates.
