# Installation

adroit is in Phase 1 (dogfooding) and isn't published yet — build it from
source.

## Prerequisites

[Rust](https://rustup.rs/) and [just](https://github.com/casey/just).

## Build from source

```sh
git clone https://github.com/como-technologies/adroit.git
cd adroit
just init      # one-time: install the toolchain (clippy, rustfmt, mdbook, …)
just build     # debug build  → target/debug/adroit  (CLI + TUI)
just release   # release build → target/release/adroit
```

The read-only web dashboard is behind the `web` feature (off by default since it
needs the Vue bundle). Build and run it with `just serve`. See
[Web Dashboard](../usage/web.md).

## Verify

```sh
./target/debug/adroit --version
```
