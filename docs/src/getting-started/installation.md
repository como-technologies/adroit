# Installation

adroit is distributed as **source**: build it with the standard Rust
toolchain. This is a decision, not a gap — crates.io publication and prebuilt
binaries are retired for the current maturity rung by ADR-0013 (publishing is
an owner-only action, and every current consumer builds from a checkout). The
[Changelog](../reference/changelog.md) records tagged releases; a consumer
pins a release by installing from the repo at the tag's commit:

```sh
cargo install --git <path-or-url-to-adroit> --rev <tag-sha> adroit
```

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
