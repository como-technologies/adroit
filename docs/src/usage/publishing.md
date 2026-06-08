# Publishing

`adroit publish` renders the **accepted** ADR set into a static-site
generator's file shape, ready for a docs site to host. It is pure and offline —
no network, no credentials — so it runs anywhere, including CI.

> **adroit produces, it does not host.** Each target writes a directory in the
> generator's layout; a consuming repo's CI then builds and deploys it. The
> networked push to a hosted wiki (Confluence / Notion) is that pipeline's job,
> deliberately out of adroit's scope — see the
> [roadmap](../dev/roadmap.md#forge-trackers--publishing).

## Usage

```sh
adroit publish --to <target> --out <dir> [--dry-run]
```

- `--to` — the static-site shape (below). Defaults to `static`; can also be set
  with the `publish_target` config key or `ADROIT_PUBLISH_TARGET`.
- `--out` — the output directory. It is created if missing.
- `--dry-run` — print what would be written without writing.

Re-running is an idempotent overwrite: the same on-disk state renders the same
bytes, so `publish` is safe to run on every merge.

Only **accepted** ADRs are published. Each page's cross-links are rewritten so
the published tree is self-contained: a link to another *published* ADR is
retargeted to its page, and a link to any other ADR (e.g. a still-proposed one)
is unlinked to plain text. When the repo uses categories (the `by_category`
layout), ADRs are grouped into sections; otherwise they form one flat list.

## Targets

| `--to` | Output shape |
|---|---|
| `static` *(default)* | Plain directory: the ADR markdown files + a generated, grouped `index.md`. |
| `mdbook` | `book.toml`, `src/SUMMARY.md` (category `# Header` groups), `src/README.md`, pages under `src/`. |
| `mkdocs` | `mkdocs.yml` with a grouped `nav:`, pages under `docs/` (category subfolders). |
| `hugo` | `content/adr/**` pages with TOML front matter (`title`/`date`/`weight`), a section `_index.md` per category. |
| `docusaurus` | `docs/**` pages with front matter (`title`/`sidebar_position`), `intro.md`, a `_category_.json` per category. |
| `jekyll` | `_config.yml` declaring an `adrs` collection, pages under `_adrs/`, a permalink `index.md`. |

For the front-matter targets (`hugo`, `docusaurus`, `jekyll`) the ADR's H1 is
dropped from the page body — the title lives in front matter — so the generator
doesn't render the title twice. The other targets keep the markdown verbatim.

## Examples

```sh
# Export to a plain directory (the default).
adroit publish --out ./public/adrs

# Render a Hugo content section, previewing first.
adroit publish --to hugo --out ./site/content --dry-run
adroit publish --to hugo --out ./site/content

# Pin a default target in config, then just `publish`.
adroit config set publish_target mkdocs
adroit publish --out ./docs-site
```

## In CI

`publish` pairs naturally with a docs deploy. After a merge to the default
branch, render the accepted set and hand the directory to the generator your
site already uses:

```sh
adroit publish --to mkdocs --out ./site
mkdocs build -f ./site/mkdocs.yml   # the consuming repo's build/deploy step
```
