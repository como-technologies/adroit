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
just build         # debug build  → target/debug/adroit  (CLI + TUI)
just release       # release build → target/release/adroit
```

The `web` feature is off by default (it needs the Vue bundle). Build/run it via
`just serve` (below) or `cargo run --features web -- serve`.

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
flags). The full set, beyond the cheatsheet: `link`, `relink`, `renumber`,
`migrate`, and `config` round out collisions, link hygiene, and profile changes.

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
