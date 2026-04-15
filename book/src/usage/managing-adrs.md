# Managing ADRs

## Creating a new ADR

```sh
adroit new "Title of the decision"
```

Each ADR is automatically assigned the next sequential number and written as a Markdown file.

## Listing ADRs

```sh
adroit list
```

Lists all ADR files in the configured directory, sorted by number.

## ADR lifecycle

Every ADR moves through a lifecycle:

| Status | Meaning |
|---|---|
| **Proposed** | Under discussion, not yet decided |
| **Accepted** | Decision has been made and is in effect |
| **Deprecated** | No longer relevant but kept for history |
| **Superseded** | Replaced by a newer decision |

New ADRs start with status **Proposed**.

## Custom ADR directory

By default, adroit looks for ADRs in `docs/adr/`. Override this with the `--dir` flag:

```sh
adroit --dir architecture/decisions list
```
