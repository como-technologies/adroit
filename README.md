# adroit

A snappy TUI for managing Architecture Decision Records.

The name hides **ADR** in plain sight — because good architecture decisions should be _adroit_: clever, skillful, and well-considered.

## What are ADRs?

Architecture Decision Records are short documents that capture important architectural decisions along with their context and consequences. They provide a decision log that helps current and future team members understand _why_ the system looks the way it does.

## What does adroit do?

adroit gives you a terminal-native interface for the full ADR lifecycle:

- **Create** new ADRs from templates with guided prompts
- **Browse** and search your decision log in a rich, interactive TUI
- **Update** status as decisions are superseded, deprecated, or accepted
- **Link** related decisions together to build a navigable decision graph
- **Export** to Markdown for integration with your existing docs pipeline

## Installation

```sh
cargo install adroit
```

## Usage

```sh
# Create a new ADR (directory is auto-created)
adroit new "Use PostgreSQL for primary datastore"

# List existing ADRs
adroit list

# Launch the interactive TUI
adroit
```

## Getting started (new developers)

Prerequisites: [Rust](https://rustup.rs/) and [just](https://github.com/casey/just).

```sh
git clone https://github.com/como-technologies/adroit.git
cd adroit
just init    # installs all tooling: clippy, rustfmt, cargo-watch, mdbook, cargo-outdated, cargo-edit, cargo-audit
just ci      # run the full CI suite to verify everything works
```

That's it. Run `just` to see all available recipes.

## Development workflow

### Day-to-day

| Recipe | What it does |
|---|---|
| `just ci` | Full CI suite: format check, clippy, tests, book build |
| `just test` | Run all tests (unit + integration) |
| `just unit` | Run unit tests only |
| `just check` | Quick type-check without building |
| `just fmt` | Auto-format code |
| `just lint` | Clippy with `-D warnings` |
| `just watch` | Auto-run tests on file changes |
| `just run <args>` | Run the binary (e.g. `just run new "My decision"`, `just run list`) |

### Building

| Recipe | What it does |
|---|---|
| `just build` | Debug build |
| `just release` | Release build |

### Dependency management

| Recipe | What it does |
|---|---|
| `just crate-refresh` | Upgrade, update lockfile, audit, and test -- all in one |
| `just crate-outdated` | Check for outdated dependencies |
| `just crate-upgrade` | Upgrade deps (including incompatible versions) |
| `just crate-update` | Update Cargo.lock to latest compatible versions |
| `just crate-audit` | Audit for known vulnerabilities |

### Documentation

| Recipe | What it does |
|---|---|
| `just book` | Build the user manual |
| `just book-serve` | Local dev server with live reload |
| `just doc` | Generate and open API docs |

The [User Manual](https://como-technologies.github.io/adroit/) is published to GitHub Pages on every push to `main`.

## Project structure

```
src/
  lib.rs           Library crate root
  adr.rs           ADR model types (AdrId, Number, Created, Status, Adr)
  frontmatter.rs   YAML frontmatter serialization
  store.rs         Filesystem storage for ADRs
  cli.rs           CLI argument parsing (clap)
  tui.rs           Interactive TUI (ratatui)
  main.rs          Binary entry point -- delegates to lib
tests/
  cli.rs           Integration tests against the compiled binary
book/
  book.toml        mdbook configuration
  src/             User manual source (Markdown)
```

## License

Apache-2.0
