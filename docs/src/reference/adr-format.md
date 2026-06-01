# ADR Format

adroit supports two on-disk **format profiles** and two **layouts**. The
default is the *markdown* profile with a *by-status* layout (status encoded by
directory). The original *frontmatter* profile with a *flat* layout is still
fully supported.

adroit infers a repo's actual profile from the files on disk and **refuses to
run** when that disagrees with your configured `layout`/`format` (so it never
silently hides ADRs or corrupts numbering). To change your preference on an
existing repo, run [`adroit migrate`](./cli.md#adroit-migrate-yes) — it moves
files and/or re-serializes between profiles, then fixes cross-ADR links.

## Markdown profile + by-status layout (default)

In this mode an ADR's **status is encoded by the directory** it lives in, and
its **number and title come from the H1 heading**. There is no YAML
frontmatter — the file is plain [MADR](https://adr.github.io/madr/)-style Markdown.

### Directory layout

```
adrs/
  proposed/    0007-adopt-graphql-for-the-public-api.md
  accepted/    0006-use-postgresql-for-the-datastore.md
  rejected/    0005-adopt-microservices.md
  superseded/  0002-use-rest-for-the-public-api.md
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
`0012` in both `proposed/` and `accepted/`) — adroit handles this gracefully.
This is the default `sequential` scheme; [Naming schemes](#naming-schemes) covers
the collision-free `date` / `uuid` alternatives.

### Structure

```markdown
# ADR-0006: Use PostgreSQL for the primary datastore

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

### Cross-ADR links stay canonical

Because a status change moves a file between directories, relative links between
ADRs (`[..](../proposed/0009-x.md)`) would otherwise go stale. adroit keeps them
correct: every `status` / `supersede` automatically rewrites all relative links
pointing at the moved ADR (and the moved file's own outbound links) to its new
location. `adroit relink` does the same on demand across the whole repo (to
repair links edited outside adroit), and `adroit check` flags any broken or
stale relative link. External URLs, anchors, and non-ADR links are never
touched.

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

Even with no explicit deadline, a still-`Proposed` ADR is flagged review-due once
it has been sitting (since its creation date) longer than `review_overdue_days`
(config; default 30, `0` disables) — so an aging backlog surfaces on the
dashboard on its own, without stamping each ADR with a deadline.

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

## Naming schemes

How an ADR's **identifier and filename** are formed is configurable
(`naming` config / `ADROIT_NAMING` / `--naming`). The identity model is
abstracted behind a single seam, so each scheme is self-contained. Pick **one
for the repo's lifetime** — adroit does not rename existing ADRs when you change
the setting.

| Scheme | Filename | Heading | Identity | Collisions |
|---|---|---|---|---|
| `sequential` (default) | `NNNN-title.md` | `# ADR-NNNN: Title` | global number | possible across branches |
| `date` | `YYYYMMDD-title.md` | `# Title` | date slug | collision-free |
| `uuid` | `<uuid>-title.md` | `# Title` | a UUID | collision-free |
| `per_category` | `<category>/NNNN-title.md` | `# ADR-NNNN: Title` | `category/NNNN` | collision-free across categories |

**`sequential`** — the classic zero-padded `NNNN`, human-friendly and sortable.
Its one weakness is **cross-branch collisions**: two branches that each create
`0009` conflict on merge. CI on the merged state plus serialized merges catches
this (see [CI integration](../usage/ci-integration.md)), and
[`adroit renumber`](./cli.md#adroit-renumber-old-new---file-path) resolves a
collision after the fact.

**`date`** (log4brains-style) — the filename carries `YYYYMMDD-title`, so two
people creating ADRs the same day on different branches never collide (a same-day
same-title clash is auto-suffixed `-2`, `-3`). There is no ADR number, so the H1
is a plain `# Title` and the identity lives in the filename.

**`uuid`** — a persisted UUID guarantees uniqueness with zero coordination, at
the cost of human-friendliness. adroit displays a short `ADR-<prefix>` and lets
you address an ADR by any unique leading prefix of the UUID.

**`per_category`** (MADR categories) — per-directory local numbering, paired
with the **`by_category` layout**: each immediate subdirectory is a category
(an *area*, not a status), and numbering restarts per category (so `data/0001`
and `infra/0001` coexist). The identity is the composite `category/NNNN`. Status
is **not** encoded by the directory here (the directory is the category) — it
lives in the `## Status` section / banner, so a status change rewrites the file
**in place** rather than moving it. Create with an explicit category:

```sh
adroit --layout by_category --naming per_category \
  new "Use Postgres" --category data
# -> data/0001-use-postgres.md, heading "# ADR-0001: Use Postgres"
```

### Addressing and scheme-specific commands

Read/lifecycle commands — `show`, `status`, `edit`, `set-review`, and
`supersede` — take an `<ID>` resolved through the active scheme: a number for
`sequential` (e.g. `9` or `ADR-0009`), the filename slug for `date`, a unique
UUID prefix for `uuid`, or the `category/NNNN` composite for `per_category`
(`data/0001`, or the unpadded `data/1`). `renumber` and `review` are
**numeric-only** (their artifacts are a single global number — `sequential`);
they error under `date` / `uuid` / `per_category`. (`per_category` migration
to/from other layouts is not automated — reorganize categories by hand.)

## Dates come from git

adroit derives an ADR's **creation date, last-modified date, and lifecycle
timeline from git history** — not from the file. This is deliberate:

- The markdown profile persists no creation date (status is a directory, number
  and title come from the H1), so there's nothing in the file to read.
- A fresh `git clone` resets every file's modification time to the checkout
  time, so the filesystem can't tell you when an ADR was written either.

Git can answer both. The first commit that added the file is its creation, and
in the by-status layout every status change is a directory move (a rename git
records) — so `proposed/0007-…md` → `accepted/0007-…md` *is* the accepted date.
`adroit show`, the TUI preview, and the web dashboard's Browse / detail /
Insights views all read these dates, and the detail views render the full
timeline (proposed → accepted / rejected / superseded).

Resolution precedence for the creation date, highest first:

1. **git** — the first commit that added the file (when the ADR dir is inside a
   git work tree and the file is tracked);
2. the authored `created:` field, in the **frontmatter** profile only;
3. the file's filesystem modification time;
4. the value parsed from the file (a fresh "now" for a brand-new, never-committed
   markdown ADR).

Outside a git repository — or for an ADR you've created but not yet committed —
adroit falls back to the modification time and omits the lifecycle timeline.

You can control the source with `date_source` (config / `ADROIT_DATE_SOURCE` /
`--date-source`): `auto` (the adaptive default above), `git` (require git — warns
if history is unavailable or the clone is **shallow**, a common CI footgun that
makes creation dates wrong), or `filesystem` (never shell git: mtime/authored
dates only, no timeline — useful in sandboxes, no-git environments, or when git
history is misleading after a big rewrite).

## Templates

New ADRs are scaffolded from a template. Built-ins are:

- **`madr`** (the default) — the [MADR](https://adr.github.io/madr/) format
  (Markdown Any Decision Records).
- **`nygard`** — Michael Nygard's original lightweight format
  ([Documenting Architecture Decisions](https://www.cognitect.com/blog/2011/11/15/documenting-architecture-decisions)):
  Title / Status / Context / Decision / Consequences.

You can also point at a custom template file or a `templates_dir`, and if the
target repo contains an `adr-template.md` it is preferred automatically.
Placeholders: `{{heading}}` (the scheme's H1 — `# ADR-NNNN: Title` for numeric
schemes, `# Title` for slug schemes), `{{number}}` (the bare identifier),
`{{title}}`, `{{date}}`, `{{status}}`.

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
