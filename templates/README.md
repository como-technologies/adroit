# adroit templates

Every template adroit ships or scaffolds lives here, grouped by kind. There are
two consumption models:

- **Embedded** (`adr/`, `review/`, `ai/`) — compiled into the binary with
  `include_str!`, so `adroit` stays a single self-contained executable. The
  source-of-truth is the `.md` file here (editable, diffable, markdown-previewable
  — not a Rust string literal); edit it and rebuild.
- **Copied** (`ci/`) — reference starters a user copies into *their own* repo;
  adroit doesn't load them.

## Layout

| Dir | Kind | What | Loaded by |
|---|---|---|---|
| `adr/` | embedded | ADR scaffolds — `madr.md`, `nygard.md` | `template::{MADR,NYGARD}` → `render()` (`adroit new`) |
| `review/` | embedded | `kickoff.md` — the review-kickoff doc | `template::REVIEW_KICKOFF` → `render_kickoff()` (`adroit review`) |
| `ai/` | embedded | AI prompts — `<verb>.system.md` (model role) + `<verb>.prompt.md` (per-call template) for `interview`/`plan`/`lint`/`summary`/`ask` | `ai::build_*_request()` |
| `ci/` | copied | GitHub/GitLab pipeline starters | copied into your repo — see [`ci/README.md`](ci/README.md) |

## Placeholders

The embedded templates use `{{name}}` substitution: the ADR/review templates are
filled by `template::render*` (`{{heading}}`, `{{status}}`, `{{number}}`,
`{{title}}`, `{{date}}`, …); the AI prompt templates are filled by `ai::build_*`
(`{{title}}`, `{{adr_body}}`, `{{corpus_block}}`, `{{context}}`, …). The
`.system.md` files carry no placeholders (they're the model's fixed instructions).

## Not the same as `templates_dir`

This directory is the **build-time source** of adroit's *built-in* templates. It is
distinct from the runtime **`templates_dir`** config, which is where a *consuming*
repo keeps its own custom ADR templates — `adroit new --template <name>` resolves
`templates_dir/<name>.md` at runtime (see the [CLI reference](../docs/src/reference/cli.md)).

> **Roadmap (RFC #5):** the `ai/` prompts are embedded today; a future step is
> making them user-overridable (a repo-local `templates/ai/`), the same way ADR
> templates already support `templates_dir`.
