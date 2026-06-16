# Automation & AI

adroit's read verbs can emit structured **JSON** so scripts, CI, and AI agents
can drive the CLI without scraping human-formatted tables. The CLI is the
integration surface — there's no separate API to learn.

## `-o json` on the read verbs (and `import`)

Pass `-o` / `--output json` (it's global, so it works before or after the verb)
to get machine-readable output from the read verbs — `list`, `show`, `status`,
`search`, `stats`, `graph`, `check`, `lint`, `related`, `dedupe`, `ask`, and
`plan`:

```sh
adroit list -o json
adroit show 21 -o json
adroit status 21 -o json
adroit search "postgres" -o json
adroit stats -o json
adroit graph -o json
adroit check -o json
adroit lint 21 -o json
adroit related 21 -o json
adroit ask "why ureq over reqwest?" -o json   # needs an AI provider
adroit plan 21 -o json                        # provider-free once a plan is stored (--save)
```

One **write** verb honors it too: `import` replaces its human report with a
machine **seed summary** (`ImportSummary`), so a loop runner can assert an
ingest result — what was seeded, what the dedupe guard skipped, and (on an
`--ai` run) what the draft sanitizer dropped — without scraping prose.
`--dry-run -o json` previews the same shape with nothing written (each entry's
`reference` is `null` there: identity is allocated only on write, and isn't
predictable under every naming scheme):

```sh
adroit import --from-assessment maturity.yaml -o json             # seed + summary
adroit import --from-assessment maturity.yaml --dry-run -o json   # same shape, no writes
```

**Sanitizer drop telemetry (`--ai`).** When `import --ai` has the model flesh
out a seed, every draft is mechanically [sanitized](#ai-assisted-authoring)
before the splice — model-shaped filler (bracket placeholders, trailing chat
residue, skeleton echoes, …) is dropped silently. Without telemetry the
artifacts can't distinguish *"the model emitted nothing bad"* from *"the
sanitizer ate it."* So an `--ai` run that drops anything carries a `sanitized`
object — per-rule drop counts (`bracket_placeholder`, `residue`,
`skeleton_echo`, `identity_echo`, `marker_echo`), aggregated across every seed —
in the JSON summary, and prints one human line to stderr
(`sanitized: 2 bracket-placeholder, 1 residue`). The counts are **non-blank
content lines** (whitespace the splice normalizes doesn't inflate them), and
**zero rules are omitted** — a clean run (and any run without `--ai`) omits the
field entirely, so the legacy shape is unchanged. Additive (`manifest_schema`
stays `1`).

The JSON is the same `view` contract the [web dashboard](./web.md)'s API returns
— so a tool can consume `adroit list -o json` locally or `GET /api/adrs` from a
running `serve`, and get identical shapes. Key types:

| Verb | JSON shape |
|---|---|
| `list` / `search` | array of `AdrSummary` (`reference`, `address`, `title`, `status`, `supersedes`, `review_due`, …) |
| `show` | `AdrDetail` — the summary fields flattened to the top level, plus `body`, `plan` (the stored implementation plan, or `null`), `related`, `history`, `last_modified` |
| `status` | `Status` — the ADR's status as a JSON string (e.g. `"Accepted"`); the human form is the bare lowercase word |
| `stats` | `Stats` — `total`, `by_status`, `proposed_age`, `review_due`, `created_over_time` |
| `graph` | `Graph` — `nodes` + directed `edges` (supersession + typed links) |
| `check` | `CheckReport` — `checked`, `problems[]` (each with `severity`, `kind`, `message`, `file`) |
| `lint` | array of `LintFinding` (`source`, `severity`, `message`) — empty when clean; only `error`-severity findings affect the exit code |
| `related` / `dedupe` | array of `Match` (`reference`, `title`, `score`) — the ranked mechanical-similarity hits |
| `ask` | `AskAnswer` — `{ answer, sources }` (the ADRs the answer cites) |
| `plan` | `Plan` — `{ reference, title, plan, stored }`, the markdown plan tagged with its ADR identity; `stored: true` when it's the plan persisted in the document (a deterministic, provider-free read) |
| `import` | `ImportSummary` — `{ source, assessment, dry_run, seeded: [{ reference, title, status, domain }], skipped: [titles], sanitized? }`; counts are the array lengths, `status` is the seeded lifecycle status (`"Proposed"` unless `default_status` overrides). `sanitized` is present **only on an `--ai` run that dropped something**: a `SanitizeReport` of per-rule drop counts (`bracket_placeholder`, `residue`, `skeleton_echo`, `identity_echo`, `marker_echo`), with zero rules omitted — so a loop runner can tell "the model emitted nothing bad" from "the sanitizer ate it" |

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
  `idempotent`, the `cost` profile (`local` / `provider-call` / `network` /
  `long-running` — what to rate-limit or confirm on), the lifecycle `stage`, the
  `-o json` output shape (`json_output`), any runtime `requires` (e.g.
  `["ai", "ai.enabled"]` or `["forge config"]` — the command is compiled but still
  needs an opt-in), and the `exit`-code meaning. A boolean switch is marked
  `"flag": true`. An arg that **escalates** its verb beyond the declared per-verb
  semantics carries `"escalates"`: `"forge"` (reaches — or applies/previews
  reaching — the forge, e.g. `review --forge` / `list --forge`), `"file-output"`
  (writes an arbitrary local file, e.g. `plan --out`), or `"writes"` (mutates the
  corpus — the whole `plan --save` control surface: `--save` / `--force` /
  `--dry-run`, plus `--regenerate`, which forces a fresh nondeterministic
  provider call where the stored read is free) — so a safety filter can allowlist
  per **(verb, flag)** mechanically instead of hardcoding flag denylists.
  `writes` itself is honest about the filesystem, not just the corpus: `publish`
  is classified `"writes": true` because producing an output tree is a write.
  A verb's `cost` / `requires` are its conservative worst case: `plan` declares
  `provider-call` + `ai`, but once a plan is **stored** (`--save`, ADR-0008) the
  bare read is local, deterministic, and needs no provider.
- **`types`** — JSON Schemas for the structured shapes the read verbs emit, so a
  consumer can validate an output before consuming it. Every command's
  `json_output` name resolves here: the `view` types (`AdrSummary` / `AdrDetail` /
  `Stats` / `Graph` / `CheckReport`) for `list` / `show` / `stats` / `graph` /
  `check`, plus `Status` (`status`), `LintFinding` (`lint`), `Match` (`dedupe` /
  `related`), `AskAnswer` (`ask`), `Plan` (`plan`), and `ImportSummary`
  (`import` — the one write verb with a structured report; its optional
  `sanitized` field nests a `SanitizeReport` of `--ai` draft-sanitizer drop
  counts). A `[]` suffix on a `json_output` name means an array of that type.
- **`global_options`** + `tool` / `version` / `manifest_schema` (the version of the
  manifest's own shape — bumped on a breaking change).

The syntax is derived from the clap command tree and the type schemas from the
same serde structs that produce `-o json`, so the manifest **always matches the
build**: feature-gated commands appear only when compiled in, and `requires` flags
the ones that exist but need a runtime opt-in. It backs the `adroit mcp` server
(below). The human-facing introspection still works too:

- `adroit --help` lists every verb grouped by workflow; `adroit <verb> --help`
  details one (terse with `-h`).
- `adroit completions <bash|zsh|fish|…>` prints a shell-completion script.

## Driving adroit over MCP — `adroit mcp`

`adroit mcp` runs a [Model Context Protocol](https://modelcontextprotocol.io)
server on **stdio** (JSON-RPC 2.0), so an MCP client — Claude / Claude Code, an
editor, the portfolio's Adopt-stage engine, any agent — drives adroit as a
first-class tool server instead of scraping `--help` or shelling out by hand.

It is a **projection of the manifest**: every **read-only** verb (`list`, `show`,
`search`, `stats`, `graph`, `check`, `plan`, `related`, `dedupe`, `summarize`,
`ask`, …) becomes an MCP **tool**, with its arguments as the tool's JSON Schema
(`inputSchema`); a `tools/call` runs the verb and returns its `-o json` output.
Because it's projected, it **can't drift** — a new read verb appears as a tool
automatically.

**Read-only by design — flag set included.** Only verbs the manifest marks
read-only and side-effect-free are exposed — no repo mutations (`new` /
`set-status` / `supersede`), no network verbs (`sync` / `notify`), no `publish`
(classified a write: it produces an output tree). And because a *flag* can
escalate a read verb, args the manifest marks `escalates` are **stripped** from
the projected tool schemas: `review` is exposed without `--forge` / `--yes` /
`--dry-run` / `--out`, `plan` / `summarize` without `--out`, `plan` also without
`--save` / `--force` / `--regenerate` / `--dry-run` (so once a plan is stored,
the projected `plan` tool is a deterministic, provider-free read), `list` /
`check` without `--forge`. The conformance, pinned by tests: **no projected tool
can mutate the repo, the forge, or the filesystem.** An agent can *read*
decisions + plans over MCP but can't change anything; the CLI remains the escape
hatch for the stripped flags. (Exposing the mutating verbs behind an explicit opt-in is a
future addition — see the [roadmap](../dev/roadmap.md#agent-surface).)

Point a client at it like any stdio MCP server — the command is `adroit mcp` (add
`--dir <path>` to pick the ADR repo). For example, in an MCP client config:

```json
{
  "mcpServers": {
    "adroit": { "command": "adroit", "args": ["--dir", "/path/to/repo/adr", "mcp"] }
  }
}
```

Built behind the default-on `mcp` Cargo feature (it needs `manifest`); a
`--no-default-features` core drops the command.

## AI-assisted authoring

`adroit new --interview` runs a short Socratic interview (problem, drivers,
options, risks) and has a configured AI provider draft the ADR body from your
answers plus the existing corpus, so a new ADR matches the team's voice. The
draft is marked `<!-- adroit:ai-suggested -->` and opened in your editor — you
review and edit before committing.

**Determinism guard:** the AI only ever writes *prose*. Identity, the
`# ADR-NNNN: Title` heading, status, dates, and supersession links stay
mechanical in the write path. Model output is also mechanically **sanitized**
before the splice — small local models re-emit shapes the prompts forbid: a
re-emitted leading title H1 and `> State:` banner are dropped (the mechanical
heading is preserved by the splice, so they would duplicate); a re-emitted
`## Status` / `## Stakeholders` skeleton section is dropped wherever it appears
(the splice preserves the document's own — a model copy is always a duplicate);
echoed adroit markers (`<!-- adroit:ai-suggested -->`,
`<!-- adroit:seeded-from-assessment -->`) are dropped; trailing conversational
residue ("Please review this revised ADR body…", "Let me know if…") is stripped,
along with the horizontal rule such a closer orphans; **whole-line bracket
placeholders** (`[Insert implementation plan or other details as needed]`,
`[Your Name]` — *novel* filler the template never contained) are dropped
wherever they appear, again with any horizontal rule a tail placeholder orphans
— detection is conservative (a curated opener list on a word boundary), so
markdown links, checkboxes, citations, footnotes, single-token `[section]`
lines, and anything inside fenced or indented code are never touched; and a
model-written `## Implementation` section with real content is retitled
`## Implementation notes`, so an AI draft can never read as the hand-written
section that blocks `plan --save` (the `## Implementation` heading belongs to
the managed plan — see `plan` below). Content inside a marker-bracketed stored
plan span always stays verbatim. If no provider is available, `--interview`
degrades to the plain template (the ADR is still created).

These drops are otherwise **silent** — so `import --ai` makes them observable:
it counts what each rule dropped and surfaces the totals (a stderr
`sanitized: N bracket-placeholder, M residue …` line, and a `sanitized` object
in `-o json`). See [Sanitizer drop telemetry](#-o-json-on-the-read-verbs-and-import).

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

`adroit plan <ID>` produces an implementation plan for an (accepted) ADR — and,
since plans are **decision content**, can persist it *inside* the ADR document.
Generation reads the ADR plus the corpus and asks the provider for an ordered
implementation checklist (steps, components touched, testing, rollout, risks),
printed to stdout (or `--out`). `plan <ID> --save` persists that checklist into
the document as a `<!-- adroit:plan -->`-marked `## Implementation` section
(replacing the template's placeholder section; it refuses to overwrite an
existing stored plan without `--force`, refuses to touch a hand-written
`## Implementation` section at all, and `--save --dry-run` previews without
writing). Once a plan is stored, a bare `plan <ID>` returns it **verbatim,
deterministically, with no AI provider** — reading a decision's plan is a pure
corpus read; only `--regenerate` (print a fresh draft) or `--save --force`
(replace it) call the provider again. `adroit plan <ID> -o json` emits a `Plan`
envelope (`{ reference, title, plan, stored }`) — the markdown plan tagged with
its ADR identity, `stored: true` when it came from (or was just saved into) the
document — so a downstream agent gets the decision *and* its implementation
steps as one structured artifact, and `show <ID> -o json` carries the stored
plan as its `plan` field alongside the body.

`adroit lint <ID>` checks one ADR's authoring quality. Its mechanical checks
(sections still left as their `_…_` prompt, a missing/empty Negative
Consequences section — `##` and `###` depth both accepted, fewer than two
recorded options — list items and `###` sub-headings both count) need
**no provider**, so `lint` is usable as an authoring gate in CI. Findings carry
a `severity`: **errors** (an unfinished draft) make `lint` exit non-zero;
**warnings** — a repeated top-level section (usually a model echo of the
template), or a whole-line bracket placeholder (`[Insert …]` / `[Your Name]` /
`[TBD]`-shaped filler; fenced code exempt) — are printed (and serialized) but
don't gate, mirroring `check`'s error/warning split. `lint --ai` adds an
advisory model review on top (always a warning).

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

**Ollama context window:** every ollama request pins `options.num_ctx` to
**8192** (`OLLAMA_NUM_CTX`). Without the pin, ollama **silently truncates** the
prompt at its default window (2048 tokens) — a corpus-bearing prompt leaves
almost no generation room and output clips mid-sentence with no error (found as
the root cause of the suite's run-1 authoring retries). Mind the memory trade
when running ollama with parallel clients: each parallel runner
(`OLLAMA_NUM_PARALLEL`) allocates its own KV cache scaled by `num_ctx`, so the
wider window multiplies across lanes. The pin is captured from the literal wire
JSON by `tests/ai_rig.rs`, so it can't silently regress with a rig upgrade.

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
