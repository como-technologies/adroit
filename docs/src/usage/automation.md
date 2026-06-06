# Automation & AI

adroit's read verbs can emit structured **JSON** so scripts, CI, and AI agents
can drive the CLI without scraping human-formatted tables. The CLI is the
integration surface — there's no separate API to learn.

## `-o json` on the read verbs

Pass `-o` / `--output json` (it's global, so it works before or after the verb)
to get machine-readable output from `list`, `show`, `search`, `stats`, `graph`,
and `check`:

```sh
adroit list -o json
adroit show 21 -o json
adroit search "postgres" -o json
adroit stats -o json
adroit graph -o json
adroit check -o json
```

The JSON is the same `view` contract the [web dashboard](./web.md)'s API returns
— so a tool can consume `adroit list -o json` locally or `GET /api/adrs` from a
running `serve`, and get identical shapes. Key types:

| Verb | JSON shape |
|---|---|
| `list` / `search` | array of `AdrSummary` (`reference`, `address`, `title`, `status`, `supersedes`, `review_due`, …) |
| `show` | `AdrDetail` — the summary fields flattened to the top level, plus `body`, `related`, `history`, `last_modified` |
| `stats` | `Stats` — `total`, `by_status`, `proposed_age`, `review_due`, `created_over_time` |
| `graph` | `Graph` — `nodes` + directed `edges` (supersession + typed links) |
| `check` | `CheckReport` — `checked`, `problems[]` (each with `severity`, `kind`, `message`, `file`) |

`json` always goes to **stdout**; human-facing warnings/notes go to **stderr**,
so a consumer can pipe stdout straight into a JSON parser.

## Exit codes

adroit follows the usual convention — `0` on success, non-zero on error — so a
script or agent can branch on the exit code:

- **`adroit check`** is the CI gate: it exits **non-zero** when any
  **Error**-severity problem is present (duplicate identifier, broken link,
  status/dir mismatch, unparseable file, broken supersession), and **`0`** when
  the repo is clean or has only warnings (e.g. transiently stale links on a
  deferred-relink branch). `check -o json` keeps this behavior: the report is
  written to stdout **and** the exit code still reflects the gate.
- A bad identifier, an invalid flag combination, or a profile mismatch exits
  non-zero with a message on stderr.

## Discovering commands

Until a structured command manifest lands, agents can introspect adroit the same
way a human does:

- `adroit --help` lists every verb grouped by workflow; `adroit <verb> --help`
  details one verb (terse with `-h`).
- `adroit completions <bash|zsh|fish|…>` prints a shell-completion script that
  enumerates subcommands, flags, and enum values.

## AI-assisted authoring

`adroit new --interview` runs a short Socratic interview (problem, drivers,
options, risks) and has a configured AI provider draft the ADR body from your
answers plus the existing corpus, so a new ADR matches the team's voice. The
draft is marked `<!-- adroit:ai-suggested -->` and opened in your editor — you
review and edit before committing.

**Determinism guard:** the AI only ever writes *prose*. Identity, the
`# ADR-NNNN: Title` heading, status, dates, and supersession links stay
mechanical in the write path. If no provider is available, `--interview` degrades
to the plain template (the ADR is still created).

`adroit plan <ID>` is the read-only companion: it reads an (accepted) ADR plus
the corpus and asks the provider for an ordered implementation checklist (steps,
components touched, testing, rollout, risks). It prints to stdout (or `--out`) and
never modifies the ADR.

`adroit lint <ID>` checks one ADR's authoring quality. Its mechanical checks
(leftover placeholders, missing negative consequences, single option) need **no
provider** and exit non-zero on findings, so `lint` is usable as an authoring gate
in CI; `lint --ai` adds an advisory model review on top.

`adroit summarize <ID>` prints a one-paragraph plain-language TL;DR of an ADR —
handy for a PR description, a notification, or a decision-log entry (read-only).

`adroit ask "<question>"` answers a question about the corpus: it retrieves the
most relevant ADRs **mechanically** (no embeddings) and the provider synthesizes
an answer with citations. `adroit related` / `adroit dedupe` are the fully
mechanical similarity verbs and need **no provider** at all.

### Enabling it

The AI adapters live behind the `ai` Cargo feature (it brings rig + tokio; the
core CLI stays sync). Build with it, then opt in via config:

```sh
cargo build --features ai          # or: just build (after adding the feature)
adroit auth anthropic              # store the key (or export ADROIT_ANTHROPIC_KEY)
```

```yaml
# config.yaml
ai:
  enabled: true            # kill-switch — AI calls only happen when true
  provider: anthropic      # or: ollama (local, no key; air-gapped)
  model: claude-sonnet-4-6 # or an Ollama model like llama3.2
  # host: http://localhost:11434   # ollama base URL override (optional)
```

| Provider | Auth | Notes |
|---|---|---|
| `anthropic` | `ADROIT_ANTHROPIC_KEY` / `adroit auth anthropic` | Hosted Claude |
| `ollama` | none | Local models; set `ai.host` for a remote instance |

The decision to build the AI layer on rig is recorded in
[ADR-0001](https://github.com/como-technologies/adroit/blob/main/adr/accepted/0001-adopt-rig-for-adroit-s-ai-integration.md).

### Testing without a provider

Set `ADROIT_AI_FAKE=<canned response>` to drive `new --interview` with an offline
stand-in (no network, no `ai` feature) — useful in tests and CI to exercise the
flow without spending tokens.

## Why this exists

This structured surface is the foundation for AI-assisted authoring — see the
[rig adoption decision](https://github.com/como-technologies/adroit/blob/main/adr/accepted/0001-adopt-rig-for-adroit-s-ai-integration.md)
and the AI-authoring RFC. The goal is that an agent can list, read, search, and
validate ADRs through the same verbs a person uses, then propose changes a human
reviews before commit.
