# Decision Records (adr/)

adroit dogfoods itself: its own architecture decisions live in the top-level
**`adr/`** corpus (markdown / by-status, sequential numbering), authored with
the `adroit` binary — never by hand-editing status or identity. The corpus is
the record of *why the code is the way it is*: the AI provider seam, the
statelessness invariant, the manifest semantics table, the MCP read-only
projection, and the standing direction decisions all live there as accepted
ADRs (ADR-0001 records why the corpus itself exists — including the earlier,
deliberate decision to *remove* it, which the current portfolio-wide
"every repo carries its own corpus" mandate reversed).

## The one rule: always `--dir adr`

In this workspace `ADROIT_DIR` (set in `.env`) points at an **external dogfood
target repo**, not at adroit's own decisions. A bare `adroit new` therefore
writes into the wrong corpus. Every command that touches adroit's own ADRs
must pass `--dir adr` explicitly:

```sh
just build
./target/debug/adroit new "Short imperative title" --dir adr --no-edit
./target/debug/adroit set-status <N> accepted --dir adr
./target/debug/adroit list --dir adr
./target/debug/adroit check --dir adr
```

## CI gate (self-hosted)

`just ci` includes the **`adr-check`** recipe: it builds adroit, then runs the
freshly built binary against its own corpus —

```sh
just adr-check    # = cargo build && ./target/debug/adroit check --dir adr
```

— so a structurally broken corpus (status/dir mismatch, duplicate identifiers,
broken links or supersession) fails CI the same way a failing test does. New
ADRs should also pass `adroit lint <N> --dir adr` clean before they are
accepted (no prompt-only sections, honest negative consequences, at least two
considered options).

## Recording a decision

The [`/adr` skill](./skills.md#adr) is the worked flow. In short:

1. `./target/debug/adroit new "Title" --dir adr --no-edit` scaffolds
   `adr/proposed/NNNN-….md`.
2. Fill **Context / Decision Drivers / Considered Options / Decision Outcome**
   (with honest **Negative Consequences**) in the house voice of the existing
   ADRs.
3. `adroit check --dir adr` and `adroit lint <N> --dir adr` must be clean.
4. `adroit set-status <N> accepted --dir adr` once decided — a real status
   transition, so the corpus history is genuine. Use `supersede` when a new
   decision replaces an old one.

Keep the records **generic**: they are public — never name a client, their
internal tech, or titles, even when the decision surfaced while dogfooding
against a client-shaped repo.

## Deliberately separate from this book

`adr/` holds *decision records*; `docs/src/` is the *user manual*. They serve
different readers and change for different reasons — don't migrate ADRs into
the book, and don't record decisions only in book prose. When a decision
changes behavior, do both: record the ADR **and** doc-sync the affected book
pages.
