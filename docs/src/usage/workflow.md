# The ADR Workflow

adroit's commands map to the **life of a decision** — from first draft to a
maintained record. `adroit --help` groups them in this same order, so once you
know the stages you know which verb to reach for next. You rarely need them all;
the [everyday path](#the-everyday-path) is a handful of verbs.

## The lifecycle at a glance

```text
Author a decision ─▶ Review & decide ─▶ (Implement)
                          │
        Explore the corpus & Maintain the repo — anytime
```

| Stage | You're here when… | Verbs |
|---|---|---|
| **Author a decision** | a question is open and you're drafting the record | `new` · `draft` · `plan` · `edit` · `lint` · `dedupe` · `related` · `link` |
| **Review & decide** | the draft is circulating and the team is converging | `set-review` · `review` · `summarize` · `set-status` · `supersede` |
| **Explore the corpus** | *(anytime)* reading, searching, or asking | `list` · `show` · `status` · `search` · `stats` · `graph` · `ask` · `serve` |
| **Maintain the repo** | keeping the set valid, linked, and published | `check` · `relink` · `renumber` · `migrate` · `index` · `publish` |

## 1. Author a decision

Start the record, then shape it until it's worth reviewing.

```sh
adroit new "Use PostgreSQL for the primary datastore"   # scaffold from the template
adroit dedupe 1            # is the team already deciding this? (mechanical, no AI)
# fill it in — by hand over the template's prompts, or let AI interview you:
adroit draft 1            # Socratic Q&A → AI drafts the body (same as `new --interview`)
adroit edit 1             # tweak in $EDITOR
adroit related 1          # find ADRs worth cross-linking
adroit link 1 --refines 0007
adroit lint 1             # authoring gate — flags any section still left as its prompt
adroit plan 1             # (optional) AI implementation checklist to scope the work
```

The template ships every section with an *instructive italic prompt* telling you
what belongs there. `lint` flags any section you leave as nothing but that
prompt, so it doubles as a "what's left to write" checklist. The AI verbs
(`draft` / `new --interview`) only ever write *prose* — identity, status, and
dates stay mechanical. See [Automation & AI](./automation.md) for the AI layer.

## 2. Review & decide

Open the draft for review, then record the outcome.

```sh
adroit set-review 1 2026-07-01   # set a review deadline
adroit review 1                  # generate a review-kickoff doc
adroit summarize 1               # one-paragraph TL;DR for the PR / a notification
adroit set-status 1 accepted     # record the decision (moves the file in by_status)
adroit supersede 9 1             # ADR-9 replaces ADR-1, wiring up both ends
```

`set-status` is the moment a decision becomes real; in the default `by_status`
layout it moves the file and heals inbound links. See
[Managing ADRs](./managing-adrs.md) for the status and supersession details.

## 3. Explore the corpus — anytime

The reading verbs work at every stage and never modify the repo. All of them
take `-o json` for scripts and agents (see [Automation & AI](./automation.md)).

```sh
adroit list --status accepted
adroit show 1
adroit search postgres
adroit stats          # status counts, ages, growth
adroit graph          # the supersession + link graph
adroit ask "what did we decide about the datastore?"   # AI answer + citations
adroit serve          # read-only web dashboard (web feature)
```

## 4. Maintain the repo

Keep the set valid and published — usually from CI and at release time.

```sh
adroit check          # CI gate — non-zero on a structural problem
adroit relink         # heal cross-ADR links after moves
adroit renumber 12 13 # resolve a number collision
adroit migrate        # convert to the configured layout / format
adroit index          # regenerate the ADR section of SUMMARY.md
adroit publish        # export the accepted set to a directory
```

See [CI Integration](./ci-integration.md) for wiring `check` into a pipeline.

## The everyday path

Most decisions only ever touch a few verbs:

```sh
adroit new "…"                 # author
adroit draft 1                 # (or fill it in by hand over the prompts)
adroit lint 1                  # make sure it's actually finished
adroit set-status 1 accepted   # decide
adroit check                   # keep the repo green
```

`adroit --help` lists every command in this same lifecycle order; `adroit <verb>
--help` explains one. For the full per-verb reference, see the
[CLI Reference](../reference/cli.md).
