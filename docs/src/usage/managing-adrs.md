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
adroit search crossplane
```

Case-insensitive search over titles and bodies.

## Updating status

```sh
adroit status 1 accepted
```

In by-status mode this moves the file to the matching directory and rewrites
the `## Status` section, leaving the rest of the file byte-identical.

## Superseding a decision

```sh
adroit supersede 6 2   # ADR-0006 supersedes ADR-0002
```

Moves the old ADR to `superseded/`, links it forward to the new one, and adds a
reciprocal note to the new ADR.

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
