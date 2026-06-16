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
# `lint-web`/`test-web` cover the opt-in web feature. `adr-check` is the
# self-hosted dogfood gate: the freshly built adroit validates its own corpus.
ci: fmt-check lint-core lint lint-web test-core test test-web adr-check book crate-outdated crate-audit

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

# Run clippy with the web feature (read-only Axum dashboard)
lint-web:
    cargo clippy --features web -- -D warnings

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

# Run tests with the web feature enabled (JSON API + markdown-render security).
# Builds without a Vue SPA present (the embed dir has a .gitkeep).
test-web *ARGS:
    cargo test --features web {{ARGS}}

# Run only unit tests (skip integration tests)
unit:
    cargo test --lib

# Hardening-blitz soak: the model-based oracle + parser property tests at a wider
# case budget. Override depth with PROPTEST_CASES, e.g. `PROPTEST_CASES=5000 just model`.
# (Both also run in `just ci` via `just test` at proptest's default 256 cases.)
model *ARGS:
    PROPTEST_CASES="${PROPTEST_CASES:-2000}" cargo test --test model --test parsers {{ARGS}}

# Self-hosted corpus gate (dogfood): build adroit, then have it validate its own
# in-repo ADR corpus (ADR-0001). Always `--dir adr` — ADROIT_DIR points elsewhere.
adr-check: build
    ./target/debug/adroit check --dir docs/adr

# M5 dogfood rehearsal: the exact read slice the Adopt engine (conduit) issues,
# end to end against a LIVE local ollama — import --ai → lint → accept →
# plan --save → manifest/list/show/plan all -o json, with the stored-plan read
# proven byte-deterministic. Runs in a throwaway temp corpus (never adr/ or
# ADROIT_DIR); skips cleanly when no ollama is listening. NOT part of `just ci`
# (live model calls). See docs/src/dev/adopt-read-slice.md.
adopt-slice: build
    #!/usr/bin/env bash
    set -euo pipefail
    host="${ADROIT_AI_HOST:-http://localhost:11434}"
    model="${ADROIT_AI_MODEL:-llama3.2}"
    if ! curl -fsS --max-time 3 "$host/api/tags" >/dev/null 2>&1; then
        echo "skip: no ollama listening at $host (start one, or set ADROIT_AI_HOST)"
        exit 0
    fi
    bin="$PWD/target/debug/adroit"
    fix="$PWD/tests/fixtures/golden-assessment.yaml"
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    export ADROIT_DIR="$tmp" ADROIT_AI_ENABLED=true ADROIT_AI_PROVIDER=ollama \
        ADROIT_AI_MODEL="$model" ADROIT_AI_HOST="$host"
    echo "== write side (corpus: $tmp; model: $model)"
    t0=$SECONDS
    "$bin" import --from-assessment "$fix" --ai -o json > "$tmp/import.json"
    echo "import --ai: $((SECONDS - t0))s"
    python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); \
        assert d["seeded"] and all(s["status"] == "Proposed" for s in d["seeded"]), d; \
        print("seeded:", ", ".join(s["reference"] for s in d["seeded"]))' "$tmp/import.json"
    # lint findings are model-quality feedback, not a slice failure.
    "$bin" lint 1 -o json || true
    "$bin" set-status 1 accepted
    t0=$SECONDS
    "$bin" plan 1 --save > /dev/null
    echo "plan --save: $((SECONDS - t0))s"
    echo "== read side (conduit-shaped; AI disabled to prove the reads are provider-free)"
    export ADROIT_AI_ENABLED=false
    "$bin" manifest -o json | python3 -c 'import json,sys; d=json.load(sys.stdin); \
        assert d["tool"] == "adroit" and d["manifest_schema"] == 1, (d["tool"], d["manifest_schema"]); \
        print("manifest: tool", d["tool"], "· schema", d["manifest_schema"], "·", len(d["commands"]), "commands")'
    "$bin" list --status accepted -o json | python3 -c 'import json,sys; rows=json.load(sys.stdin); \
        assert len(rows) == 1 and rows[0]["status"] == "Accepted", rows; \
        print("list --status accepted:", rows[0]["reference"])'
    "$bin" show 1 -o json | python3 -c 'import json,sys; d=json.load(sys.stdin); \
        assert d["plan"], "show -o json carries no stored plan"; \
        print("show: stored plan,", len(d["plan"]), "chars")'
    "$bin" plan 1 -o json > "$tmp/plan-a.json" 2> /dev/null
    "$bin" plan 1 -o json > "$tmp/plan-b.json" 2> /dev/null
    cmp "$tmp/plan-a.json" "$tmp/plan-b.json"
    python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); \
        assert d["stored"] is True, d' "$tmp/plan-a.json"
    echo "plan -o json: byte-identical twice (sha256 $(sha256sum "$tmp/plan-a.json" | cut -c1-12)…), stored: true"
    echo "adopt-slice: PASS"

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
    # vite empties web/dist on build, removing the tracked placeholder that keeps
    # the rust-embed dir present on a clean checkout — recreate it.
    @touch web/dist/.gitkeep

# Build the SPA, then run the read-only web dashboard with live-reload. `forge` is
# a default feature, so the dashboard's read-only forge panel is live too (needs
# forge.* configured + a token).
serve *ARGS: web-build
    cargo run --features web -- serve {{ARGS}}

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
