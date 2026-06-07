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

## Discovering commands — `adroit manifest`

`adroit manifest` prints a **machine-readable JSON catalog** of the whole CLI
surface, so an agent can discover and drive adroit without scraping `--help`:

```sh
adroit manifest          # one JSON document; always available, offline
```

Three parts, none of which can drift from the binary:

- **`commands`** — every command compiled into this build, with its args / flags /
  enums / defaults, plus the semantics `--help` only implies: `reads` / `writes`,
  `idempotent`, the lifecycle `stage`, the `-o json` output shape (`json_output`),
  any runtime `requires` (e.g. `["ai", "ai.enabled"]` or `["forge config"]` — the
  command is compiled but still needs an opt-in), and the `exit`-code meaning. A
  boolean switch is marked `"flag": true`.
- **`types`** — JSON Schemas for the `view` types the read verbs emit
  (`AdrSummary` / `AdrDetail` / `Stats` / `Graph` / `CheckReport`), so a consumer
  knows the exact shape of `list -o json`, `show -o json`, `check -o json`, etc.
- **`global_options`** + `tool` / `version` / `manifest_schema` (the version of the
  manifest's own shape — bumped on a breaking change).

The syntax is derived from the clap command tree and the type schemas from the
same serde structs that produce `-o json`, so the manifest **always matches the
build**: feature-gated commands appear only when compiled in, and `requires` flags
the ones that exist but need a runtime opt-in. It's the natural backing for an MCP
tool catalog (each command → a tool with its args as a JSON Schema). The
human-facing introspection still works too:

- `adroit --help` lists every verb grouped by workflow; `adroit <verb> --help`
  details one (terse with `-h`).
- `adroit completions <bash|zsh|fish|…>` prints a shell-completion script.

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

**Cost notice + draft journal:** before each provider call adroit prints a
one-line token estimate (`~N input tokens, up to M generated`) to stderr, so a
large call never happens silently. When the model returns a draft, the raw output
is journaled to a git-ignored `<adr>.md.draft` sidecar **before** it's spliced in
— so it survives a failed write or a botched edit (resume or discard it). The
sidecar's extension isn't `.md`, so adroit never treats it as an ADR. Add
`*.draft` to your repo's `.gitignore`.

`adroit draft <ID>` is the after-the-fact version of `--interview`: it runs the
*same* interview on an ADR you already created with a plain `new` (a bare
template), drafts the body, splices it in (heading/status stay mechanical), marks
it `<!-- adroit:ai-suggested -->`, and opens your editor. The iterative flow:
`new` → `draft` (AI fill, whenever) → `edit` → PR.

`adroit compose <ID> "<instruction>"` is the **targeted** revision verb. Where
`draft` re-runs the fixed interview and redrafts the whole body, `compose` takes a
free-form instruction (e.g. `"expand the negative consequences"`,
`"add a rejected option about Redis"`) plus the ADR's *current* body and returns a
revised body — for iterative edits to an ADR that already has content. It writes the
revision (marked `<!-- adroit:ai-suggested -->`, heading/status stay mechanical) and
opens your editor (`--no-edit` to skip). It's the same engine as the TUI's "AI:
draft / revise body" assist, and needs a provider.

`adroit plan <ID>` is the read-only companion: it reads an (accepted) ADR plus
the corpus and asks the provider for an ordered implementation checklist (steps,
components touched, testing, rollout, risks). It prints to stdout (or `--out`) and
never modifies the ADR.

`adroit lint <ID>` checks one ADR's authoring quality. Its mechanical checks
(sections still left as their `_…_` prompt, missing negative consequences, single
option) need **no provider** and exit non-zero on findings, so `lint` is usable as
an authoring gate in CI; `lint --ai` adds an advisory model review on top.

`adroit summarize <ID>` prints a one-paragraph plain-language TL;DR of an ADR —
handy for a PR description, a notification, or a decision-log entry (read-only).

`adroit ask "<question>"` answers a question about the corpus: it retrieves the
most relevant ADRs **mechanically** (no embeddings) and the provider synthesizes
an answer with citations. `adroit related` / `adroit dedupe` are the fully
mechanical similarity verbs and need **no provider** at all.

### Enabling it

The AI adapters are in the default build (the `ai` Cargo feature is on by
default; it brings rig + tokio, while a `--no-default-features` core stays sync).
You just opt in at runtime via config:

```sh
just build                         # the default binary already includes the AI verbs
adroit auth anthropic              # store the key in the OS keychain (or export ADROIT_ANTHROPIC_KEY)
```

`adroit auth anthropic` saves the key to the **OS keychain** (falling back to a
`0600` file), the same store as the forge tokens — so the key needn't live in a
plaintext `.env`. `ADROIT_ANTHROPIC_KEY` still takes precedence when set.

Enable it either in `config.yaml`:

```yaml
# config.yaml
ai:
  enabled: true            # kill-switch — AI calls only happen when true
  provider: anthropic      # or: ollama (local, no key; air-gapped)
  model: claude-sonnet-4-6 # or an Ollama model like llama3.2
  # host: http://localhost:11434   # ollama base URL override (optional)
```

…or entirely via environment / `.env` (these `ADROIT_AI_*` vars override the
config section, so you can enable AI without editing `config.yaml`):

```sh
# .env  (git-ignored)
ADROIT_AI_ENABLED=true
ADROIT_AI_PROVIDER=anthropic        # or ollama
ADROIT_AI_MODEL=claude-sonnet-4-6
ADROIT_ANTHROPIC_KEY=sk-ant-...     # anthropic only
# ADROIT_AI_HOST=http://localhost:11434   # ollama only
```

| Provider | Auth | Notes |
|---|---|---|
| `anthropic` | `ADROIT_ANTHROPIC_KEY` / `adroit auth anthropic` | Hosted Claude |
| `ollama` | none | Local models; set `ai.host` for a remote instance |

The AI layer is built on the **rig** framework (provider-agnostic LLM adapters),
chosen so the provider stays swappable — see the
[AI-authoring RFC](https://github.com/como-technologies/adroit/issues/5).

### Testing without a provider

Set `ADROIT_AI_FAKE=<canned response>` to drive `new --interview` with an offline
stand-in (no network, no `ai` feature) — useful in tests and CI to exercise the
flow without spending tokens.

## Why this exists

This structured surface is the foundation for AI-assisted authoring — see the
[AI-authoring RFC](https://github.com/como-technologies/adroit/issues/5). The goal
is that an agent can list, read, search, and validate ADRs through the same verbs
a person uses, then propose changes a human reviews before commit.
