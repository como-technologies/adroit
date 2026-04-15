# adroit

A snappy TUI for managing Architecture Decision Records, built with Rust.

## Build & Test

Always use `just` recipes — never raw `cargo` or `mdbook` commands.

```sh
just init        # install all project tools (clippy, rustfmt, cargo-watch, mdbook)
just ci          # full CI suite: fmt-check, lint, test, book build
just check       # type-check without building
just build       # debug build
just test        # all tests (unit + integration)
just unit        # unit tests only (--lib)
just lint        # clippy with -D warnings
just fmt         # auto-format
just fmt-check   # check formatting
just book        # build the mdbook user manual
just book-serve  # local book dev server with live reload
just run <args>  # run the binary
```

## Project Layout

- `src/lib.rs` — library crate root
- `src/main.rs` — thin binary entry point, delegates to lib
- `tests/cli.rs` — integration tests against the compiled binary
- `book/` — mdbook user manual (published to GitHub Pages)
- `justfile` — all dev workflow recipes

## Conventions

- Lib/bin separation: all logic in the library crate, `main.rs` is glue only
- Use `thiserror` for library error types, `anyhow` for the binary
- Use `strum` for enum Display derives
- Use newtypes for domain identifiers (e.g. `AdrId`, `Number`, `Created`)
