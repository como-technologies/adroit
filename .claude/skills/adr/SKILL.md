---
name: adr
description: Use when working ON adroit and making an architectural decision worth recording — choosing a dependency, a module seam, an on-disk format, an async/sync boundary, etc. Invoke for "should we record an ADR for this", "draft an ADR for <decision>", "accept/supersede adroit ADR N". Records the decision in adroit's OWN `adr/` corpus, using adroit itself (dogfooding).
user-invocable: true
---

# Record adroit's own architecture decisions (in `adr/`)

adroit dogfoods itself: its architecture decisions live in the top-level **`adr/`**
corpus (markdown / by-status), authored with the `adroit` binary. This composes
the generic `use-adroit` skill with the facts specific to this repo.

## Critical: always pass `--dir adr`
`ADROIT_DIR` in this workspace points at the **dogfood target repo**, not adroit's
own decisions. So every command for an adroit ADR MUST be explicit:
```sh
./target/debug/adroit new "Title" --dir adr [--no-edit]
./target/debug/adroit set-status <N> accepted --dir adr
./target/debug/adroit list --dir adr
./target/debug/adroit check --dir adr
```
A bare `adroit new` would write into the dogfood repo by mistake.

## When to record one
A decision belongs in `adr/` when it shapes adroit's architecture and a future
contributor would ask "why is it this way?": a new dependency (e.g. ADR-0001
adopting rig), a sync/async boundary, a seam design, an on-disk format change, a
feature-gating choice. Routine bug fixes and refactors don't.

## Flow
1. `adroit new "Short imperative title" --dir adr --no-edit` — scaffolds
   `adr/proposed/NNNN-…md` (or use `--interview` if an AI provider is configured).
2. Fill **Context / Decision Drivers / Considered Options / Decision Outcome**
   (with honest **Negative Consequences**) and **Implementation**. Match the house
   voice of the existing ADRs.
3. `adroit check --dir adr` — must be clean.
4. `adroit set-status NNNN accepted --dir adr` once decided (moves it to
   `accepted/`). Use `supersede A B --dir adr` when a new ADR replaces an old one.

## Keep it generic
These ADRs are public. **Never** name a client, their internal tech, or titles
(see the project's "no client names" rule) — keep examples generic, even when the
decision was surfaced while dogfooding against a client repo.

## Don't fight the existing doc rule
ADRs in `adr/` are decision records, deliberately **separate** from the mdbook
user manual (`docs/src/`). Don't migrate `adr/` into the book or hand-edit files
to change status/identity — use the verbs.
