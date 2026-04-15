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
# Initialize an ADR directory in your project
adroit init

# Create a new ADR
adroit new "Use PostgreSQL for primary datastore"

# List existing ADRs
adroit list

# Launch the interactive TUI
adroit
```

## Documentation

The [User Manual](https://como-technologies.github.io/adroit/) is built with mdbook and published to GitHub Pages on every push to `main`.

To work on the book locally:

```sh
just book-serve
```

## Development

Requires [Rust](https://rustup.rs/) and [just](https://github.com/casey/just).

```sh
# Install all project tools (clippy, rustfmt, cargo-watch, mdbook)
just init

# See all available recipes
just

# Run the full CI suite (format check, clippy, tests, book build)
just ci

# Run all tests
just test

# Run only unit tests
just unit

# Auto-format code
just fmt

# Build a release binary
just release

# Build the user manual
just book

# Run with arguments
just run init
just run new "My decision"
just run list
```

## Project structure

```
src/
  lib.rs       Library crate root
  adr.rs       ADR model types (Adr, Status)
  store.rs     Filesystem storage for ADRs
  cli.rs       CLI argument parsing (clap)
  tui.rs       Interactive TUI (ratatui)
  main.rs      Binary entry point — delegates to lib
tests/
  cli.rs       Integration tests against the compiled binary
book/
  book.toml    mdbook configuration
  src/         User manual source (Markdown)
```

## License

Apache-2.0
