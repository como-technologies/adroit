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
| **Author a decision** | a question is open and you're drafting the record | `new` · `draft` · `compose` · `plan` · `edit` · `lint` · `dedupe` · `related` · `link` |
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

### Seed a backlog from an assessment — `adroit import`

When the decisions come from a structured **assessment** rather than a blank page,
`import` turns that export into a proposed-ADR backlog in one shot — the *ingest*
seam of the portfolio loop (Assess → Prescribe):

```sh
adroit import --from-assessment maturity.json   # one proposed ADR per practice
adroit import --from-assessment maturity.yaml --dry-run   # preview, write nothing
```

It reads an [`assessments`](https://github.com/como-technologies) export (a
`Domain → Practice → Question` maturity model, as `.json` or `.yaml`) and seeds one
**proposed** ADR per practice: the practice's *context* becomes the problem
statement, its *value* / *risk* / *effort* become decision drivers, and its
questions are recorded as assessment signals. The body is marked
`<!-- adroit:seeded-from-assessment -->` and carries a provenance note back to the
source practice. The mapping is **mechanical** — no AI, no network — so identity,
status, and the heading stay fixed; the seeded prose is a starting point you refine
(`adroit draft <id>` to flesh one out, then `edit` / `lint` as above).

`import` is **re-runnable**: practices whose title already has an ADR are skipped
(report `(N skipped — already present)`), so importing an *updated* assessment only
adds what's new. Pass `--force` to seed anyway. Under the `by_category` layout each
domain becomes the category.

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

## Worked workflows

Concrete, copy-pasteable sequences for the common ways teams run adroit. Pick the
one that matches your setup; they share the same core verbs.

### Local-only (no forge, no AI)

The plain solo path — everything on disk, no network, no provider:

```sh
adroit new "Use PostgreSQL for the datastore"   # scaffold proposed/0001-…, opens $EDITOR
adroit edit 1                                    # fill in the template's prompts
adroit lint 1                                    # finished? (mechanical, no AI)
adroit set-status 1 accepted                     # decide → moves to accepted/, heals links
adroit index                                     # refresh SUMMARY.md
adroit check                                     # validate the repo (the CI gate)
```

### AI-assisted authoring

Let the model draft the prose from a short interview; you review and decide.
Needs a provider configured ([Automation & AI](./automation.md)):

```sh
adroit new "Adopt event sourcing" --interview    # Socratic Q&A → AI drafts the body
# (or author plainly, then fill it in later: adroit new "…"  →  adroit draft 1)
adroit edit 1                                    # review / trim the AI draft in $EDITOR
adroit compose 1 "expand the negative consequences"   # targeted AI revision (vs draft's full redraft)
adroit lint 1                                    # mechanical gate (add --ai for advisory review)
adroit summarize 1                               # one-paragraph TL;DR for the PR description
adroit set-status 1 accepted
adroit plan 1                                    # (optional) AI implementation checklist
```

The AI only ever writes *prose* (marked `<!-- adroit:ai-suggested -->`); identity,
status, dates, and links stay mechanical, and you review before committing.

### Forge — the PR is the decision

The ADR gets a tracker issue and a review PR; merging the PR is what accepts it.
See [Forge Integration](./forge.md):

```sh
adroit init                                      # one-time: detect + write forge.* config
adroit auth github                               # …or export ADROIT_GITHUB_TOKEN
adroit new "Use PostgreSQL" --forge              # ADR + linked issue + draft PR off adr/0001-…
adroit review 1 --forge                          # post the review-kickoff as a PR/issue comment
# … the team reviews and approves on the forge …
adroit set-status 1 accepted --forge --yes       # verify approvals + CI, merge the PR, close the
                                                 #   issue, and land accepted/ on main
```

Every `--forge` action is opt-in and **previews unless you pass `--yes`**. If the
forge is unreachable, adroit warns and keeps the local ADR — it never loses your work.

### Combined — AI draft on a forge PR

The two layers compose: draft with AI, then run it through the PR-is-the-decision
flow.

```sh
adroit new "Adopt event sourcing" --interview --forge   # AI-drafted body + issue + draft PR
adroit edit 1                                           # review the draft
adroit review 1 --forge
adroit set-status 1 accepted --forge --yes
```

## Best practices

- **Lint before you circulate.** `adroit lint` is mechanical (no provider needed)
  and exits non-zero on unfinished sections — run it as an authoring gate locally
  and in CI.
- **Let `set-status` move files; don't move them by hand.** It relocates the ADR
  and heals inbound links; a manual move strands them (and `check` will flag it).
- **Review AI output before committing.** AI writes only prose, clearly marked;
  identity / status / dates stay mechanical, but the words are yours to own.
- **Keep the repo green in CI.** `adroit check` + `adroit index --check` are the
  gate — wire them in early ([CI Integration](./ci-integration.md)).
- **On a busy branch, set `relink_scope = self`** so a status-change PR touches
  only its own ADR (no false merge conflicts); a post-merge `adroit relink` on
  `main` canonicalizes the rest. See
  [Managing ADRs](./managing-adrs.md#concurrent-contributors--branching).
- **Avoid number collisions by construction** when many people author in parallel:
  use the `date` / `uuid` scheme or the `by_category` layout
  ([Naming schemes](../reference/adr-format.md#naming-schemes)).

`adroit --help` lists every command in this same lifecycle order; `adroit <verb>
--help` explains one. For the full per-verb reference, see the
[CLI Reference](../reference/cli.md).
