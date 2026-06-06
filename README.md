# adroit

A snappy tool for managing Architecture Decision Records — the name hides
**ADR** in plain sight.

> **Status: Phase 1 — dogfooding.** We use adroit on our own consulting gigs to
> manage ADRs in-repo. Not published yet; build from source. Expect rough edges.

By default it speaks a **status-by-directory** convention: status is the
directory (`proposed/`, `accepted/`, `rejected/`, `superseded/`, `deprecated/`), number and
title come from the `# ADR-NNNN: Title` heading, no YAML frontmatter. A
frontmatter/flat profile is also supported.

## Three ways to use it

One binary, three surfaces over the same ADR repo:

- **CLI** — fast capture and scripting. Always available.
- **TUI** — interactive browse / triage / in-terminal editing. Run bare
  `adroit`. On by default (`tui` feature).
- **Web** — read-only dashboard (Dashboard / Browse+Search / Insights), with
  live-reload. `adroit serve`, behind the `web` feature.

## Build

```sh
just init          # one-time: install toolchain (clippy, rustfmt, mdbook, …)
just build         # debug build  → target/debug/adroit  (TUI + AI + forge)
just release       # release build → target/release/adroit
```

### Features

`just build` gives you the full binary: the TUI plus the **AI** and **forge**
integrations. Each is a Cargo feature, and the bare core still builds without any
of them (`cargo build --no-default-features` / `just build-core`) — small and
synchronous (no tui, no rig/tokio, no http client).

| Feature | Default? | Adds |
|---|---|---|
| `tui` | ✅ | the interactive TUI (bare `adroit`) |
| `ai` | ✅ | AI authoring: `new --interview`, `draft`, `plan`, `lint --ai`, `summarize`, `ask` (Anthropic or local Ollama). Calls are still gated at runtime by `ai.enabled` |
| `forge` | ✅ | GitHub/GitLab issue + PR/MR sync: `init`, `auth`, `sync`, `reconcile`, `notify` |
| `web` | — | the read-only web dashboard. Opt-in (it needs the Vue SPA bundle); build + run with `just serve` |

## Test

```sh
just ci            # the full gate: fmt, clippy, all suites, book, audit
just test          # default-feature tests (TUI + AI + forge: unit + CLI + oracle + parsers)
just test-core     # the bare core (--no-default-features); just test-web for the dashboard
just model         # wide property soak (PROPTEST_CASES, default 2000)
```

adroit has a model-based ("oracle") tester that drives the real binary through
random command sequences across the format × layout × scheme matrix, plus parser
fuzzing (incl. coverage-guided via bolero), forge fault-injection, and dashboard
XSS tests. See
[Testing & Fuzzing](docs/src/dev/testing.md) for how to run, soak, extend, and
triage them — and how to drive an AI assistant to do it — and
[Hardening & Quality](docs/src/dev/hardening.md) for the bug-finding campaign.

## Point it at your ADR repo

Pass `--dir`, or set it once and forget it:

```sh
cp .env.example .env      # then edit ADROIT_DIR (git-ignored)
# .env:  ADROIT_DIR=/path/to/your-repo/src/adrs
```

`--dir` / `ADROIT_DIR` work for every command and surface. Precedence:
flag > env/`.env` > `~/.config/adroit/config.yaml` > default.

`adroit config` shows every setting, its resolved value, and which of those
layers it came from; `adroit config set <key> <value>` persists a default (add
`--local` to write the project `.env` instead).

## CLI cheatsheet

```sh
adroit new "Use PostgreSQL for the datastore"   # next number, scaffolds proposed/, opens $EDITOR
adroit list                             # or: --status accepted
adroit search postgres
adroit status 9                         # getter: prints the status (lowercase, scriptable)
adroit set-status 9 accepted            # setter: moves the file + rewrites ## Status
adroit supersede 9 4                    # 9 supersedes 4 (moves 4, links both)
adroit link 9 --depends-on 4            # typed relational link (frontmatter profile)
adroit set-review 9 2026-07-15          # review deadline (review-due once past)
adroit review 9 --output kickoff.md     # generate the MR review-kickoff doc
adroit index                            # refresh SUMMARY.md, grouped by status
adroit check                            # CI gate: validate the ADR repo (non-zero on problems)
adroit index --check                    # CI gate: fail if SUMMARY.md is stale
adroit config                           # list every setting and where it came from
```

`adroit --help` lists every command (and `adroit <cmd> --help` the per-command
flags), grouped by workflow stage — author → review & decide → explore →
maintain. The full set, beyond the cheatsheet: `link`, `relink`, `renumber`,
`migrate`, and `config` round out collisions, link hygiene, and profile changes.

## AI-assisted authoring (opt-in)

The AI verbs are in the default build — just enable them via config or
`ADROIT_AI_ENABLED=true` and pick a provider (hosted Anthropic, or local Ollama
for an air-gapped, no-key setup):

```sh
adroit new "Adopt event sourcing" --interview   # Socratic Q&A → AI drafts the body
adroit draft 9                                  # run that interview on an existing ADR
adroit lint 9                                   # flag unfilled sections / missing trade-offs
adroit summarize 9                              # one-paragraph TL;DR
adroit plan 9                                   # AI implementation checklist
adroit ask "why did we pick Postgres?"          # corpus Q&A with citations
```

The AI only ever writes *prose* (marked `<!-- adroit:ai-suggested -->`) — identity,
status, dates, and links stay mechanical, and you review before committing. The
mechanical cousins `dedupe`/`related` need no provider at all. See
[Automation & AI](docs/src/usage/automation.md) and
[The ADR Workflow](docs/src/usage/workflow.md).

## Shell completions

`adroit completions <bash|zsh|fish|powershell|elvish>` prints a completion
script generated from the command tree. Source it from your shell rc
(kubectl-style):

```sh
. <(adroit completions bash)     # ~/.bashrc
. <(adroit completions zsh)      # ~/.zshrc
adroit completions fish | source # fish
```

— or save it onto your shell's completion path (see the
[CLI reference](docs/src/reference/cli.md)). It completes subcommands, flags, and
enum values (e.g. `set-status <TAB>`).

## TUI

```sh
adroit                                  # bare command launches the TUI
```

Two panes: list (filter `f`, search `/`, sort `o`) + rendered-markdown preview.
`Enter` focuses the preview, `m` toggles rendered ↔ raw, `i` edits the body
in-terminal, `s`/`S` change status / supersede, `n` new, `e` opens `$EDITOR`,
`q` quits. `--theme gruvbox` for a true-color theme.

## Web dashboard

```sh
just serve                              # build the SPA + serve with live-reload (:8080)
# or manually:
cargo run --features web -- serve --dir /path/to/repo/src/adrs
```

Open the printed `http://127.0.0.1:8080`. Read-only (authoring stays in CLI/TUI);
it auto-refreshes when ADR files change on disk. `Ctrl-C` to stop.

## ADR styles we follow

ADRs originate with Michael Nygard's
[Documenting Architecture Decisions](https://www.cognitect.com/blog/2011/11/15/documenting-architecture-decisions)
— a short, version-controlled record of a decision, its context, and its
consequences. We lean on two well-established conventions and recommend either:

- **[MADR](https://adr.github.io/madr/)** (Markdown Any Decision Records) — our
  default (`--template madr`). A fuller structure (context, decision drivers,
  considered options, outcome, consequences) that holds up when a decision needs
  real justification.
- **[Nygard](https://www.cognitect.com/blog/2011/11/15/documenting-architecture-decisions)**
  (`--template nygard`) — the original minimal form (Status / Context / Decision
  / Consequences). Reach for it when MADR is more ceremony than the decision
  warrants.

We also treat "ADR" broadly — any team decision worth recording, not just
architecture (see Olaf Zimmermann's
[Any Decision Records](https://ozimmer.ch/practices/2021/04/23/AnyDecisionRecords.html)).
For more templates and examples, the
[architecture-decision-record](https://github.com/joelparkerhenderson/architecture-decision-record)
collection is the best reference. Bring your own template with
`--template <path>` or an `adr-template.md` in your repo.

## Bake it into CI

The ADR process fits a GitHub/GitLab pipeline: propose on `main`, then the
PR/MR *is* the decision (move proposed → accepted/rejected). `adroit check` and
`adroit index --check` gate it, and `adroit review` posts the kickoff brief on
the decision PR/MR. Copy-and-customize templates for both platforms live in
[`ci-templates/`](ci-templates/); see
[docs/src/usage/ci-integration.md](docs/src/usage/ci-integration.md).

## More

- User manual: `just book` (source in `docs/`).
- ADR format & both profiles: [docs/src/reference/adr-format.md](docs/src/reference/adr-format.md).
- Naming schemes (`sequential` / `date` / `uuid` / `per_category`): [docs/src/reference/adr-format.md#naming-schemes](docs/src/reference/adr-format.md#naming-schemes).
- Every command: `adroit --help`; every `just` recipe: run `just`.

## License

Apache-2.0
