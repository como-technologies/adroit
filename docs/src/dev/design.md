# Design principles — statelessness & idempotency

Two invariants shape adroit's architecture. Every change is expected to preserve
them, and the test suite guards them.

## The only state is the filesystem

A command's entire input is the ADR documents on disk, plus the configuration
resolved at startup (precedence: flag > process-env > `.env` > `config.yaml` >
built-in default). There is deliberately **no** daemon, database, cache, index
file, lock file, or session/cross-command state. Running a command is a pure
function of "the documents right now" and "the config right now".

- The single process-global, `GIT_STRICT_WARNED` in `src/query.rs`, is a
  warn-once-per-process flag so `date_source = git` doesn't repeat its
  "not a git repo / shallow clone" warning within one run. It resets on every
  invocation and never affects command output.
- `adroit serve` reopens the store **per request**, so every API response
  reflects current on-disk state. Its only in-process state is the active-directory
  pointer (switchable via `POST /api/workspace`) and the live-reload filesystem
  watcher — both scoped to that one `serve` process, neither persisted.

Because there is no stored state, there is nothing to migrate, corrupt, or get
out of sync: delete the config and the ADRs are still fully described by their
own files; point adroit at any directory and it reads the truth from disk.

## Commands are idempotent where it makes sense

A mutating command reads the current on-disk state, computes the target, and
writes **only what differs**. Writes are minimal-diff, so a file already in the
target state round-trips byte-identical. Running the same command twice, with the
same arguments against the same on-disk state, is therefore a no-op the second
time.

**Idempotent verbs** — re-running is byte-identical:

| Verb | Re-run behavior |
|------|-----------------|
| `set-status` | status already set → no change (incl. the dir move + relink) |
| `supersede` | supersession already recorded both directions → no change |
| `set-review` (and `--clear`) | deadline already set/cleared → no change |
| `relink` | links already canonical → writes nothing |
| `migrate` | converges to a fixpoint, then "nothing to migrate" |
| `index` | regenerates the same `SUMMARY.md` |
| `link` | link rewriting reaches a fixpoint |
| `publish` | re-exports the same accepted set |
| `check` | read-only |

**Intentionally non-idempotent verbs** are *imperative events*, not
*desired-state assertions* — repeating them repeats the event, by design:

| Verb | Why repeating is a new event |
|------|------------------------------|
| `new` | allocates the next ADR number / file each run |
| `renumber old new` | one-shot rename; re-running fails because `old` is gone |
| `notify` | posts a fresh webhook message each run |
| forge / git side effects | issue / PR creation, commit, push are real events |

The mental model: declarative "converge to this state" verbs are idempotent;
imperative "do this thing now" verbs are not, and shouldn't pretend to be.

## Implications for contributors

- Prefer **converge** semantics for any new mutation: read state → compute target
  → write only if different. Don't rewrite a file you didn't need to change.
- Don't introduce hidden persisted state — a cache, a lock file, a daemon, an
  on-disk "last run" marker. If you think you need one, that's a design smell;
  the filesystem is the state.
- The guard test `commands_are_idempotent` (`tests/cli.rs`) runs the idempotent
  verbs twice and asserts the repo is byte-identical. The model-based oracle
  (`tests/model.rs`) also asserts link-canonicality and clean `check` after every
  command in a random sequence, which catches accumulation bugs. Keep both green.

See also: [Testing & Fuzzing](./testing.md) and [Hardening & Quality](./hardening.md).
