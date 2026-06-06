---
name: use-adroit
description: Use when a project manages its Architecture Decision Records with adroit and you need to read, author, validate, or transition them from the CLI. Invoke for "what ADRs do we have / what did we decide about X", "create / draft an ADR", "accept / reject / supersede ADR N", "is the ADR repo valid", "plan the implementation of ADR N". Drive the `adroit` CLI (machine-readable `-o json`) rather than hand-editing files.
user-invocable: true
---

# Use adroit to manage a project's ADRs

`adroit` is the source of truth for this project's ADRs. **Drive the CLI** — it
owns identity, dates, status, file layout, and cross-links mechanically. Don't
hand-create or hand-move ADR files; let the verbs do it (re-running a write verb
on unchanged state is a no-op).

Point it at the repo's ADR dir with `--dir` / `ADROIT_DIR` if it isn't the
default. Prefer `-o json` whenever you're parsing output.

## Read (machine-readable: `-o json`)
```sh
adroit list -o json                 # every ADR: reference, status, title, links, review_due
adroit show <id> -o json            # one ADR: summary fields (flattened) + body + history
adroit search "<term>" -o json      # title/body matches
adroit stats -o json                # counts, oldest-proposed, growth
adroit graph -o json                # supersession + typed-link edges
adroit check -o json                # validation report; EXIT NON-ZERO on an error
```
`<id>` is a number (`9` / `ADR-0009`) or, under slug/uuid schemes, the slug / uuid
prefix. `check` is the CI gate — branch on its exit code.

## Author
```sh
adroit new "Short imperative title"               # scaffold from the template
adroit new "Short imperative title" --interview   # AI drafts the body from a Q&A (opt-in)
```
With `--interview`, adroit asks a few questions and an AI drafts the prose,
marked `<!-- adroit:ai-suggested -->`. **The human owns the words** — review and
edit the draft before committing. Identity/status stay mechanical.

## Transition (lifecycle)
```sh
adroit set-status <id> accepted     # moves the file (proposed/ -> accepted/) + relinks
adroit set-status <id> rejected     # or: deprecated, superseded
adroit supersede <new-id> <old-id>  # record that one ADR replaces another
adroit set-review <id> <YYYY-MM-DD> # set a review deadline
```

## Plan
```sh
adroit plan <id>                    # AI implementation checklist for an accepted ADR (read-only)
adroit plan <id> --out plan.md      # write it to a file
```

## Rules
- **The ADR markdown is the durable record.** The CLI is the safe way to mutate
  it; reads can fan out (`-o json`).
- **Never bypass the verbs** to rename/move/renumber files — that strands links
  and breaks identity. Use `set-status` / `supersede` / `renumber` / `relink`.
- **Validate before you call it done:** `adroit check` must exit 0 (errors fail
  it; transient warnings on a deferred-relink branch are OK).
- AI verbs (`--interview`, `plan`) are **opt-in** and need a configured provider;
  without one, `--interview` falls back to the plain template and `plan` errors.
- Don't commit on the user's behalf without being asked.
