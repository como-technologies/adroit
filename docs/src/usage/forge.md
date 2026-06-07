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
