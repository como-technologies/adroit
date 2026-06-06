---
name: use-adroit
description: Use when a project manages its Architecture Decision Records with adroit and you need to read, author, validate, or transition them from the CLI. Invoke for "what ADRs do we have / what did we decide about X", "create / draft an ADR", "lint / is this ADR finished", "any duplicate of this decision", "accept / reject / supersede ADR N", "is the ADR repo valid", "plan the implementation of ADR N". Drive the `adroit` CLI (machine-readable `-o json`) rather than hand-editing files.
user-invocable: true
---

# Use adroit to manage a project's ADRs

`adroit` is the source of truth for this project's ADRs. **Drive the CLI** — it
owns identity, dates, status, file layout, and cross-links mechanically. Don't
hand-create or hand-move ADR files; let the verbs do it (re-running a write verb
on unchanged state is a no-op).

Point it at the repo's ADR dir with `--dir` / `ADROIT_DIR` if it isn't the
default. Prefer `-o json` whenever you're parsing output.

The commands below follow a decision's **lifecycle** — the same order
`adroit --help` groups them in. Author → Review & decide; Explore and Maintain
apply at any stage.

## Explore the corpus — anytime (`-o json`)
```sh
adroit list -o json                 # every ADR: reference, status, title, links, review_due
adroit show <id> -o json            # one ADR: summary fields (flattened) + body + history
adroit search "<term>" -o json      # title/body matches
adroit stats -o json                # counts, oldest-proposed, growth
adroit graph -o json                # supersession + typed-link edges
adroit ask "<question>"             # AI answer about the corpus + citations (needs a provider)
adroit check -o json                # validation report; EXIT NON-ZERO on an error
```
`<id>` is a number (`9` / `ADR-0009`) or, under slug/uuid schemes, the slug / uuid
prefix. `check` is the CI gate — branch on its exit code.

## Author a decision
```sh
adroit new "Short imperative title"               # scaffold (warns on an exact-title dup)
adroit new "Short imperative title" --interview   # ...or have AI draft the body from a Q&A
adroit dedupe <id>                  # did we already decide this? overlapping ADRs (no AI)
adroit draft <id>                   # run that same AI interview on an existing ADR
adroit related <id> -o json         # find ADRs worth cross-linking (no AI)
adroit link <id> --refines <target> # or --relates-to / --depends-on
adroit lint <id>                    # authoring gate (see below) — exits non-zero if unfinished
adroit plan <id>                    # (optional) AI implementation checklist; --out FILE to save
```
The scaffold ships each section with an *instructive italic `_…_` prompt* saying
what belongs there. Fill them in by hand, or let `--interview` / `draft` draft
the prose (marked `<!-- adroit:ai-suggested -->`; **the human owns the words** —
review before committing; identity/status stay mechanical). Then **`lint`**: it
flags any section still left as nothing but its prompt, plus missing negative
consequences or a single option, and exits non-zero — so run it before you call a
draft done.

## Review & decide
```sh
adroit set-review <id> <YYYY-MM-DD> # set a review deadline
adroit review <id>                  # generate a review-kickoff doc
adroit summarize <id>               # one-paragraph TL;DR for a PR / notification
adroit set-status <id> accepted     # record the decision: moves proposed/ -> accepted/ + relinks
adroit set-status <id> rejected     # or: deprecated, superseded
adroit supersede <new-id> <old-id>  # record that one ADR replaces another
```

## Maintain the repo
```sh
adroit check                        # the gate — non-zero on a structural problem
adroit relink                       # heal cross-ADR links after moves
adroit renumber <old> <new>         # resolve a number collision
adroit index                        # regenerate the ADR section of SUMMARY.md
```

## Rules
- **The ADR markdown is the durable record.** The CLI is the safe way to mutate
  it; reads can fan out (`-o json`).
- **Never bypass the verbs** to rename/move/renumber files — that strands links
  and breaks identity. Use `set-status` / `supersede` / `renumber` / `relink`.
- **A draft isn't done until `lint <id>` passes** (no section left as its prompt;
  trade-offs and alternatives recorded). It needs no provider and exits non-zero,
  so it's safe as a gate; `lint --ai` adds an advisory review.
- **Validate the repo before you call it done:** `adroit check` must exit 0
  (errors fail it; transient warnings on a deferred-relink branch are OK).
- AI verbs (`--interview`, `draft`, `plan`, `ask`, `summarize`, `lint --ai`) are
  **opt-in** and need a configured provider; without one, `--interview` falls
  back to the plain template and the others error. The fully-mechanical verbs
  (`dedupe`, `related`, `lint`) work with no provider.
- Don't commit on the user's behalf without being asked.
