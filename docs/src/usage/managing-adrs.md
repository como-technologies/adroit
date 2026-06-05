# Managing ADRs

This guide covers the day-to-day workflow of creating and maintaining ADRs.
adroit defaults to the **markdown / by-status** profile: status is encoded by
the directory, and number/title come from the H1 heading. See
[ADR Format](../reference/adr-format.md) for the full format.

## Creating an ADR

```sh
adroit new "Use PostgreSQL for primary datastore"
```

This assigns the next sequential number, scaffolds the file from a template,
writes it into the `proposed/` directory, and opens it in your editor. Use
`--no-edit` to skip the editor and `--template <name|path>` to choose a
template.

## Listing ADRs

```sh
adroit list
adroit list --status accepted
```

Lists ADRs across all status directories, sorted by number. `--status` filters.

## Viewing an ADR

```sh
adroit show 1
```

## Searching

```sh
adroit search postgres
```

Case-insensitive search over titles and bodies.

## Updating status

```sh
adroit set-status 1 accepted   # the setter
adroit status 1                # the getter — prints `accepted` (lowercase, scriptable)
```

`set-status` moves the file to the matching directory (in by-status mode) and
rewrites the `## Status` section, leaving the rest of the file byte-identical.
`status <ID>` is the read-only counterpart — just the status word, lowercase, so
it pipes cleanly; `show` gives the full, capitalized record.

> **Don't want files moving between directories on every status change?** The
> default `by_status` layout encodes status as the directory. If that churn is
> noisy for you, use a layout that keeps each ADR put and records status *in the
> file* instead: `--layout flat` (one directory) or `--layout by_category`
> (directory = area/category, status in `## Status`). Cross-ADR links survive
> moves regardless — `adroit relink` (run automatically on a status change by
> default; see [Concurrent contributors](#concurrent-contributors--branching))
> retargets them — and ADRs are addressable by number, slug, uuid, or
> `category/NNNN`, so references don't go stale. See
> [Naming schemes](../reference/adr-format.md#naming-schemes).

## Concurrent contributors & branching

When several people work the same ADR set on branches, two kinds of
branch-local state collide on merge — and adroit handles both at the merge
point rather than asking you to coordinate up front.

**Stale links (the `relink_scope` knob).** By default (`relink_scope = all`) a
status change heals *every* cross-ADR link repo-wide. With many PRs in flight
that means two unrelated decisions both rewrite the same neighbor files — false
merge conflicts. Set `relink_scope = self` (or `none`) so a status-change PR
touches **only the ADR it is about**:

```sh
adroit config set relink_scope self   # or: ADROIT_RELINK_SCOPE=self / --relink-scope self
```

- `self` — fix only the moved file's own links; leave neighbors' inbound links
  alone (the moved ADR still validates).
- `none` — move only.

The deferred inbound links are transiently stale — `adroit check` reports them
as **warnings** (it still exits 0, so PRs aren't blocked) — until a single
`adroit relink` runs on `main` after each merge and commits the canonicalized
links. That "propose-on-branch, heal-on-main" split is wired by the CI
templates; see [CI Integration](./ci-integration.md#stale-links-across-branches).

**Duplicate numbers.** Two branches each running `adroit new` pick the same
`NNNN`. Keep sequential numbers and catch the collision at the merge queue with
`adroit check`, then resolve with `adroit renumber` — see
[CI Integration](./ci-integration.md#concurrent-adr-numbers-across-branches).

**Prefer to avoid collisions by construction?** Use a collision-free identity:
the `date` or `uuid` [naming scheme](../reference/adr-format.md#naming-schemes)
(the route log4brains and database-migration tools took), or the `by_category`
layout for per-area number sequences.

## Superseding a decision

```sh
adroit supersede 6 2   # ADR-0006 supersedes ADR-0002
```

Moves the old ADR to `superseded/`, links it forward to the new one, and adds a
reciprocal note to the new ADR.

## Relating decisions (typed links)

Beyond supersession, ADRs can carry **typed relational links** to other ADRs —
`relates_to`, `depends_on`, and `refines`:

```sh
adroit link 6 --depends-on 2     # ADR-0006 depends on ADR-0002
adroit link 6 --relates-to 4
adroit link 6 --refines 3
adroit link 6 --depends-on 2 --remove
```

The link is recorded in the source ADR's frontmatter, shows in `adroit show`,
and appears as a distinct, colored edge in the dashboard's
[relationship graph](./web.md). Typed links are a **frontmatter-profile**
feature (they're structured fields); under the markdown profile `adroit link`
asks you to switch with `adroit --format frontmatter migrate`. The targets use
the same identifiers as everything else (number / slug / uuid / `category/NNNN`).

## Setting a review deadline

```sh
adroit set-review 3 2026-07-15   # propose a review by this date
adroit set-review 3 --clear      # remove it
```

Records an optional `Review by:` deadline on a Proposed ADR. Once the date has
passed, the ADR is flagged review-due in `stats` and the web dashboard. The
write is format-preserving (only the `Review by:` line changes).

## Regenerating SUMMARY.md

```sh
adroit index
```

Regenerates the ADR section of `SUMMARY.md` grouped by status, preserving the
rest of the file. Prints to stdout if no `SUMMARY.md` is found.

## Generating a review kickoff

```sh
adroit review 1
adroit review 1 --days 5 --quorum 3 --output review-kickoff.md
```

Generates a review-kickoff doc for an ADR — the structured "here's what you're
reviewing" document the team writes when opening an ADR for a formal accept or
reject decision. It includes the review timeline (computed in business days,
weekends skipped), the quorum, a table of key docs, and a checklist of what the
review MR changes. This is pure generation: no git operations, and the ADR
itself is untouched. Without `--output` it prints to stdout.

## Editing an ADR

```sh
adroit edit 1
```

Opens the ADR in your configured editor.
