# ADR Format

adroit supports two on-disk **format profiles** and two **layouts**. The
default is the *markdown* profile with a *by-status* layout (status encoded by
directory). The original *frontmatter* profile with a *flat* layout is still
fully supported.

## Markdown profile + by-status layout (default)

In this mode an ADR's **status is encoded by the directory** it lives in, and
its **number and title come from the H1 heading**. There is no YAML
frontmatter — the file is plain [MADR](https://adr.github.io/madr/)-style Markdown.

### Directory layout

```
adrs/
  proposed/    0007-remove-audit-mode.md
  accepted/    0006-adopt-adrs.md
  rejected/    0016-ack-vs-crossplane.md
  superseded/  0002-adopt-adrs.md
  deprecated/
  README.md          # not an ADR — skipped
  adr-template.md    # repo-local template — skipped
```

Each status directory may contain a `README.md`; those (and `adr-template.md`)
are skipped when listing ADRs. Changing an ADR's status **moves the file**
between these directories.

### File naming

`NNNN-kebab-case-title.md`, where `NNNN` is the zero-padded, permanent number.
Numbers never reset across directories and may legitimately collide (e.g. a
`0009` in both `proposed/` and `accepted/`) — adroit handles this gracefully.

### Structure

```markdown
# ADR-0006: Adopt ADRs as the Team Decision Process

> State: Accepted

## Status

Accepted

## Context and Problem Statement

...

## Decision Drivers

## Considered Options

## Decision Outcome

### Positive Consequences

### Negative Consequences

## Implementation
```

The `## Status` value line is one of `Proposed`, `Accepted`, `Rejected`,
`Deprecated`, or `Superseded by [ADR-NNNN](relative/path.md)`. The optional
`> State:` blockquote banner mirrors it. The directory is the source of truth
for status; the section is used as a fallback when reading.

### Supersession (both directions)

The `## Status` region carries supersession prose in both directions, and adroit
parses both when reading:

- the **older** ADR says `Superseded by [ADR-NNNN](...)` → `superseded_by`
- the **newer** ADR says `Supersedes [ADR-NNNN](...)` → `supersedes`

Both forms accept a `[ADR-NNNN](path)` link or a bare `ADR-NNNN`. The
supersession graph collapses the two reciprocal notes for one decision into a
single edge.

### Review deadlines

A still-`Proposed` ADR may carry an optional review deadline, written as a line
inside the `## Status` region:

```markdown
## Status

Proposed

Review by: 2026-07-15
```

Set it with `adroit set-review <NUMBER> <YYYY-MM-DD>` (`--clear` to remove). Once
the date is on or before today, the ADR is flagged review-due in `stats` and the
web dashboard. Writing/removing the line is format-preserving.

### Format-preserving writes

When you change only the status, adroit rewrites just the `## Status` value
line and the `> State:` banner, leaving every other byte untouched. Parsing an
unchanged ADR and writing it back is a no-op — round-trips are byte-identical.

## Frontmatter profile + flat layout

The original adroit format stores metadata in YAML frontmatter and keeps every
ADR in one flat directory. Enable it with `format: frontmatter` and
`layout: flat` in the config, or `--format frontmatter --layout flat` on the
command line.

```markdown
---
id: 550e8400-e29b-41d4-a716-446655440000
number: 1
title: Use PostgreSQL for primary datastore
status: Proposed
created: 2026-04-15T10:30:00Z
---

## Context

...
```

| Field | Type | Description |
|---|---|---|
| `id` | UUID v4 | Canonical unique identifier, generated on creation |
| `number` | integer | Sequential display number, assigned on write |
| `title` | string | Short title describing the decision |
| `status` | enum | One of: Proposed, Accepted, Rejected, Deprecated, Superseded |
| `created` | RFC 3339 | UTC timestamp of creation |
| `supersedes` | integer | *(optional)* Number of an older ADR this one supersedes |
| `superseded_by` | integer | *(optional)* Number of the newer ADR that supersedes this one |
| `review_by` | `YYYY-MM-DD` | *(optional)* Review deadline; flags review-due when past for a Proposed ADR |

Optional fields are only written when set, so existing files stay clean.

## Status values

- **Proposed** — the decision is under discussion
- **Accepted** — the decision is in effect
- **Rejected** — considered but not adopted, kept for historical context
- **Deprecated** — no longer recommended but not replaced
- **Superseded** — replaced by a newer ADR (linked in the status line)

## Templates

New ADRs are scaffolded from a template. Built-ins are:

- **`madr`** (the default) — the [MADR](https://adr.github.io/madr/) format
  (Markdown Any Decision Records).
- **`nygard`** — Michael Nygard's original lightweight format
  ([Documenting Architecture Decisions](https://www.cognitect.com/blog/2011/11/15/documenting-architecture-decisions)):
  Title / Status / Context / Decision / Consequences.

You can also point at a custom template file or a `templates_dir`, and if the
target repo contains an `adr-template.md` it is preferred automatically.
Placeholders: `{{number}}`, `{{title}}`, `{{date}}`, `{{status}}`.

## References

The ADR formats adroit follows and templates from:

- [MADR](https://adr.github.io/madr/) — Markdown Any Decision Records; the basis
  for the default `madr` template and the markdown profile's section structure.
- [Documenting Architecture Decisions](https://www.cognitect.com/blog/2011/11/15/documenting-architecture-decisions)
  — Michael Nygard's original ADR post; the basis for the `nygard` template.
- [architecture-decision-record](https://github.com/joelparkerhenderson/architecture-decision-record)
  — a comprehensive collection of ADR templates and examples.
- [ADR = Any Decision Record?](https://ozimmer.ch/practices/2021/04/23/AnyDecisionRecords.html)
  — Olaf Zimmermann on broadening ADRs beyond architecture to any team decision.
