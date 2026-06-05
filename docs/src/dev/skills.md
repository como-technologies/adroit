# Project skills

adroit ships a set of repo-local **skills** under
[`.claude/skills/`](https://github.com/como-technologies/adroit/tree/main/.claude/skills)
— invokable procedures that capture the recurring development workflows so they run
the same way every time. They are committed with the repo, so anyone (or any AI
session) working on adroit gets them.

Each skill is a thin, adroit-specific orchestration that **composes** the generic
"superpowers" skills (`test-driven-development`, `systematic-debugging`,
`verification-before-completion`) — it doesn't re-derive them. Invoke one by name,
e.g. `/harden` or `/gate`.

| Skill | When to use | What it does |
|---|---|---|
| `/harden` | "find bugs in X", "harden / fuzz X", widening coverage, before a release | Runs a bug-hunting campaign: build/extend the oracle or a property/fuzz/fault-injection harness, soak it, and turn every finding into a root-cause fix + regression. |
| `/gate` | before committing or finishing work | The pre-commit quality gate: fmt + clippy and tests across default/forge/web + `just book`; commit only the changed files; **stop before pushing**. |
| `/doc-sync` | after changing behavior, or a periodic sweep | Keep code and docs in sync: update `CLAUDE.md` + the mdbook, verify by running the CLI, build the book. |
| `/extend` | "add a gitea/bitbucket provider", "add a naming scheme", "add a publish adapter", "add a config key" | Scaffold a new variant of an adroit seam (forge provider, tracker, naming scheme, format, layout, publish adapter, template, config key, CLI subcommand) with the tests + docs each requires. |

## `/harden`

The bug-hunting campaign behind every suite on the [Hardening & Quality](./hardening.md)
page. It encodes the design rules — the oracle is an *outcome predictor, not a
reimplementation*; drive the real binary; read identity back for non-deterministic
schemes; keep it git-free and clock-pinned — the soak knobs (`PROPTEST_CASES`,
bolero), the **explore → triage → crystallize** triage taxonomy, and a
"where-bugs-hide-in-adroit" checklist. See [Testing & Fuzzing](./testing.md) for the
underlying suites.

## `/gate`

The concrete ship gate (see also [Testing & Fuzzing](./testing.md)). Beyond the
fmt/clippy/test/book run it bakes in the lessons that cost us a red CI: run the
*whole* gate (fail-fast hides later breaks), reproduce the **clean-checkout** state
when touching the `web` feature (the `web/dist/.gitkeep` rust-embed requirement),
re-`cargo audit` after adding a dependency, stage only the specific files, and
**never push without explicit permission**.

## `/doc-sync`

The code↔docs sweep. One doc system — the mdbook (`docs/src/`); never standalone
docs. Update `CLAUDE.md`, `README.md`, and the relevant book pages together; verify
what the binary *actually* does by running it; `just book` must build; keep examples
generic.

## `/extend`

The fan-out helper for adding a seam variant. adroit is built so a new variant edits
one module + one match arm; this skill is the per-seam checklist (files, pattern,
tests, docs) for forge providers, trackers, naming schemes, formats, layouts,
publish adapters, templates, config keys, and CLI subcommands — including the gotcha
that a flag-settable config key must also be wired into `config_cli_value`.

> These skills and the always-on rules in `CLAUDE.md` overlap on purpose:
> `CLAUDE.md` is the guardrail that's always loaded; a skill is the invokable
> procedure. When you change one, check the other.
