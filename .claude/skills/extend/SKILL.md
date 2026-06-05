---
name: extend
description: Use to add a new variant of an adroit extension seam — a forge provider, naming scheme, format profile, layout, tracker, publish adapter, template, config key, or CLI subcommand — following the existing pattern with the tests and docs each one requires. Invoke for "add a gitea/bitbucket provider", "add a naming scheme", "add a Confluence/Notion publish adapter", "add a config key", etc.
user-invocable: true
---

# Extend adroit — add a seam variant

adroit is built around isolated seams: adding a variant edits one module + wires
one match arm, never the ~12 consumers. Find the seam below, follow its checklist,
and ALWAYS add the tests + docs it lists. Compose `test-driven-development`; finish
with `gate` + `doc-sync`, then ask before pushing.

## Forge provider (e.g. gitea, bitbucket, codeberg)
"Adding a provider = one match arm + one module."
- `src/forge/<name>.rs` — implement `Forge` + `Tracker` over the `HttpTransport`
  seam (token env var, slug/host), same shape as `github.rs`. Expose
  `with_transport(...)` (NOT `#[cfg(test)]`-gated) for fault-injection.
- `src/forge/mod.rs` — add the `Provider` arm + the `open()` match arm.
- `src/config.rs` — the `Provider` value; teach `parse_remote_url` its remote.
- **Tests:** add it to the adapter list in `tests/forge_faults.rs` (hostile-response
  fuzz — auto-covered); unit tests with a `FakeTransport` in the module.
- **Docs:** the forge section in CLAUDE.md; usage if user-facing.

## Tracker (split tracker, e.g. Linear alongside a GitHub/GitLab forge)
- `src/forge/<name>.rs` implementing `Tracker`; `TrackerProvider` arm; `open()`
  chooses forge and tracker independently. Tests + docs as above.

## Naming scheme (a new identity form)
"Adding a scheme edits only `src/naming.rs`."
- `src/naming.rs` — add the `NamingScheme` arm and implement EVERY method (`assign`,
  `parse`, `parse_ref`, `filename`, `display`, `heading`, `link_label`,
  `ref_in_link`, `ref_in_link_from`, `ref_matches`, `scope`). Watch: slug-vs-numeric
  identity, char-boundary-safe `display`, and same-dir link resolution via the
  source category (`ref_in_link_from`).
- **Tests:** a weighted cell in `arb_profile()` (`tests/model.rs`) — identity is read
  back from disk, so little prediction is needed; naming unit tests.
- **Docs:** the naming table in `docs/src/reference/adr-format.md`; the seam in
  CLAUDE.md.

## Format profile / Layout
- **Format:** `src/format.rs` (`Format` arm) + a `src/<name>.rs` with
  `serialize`/`deserialize`. Mind numeric-vs-slug identity (frontmatter is
  numeric-only — guard invalid scheme combos up front in `main`).
- **Layout:** `src/store.rs` (`Layout` arm + `list_files`/`status_dir`/
  `status_target_dir`/`detect_profile`/`migrate`). Add oracle cells + a migrate
  round-trip if applicable.

## Publish adapter (Confluence / Notion — noted as future)
- `src/publish.rs` — the export path (offline core + the adapter). A `publish`
  flag/subcommand if user-facing. Tests + docs.

## Config key
- `src/config.rs` — `CONFIG_KEYS`, `get_str`/`set_str` (validate on set),
  `env_var_for` (the `ADROIT_*` var).
- **If it is also a `--flag`/env override:** add the arm to `config_cli_value` in
  `main.rs`. (Forgetting this was a real bug — `config show`/`get` then reports the
  file/default value and *ignores the flag*. Covered by `tests/config_precedence.rs`.)
- **Tests:** `tests/config_precedence.rs`. **Docs:** CLI reference.

## CLI subcommand
- `src/cli.rs` — the `Command` enum + place it in a `help_template` category (the
  `commands_are_all_grouped` test guards this) — plus a handler in `main.rs`.
- **Tests:** `tests/cli.rs`. **Docs:** `docs/src/reference/cli.md`.
