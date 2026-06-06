# Default: list available recipes
default:
    @just --list

# Install project dependencies and tools
init:
    rustup component add clippy rustfmt
    cargo install cargo-watch mdbook cargo-outdated cargo-edit cargo-audit

# Run all CI checks (used by .github/workflows/ci.yml). `ai`+`forge` are default
# features now, so the plain `lint`/`test` cover them; `lint-core`/`test-core`
# guard the `--no-default-features` core (no tui/ai/forge — stays tokio-free), and
# `lint-web`/`test-web` cover the opt-in web feature.
ci: fmt-check lint-core lint lint-web test-core test test-web book crate-outdated crate-audit

# Format code
fmt:
    cargo fmt

# Check formatting without modifying files
fmt-check:
    cargo fmt --check

# Run clippy lints (default features: tui + ai + forge)
lint:
    cargo clippy -- -D warnings

# Clippy the bare core (no tui/ai/forge) — guards the layering: the lib + CLI must
# build with NO ratatui/crossterm, NO rig/tokio, NO ureq.
lint-core:
    cargo clippy --no-default-features -- -D warnings

# Run clippy with the forge feature (GitHub/GitLab adapters)
lint-forge:
    cargo clippy --features forge -- -D warnings

# Run clippy with the web feature (read-only Axum dashboard)
lint-web:
    cargo clippy --features web -- -D warnings

# Run clippy with the ai feature (rig-backed Anthropic/Ollama adapter)
lint-ai:
    cargo clippy --features ai -- -D warnings

# Build the bare core: no tui/ai/forge (the minimal, tokio-free CLI).
build-core:
    cargo build --no-default-features

# Run all tests (default features: tui + ai + forge)
test *ARGS:
    cargo test {{ARGS}}

# Test the bare core (no tui/ai/forge) — exercises the `#[cfg(not(feature=…))]`
# paths (e.g. forge verbs absent) that the default build compiles out.
test-core *ARGS:
    cargo test --no-default-features {{ARGS}}

# Run tests with the forge feature enabled
test-forge *ARGS:
    cargo test --features forge {{ARGS}}

# Run tests with the web feature enabled (JSON API + markdown-render security).
# Builds without a Vue SPA present (the embed dir has a .gitkeep).
test-web *ARGS:
    cargo test --features web {{ARGS}}

# Run tests with the ai feature enabled (rig-backed adapter compiles + the
# always-on interview flow). Live calls still need a key/Ollama at runtime.
test-ai *ARGS:
    cargo test --features ai {{ARGS}}

# Run only unit tests (skip integration tests)
unit:
    cargo test --lib

# Hardening-blitz soak: the model-based oracle + parser property tests at a wider
# case budget. Override depth with PROPTEST_CASES, e.g. `PROPTEST_CASES=5000 just model`.
# (Both also run in `just ci` via `just test` at proptest's default 256 cases.)
model *ARGS:
    PROPTEST_CASES="${PROPTEST_CASES:-2000}" cargo test --test model --test parsers {{ARGS}}

# Type-check without building
check:
    cargo check

# Build in debug mode
build:
    cargo build

# Build in release mode
release:
    cargo build --release

# Run the binary with arguments
run *ARGS:
    cargo run -- {{ARGS}}

# Build the Vue web dashboard SPA into web/dist (embedded by the `web` feature)
web-build:
    cd web && npm install && npm run build

# Build the SPA, then run the read-only web dashboard with live-reload
serve *ARGS: web-build
    cargo run --features web -- serve {{ARGS}}

# Like `serve` but with `forge` too, so the dashboard's read-only forge panel is live (needs forge.* configured + a token)
serve-forge *ARGS: web-build
    cargo run --features web,forge -- serve {{ARGS}}

# Check for outdated dependencies (skipped if cargo-outdated isn't installed;
# `just init` installs it and GitHub CI always runs it)
crate-outdated:
    @if command -v cargo-outdated >/dev/null 2>&1; then cargo outdated; else echo "skip: cargo-outdated not installed (run 'just init')"; fi

# Upgrade dependencies (including incompatible versions)
crate-upgrade:
    cargo upgrade --incompatible

# Update Cargo.lock to latest compatible versions
crate-update:
    cargo update

# Audit dependencies for known vulnerabilities (skipped if cargo-audit isn't
# installed; `just init` installs it and GitHub CI always runs it)
crate-audit:
    @if command -v cargo-audit >/dev/null 2>&1; then cargo audit; else echo "skip: cargo-audit not installed (run 'just init')"; fi

# Upgrade deps, update lockfile, audit, and test
crate-refresh: crate-upgrade crate-update crate-audit test

# Clean build artifacts
clean:
    cargo clean

# Watch for changes and run tests (requires cargo-watch)
watch:
    cargo watch -x test

# Generate and open API docs
doc:
    cargo doc --open --no-deps

# Build the user manual (mdbook)
book:
    mdbook build docs
    @echo "Book built -> docs/book"

# Serve the book locally with live reload
book-serve:
    mdbook serve docs --open

# Clean built book artifacts
book-clean:
    rm -rf docs/book
