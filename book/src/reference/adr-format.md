# ADR Format

adroit stores each decision as a Markdown file with YAML frontmatter and a structured naming convention.

## Filename

Files follow the pattern:

```
NNNN-slug-of-title.md
```

- `NNNN` — zero-padded sequential number (e.g. `0001`, `0042`). This is cosmetic — the canonical identity is the UUID in the frontmatter.
- `slug-of-title` — lowercase, hyphen-separated title

## File structure

```markdown
---
id: 550e8400-e29b-41d4-a716-446655440000
number: 1
title: Use PostgreSQL for primary datastore
status: Proposed
created: 2026-04-14T10:30:00Z
---

(body in Markdown)
```

## Frontmatter fields

| Field | Type | Description |
|---|---|---|
| `id` | UUID v4 | Canonical unique identifier. Immutable once created. |
| `number` | integer | Cosmetic sequential number, assigned on first save. |
| `title` | string | Short description of the decision. |
| `status` | string | Current lifecycle status (see below). |
| `created` | RFC 3339 timestamp | When the ADR was first created (UTC). |

## Statuses

- **Proposed** — under discussion
- **Accepted** — decided and in effect
- **Deprecated** — no longer relevant
- **Superseded** — replaced by a newer ADR

New ADRs start with status **Proposed**.

## Versioning

adroit assumes your ADR directory is tracked by git. The git history provides full versioning — who changed what, when, and why. Git SHAs are not stored in the file; they are derived at runtime when needed.
