# Default: list available recipes
default:
    @just --list

# Install project dependencies and tools
init:
    rustup component add clippy rustfmt
    cargo install cargo-watch mdbook cargo-outdated cargo-edit cargo-audit

# Run all CI checks (used by .github/workflows/ci.yml)
ci: fmt-check lint lint-forge test test-forge book crate-outdated crate-audit

# Format code
fmt:
    cargo fmt

# Check formatting without modifying files
fmt-check:
    cargo fmt --check

# Run clippy lints
lint:
    cargo clippy -- -D warnings

# Run clippy with the forge feature (GitHub/GitLab adapters)
lint-forge:
    cargo clippy --features forge -- -D warnings

# Run all tests
test *ARGS:
    cargo test {{ARGS}}

# Run tests with the forge feature enabled
test-forge *ARGS:
    cargo test --features forge {{ARGS}}

# Run only unit tests (skip integration tests)
unit:
    cargo test --lib

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

# Check for outdated dependencies
crate-outdated:
    cargo outdated

# Upgrade dependencies (including incompatible versions)
crate-upgrade:
    cargo upgrade --incompatible

# Update Cargo.lock to latest compatible versions
crate-update:
    cargo update

# Audit dependencies for known vulnerabilities
crate-audit:
    cargo audit

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
