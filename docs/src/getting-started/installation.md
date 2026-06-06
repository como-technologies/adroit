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
just build     # debug build  → target/debug/adroit  (TUI + AI + forge)
just release   # release build → target/release/adroit
```

`just build` includes the AI and forge integrations by default; the bare core
builds with `cargo build --no-default-features` (`just build-core`). The read-only
web dashboard is the one opt-in feature (it needs the Vue bundle) — build and run
it with `just serve`. See [Web Dashboard](../usage/web.md).

## Verify

```sh
./target/debug/adroit --version
```
