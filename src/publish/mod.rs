//! `adroit publish`: render the **accepted** ADR set into a static-site
//! generator's file shape — a `Publisher` seam with one variant per target.
//!
//! adroit **produces** the published artifact; it does not **host** it. Each
//! adapter is a pure, offline writer (no network, no credentials) that turns the
//! accepted set into the directory layout a generator expects — front matter,
//! nav/landing files, and cross-links rewritten to point only at *published*
//! pages. A consuming repo's CI then hosts the produced tree (the networked
//! Confluence / Notion push is that pipeline's job, deliberately out of scope —
//! see `docs/src/dev/roadmap.md`).
//!
//! Adding a target = one [`PublishTarget`] arm + one module implementing
//! [`Publisher`]; the shared work (accepted-set model, category grouping,
//! cross-link rewriting, idempotent write) lives here, so an adapter only
//! *serializes* the [`PublishModel`].

mod docusaurus;
mod hugo;
mod jekyll;
mod mdbook;
mod mkdocs;
mod static_dir;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::adr::Status;
use crate::links;
use crate::naming::{AdrRef, NamingScheme};
use crate::query::{self, Filter};
use crate::store::Store;

/// Which static-site shape `adroit publish` renders the accepted set into. The
/// publish seam: each variant maps to one [`Publisher`] module.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    Serialize,
    Deserialize,
    strum::Display,
    strum::EnumString,
    clap::ValueEnum,
)]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum PublishTarget {
    /// A plain directory: the ADR markdown files plus a generated `index.md`
    /// (the original, default behaviour).
    #[default]
    Static,
    /// An mdBook source tree (`book.toml` + `src/SUMMARY.md` + pages).
    Mdbook,
    /// A MkDocs project (`mkdocs.yml` nav + `docs/` pages).
    Mkdocs,
    /// A Hugo content section (`content/adr/**` with TOML front matter).
    Hugo,
    /// A Docusaurus docs tree (`docs/**` with front matter + `_category_.json`).
    Docusaurus,
    /// A Jekyll site (`_config.yml` collection + `_adrs/` pages).
    Jekyll,
}

impl PublishTarget {
    fn publisher(self) -> Box<dyn Publisher> {
        match self {
            PublishTarget::Static => Box::new(static_dir::StaticDir),
            PublishTarget::Mdbook => Box::new(mdbook::MdBook),
            PublishTarget::Mkdocs => Box::new(mkdocs::MkDocs),
            PublishTarget::Hugo => Box::new(hugo::Hugo),
            PublishTarget::Docusaurus => Box::new(docusaurus::Docusaurus),
            PublishTarget::Jekyll => Box::new(jekyll::Jekyll),
        }
    }
}

/// One accepted ADR, prepared for publishing.
#[derive(Debug, Clone)]
pub struct PublishedAdr {
    /// Display reference (e.g. `ADR-0001`).
    pub reference: String,
    /// The scheme-agnostic ref, for cross-link resolution.
    pub adr_ref: AdrRef,
    /// Filename stem of the source ADR (no `.md`), e.g. `0001-use-postgres`.
    pub slug: String,
    /// Short decision title.
    pub title: String,
    /// Category (the `by_category` area), `None` for flat/by_status repos.
    pub category: Option<String>,
    /// Creation date as `YYYY-MM-DD`, `None` if unknown.
    pub date: Option<String>,
    /// Raw on-disk markdown (H1 heading + status + body), cross-links *not* yet
    /// rewritten — that happens per-target in [`render`].
    pub raw: String,
}

/// The accepted set grouped into sections (by category, else one flat section),
/// plus the naming scheme for cross-link resolution. Built once, serialized by
/// each [`Publisher`].
#[derive(Debug, Clone)]
pub struct PublishModel {
    /// Accepted ADRs in publish order (number/date ascending).
    pub adrs: Vec<PublishedAdr>,
    /// Grouping: category sections when the repo uses categories, else a single
    /// unnamed section over every ADR.
    pub sections: Vec<Section>,
    /// Naming scheme, for resolving in-body cross-links to published pages.
    pub naming: NamingScheme,
}

/// A group of ADRs in the published output. `name` is the category, or `None`
/// for the single flat section of a non-category repo.
#[derive(Debug, Clone)]
pub struct Section {
    pub name: Option<String>,
    /// Indices into [`PublishModel::adrs`], in publish order.
    pub indices: Vec<usize>,
}

/// One file an adapter wants written, relative to the output directory.
#[derive(Debug, Clone)]
pub struct OutputFile {
    pub path: PathBuf,
    pub contents: String,
}

/// The per-page context handed to [`Publisher::render_page`].
pub struct PageCtx<'a> {
    /// The ADR being rendered.
    pub adr: &'a PublishedAdr,
    /// Zero-based publish order (for `weight` / `sidebar_position` / `nav_order`).
    pub order: usize,
    /// Full markdown (H1 + body) with cross-links already rewritten for this
    /// target. Use for generators that key the title off the H1.
    pub raw: &'a str,
    /// Body with the leading H1 stripped (cross-links rewritten). Use for
    /// front-matter generators that carry the title in front matter, to avoid a
    /// duplicate title.
    pub body: &'a str,
}

/// A static-site target: where each ADR page goes, how a page is serialized, and
/// the auxiliary nav/landing/config files the generator needs. The shared model,
/// cross-link rewriting, and idempotent write live in [`render`] / [`publish`].
pub trait Publisher {
    /// Output-relative path for this ADR's page file.
    fn page_path(&self, adr: &PublishedAdr) -> PathBuf;
    /// The page file's contents (front matter + body).
    fn render_page(&self, ctx: &PageCtx) -> String;
    /// Auxiliary files: nav/config/landing. `pages[i]` is the path
    /// [`page_path`](Self::page_path) returned for `model.adrs[i]`.
    fn aux_files(&self, model: &PublishModel, pages: &[PathBuf]) -> Vec<OutputFile>;
}

/// Outcome of a [`publish`] run.
#[derive(Debug)]
pub struct PublishReport {
    /// The target shape that was rendered.
    pub target: PublishTarget,
    /// Number of accepted ADR pages written (or that would be, on a dry run).
    pub written: usize,
    /// `(title, output-relative page path)` for each published ADR, in order.
    pub files: Vec<(String, String)>,
    /// The output directory.
    pub out: PathBuf,
}

/// Render the accepted set in `store` into `target`'s shape and write it to
/// `out`. With `apply == false` nothing is written — the report describes what
/// would happen. Idempotent: re-running overwrites the same files byte-for-byte.
pub fn publish(
    store: &Store,
    target: PublishTarget,
    out: &Path,
    apply: bool,
) -> anyhow::Result<PublishReport> {
    let model = build_model(store)?;
    let publisher = target.publisher();
    let pages: Vec<PathBuf> = model.adrs.iter().map(|a| publisher.page_path(a)).collect();

    let files = render_with_pages(&model, publisher.as_ref(), &pages);

    let report = PublishReport {
        target,
        written: model.adrs.len(),
        files: model
            .adrs
            .iter()
            .zip(&pages)
            .map(|(a, p)| (a.title.clone(), p.to_string_lossy().into_owned()))
            .collect(),
        out: out.to_path_buf(),
    };

    if apply {
        for f in &files {
            let full = out.join(&f.path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&full, &f.contents)?;
        }
    }
    Ok(report)
}

/// Build the accepted-set [`PublishModel`] from the store (read-only).
pub fn build_model(store: &Store) -> anyhow::Result<PublishModel> {
    let naming = store.options().naming;
    let rows = query::summaries(
        store,
        &Filter {
            status: Some(Status::Accepted),
            ..Default::default()
        },
    )?;

    let mut adrs = Vec::new();
    for row in &rows {
        let Some(r) = naming.parse_ref(&row.address) else {
            continue;
        };
        let path = store.find_path_by_ref(&r)?;
        let adr = store.read(&path)?;
        let raw = std::fs::read_to_string(&path)?;
        let slug = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("adr")
            .to_string();
        let date = row
            .created
            .as_deref()
            .map(|c| c.get(..10).unwrap_or(c).to_string());
        adrs.push(PublishedAdr {
            reference: row.reference.clone(),
            adr_ref: r,
            slug,
            title: row.title.clone(),
            category: adr.category.clone(),
            date,
            raw,
        });
    }

    let sections = build_sections(&adrs);
    Ok(PublishModel {
        adrs,
        sections,
        naming,
    })
}

/// Group ADRs by category when any is categorized (preserving first-seen order),
/// else a single unnamed section over the whole accepted set.
fn build_sections(adrs: &[PublishedAdr]) -> Vec<Section> {
    if !adrs.iter().any(|a| a.category.is_some()) {
        return vec![Section {
            name: None,
            indices: (0..adrs.len()).collect(),
        }];
    }
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, a) in adrs.iter().enumerate() {
        let cat = a
            .category
            .clone()
            .unwrap_or_else(|| "uncategorized".to_string());
        if !groups.contains_key(&cat) {
            order.push(cat.clone());
        }
        groups.entry(cat).or_default().push(i);
    }
    order
        .into_iter()
        .map(|name| Section {
            indices: groups.remove(&name).unwrap_or_default(),
            name: Some(name),
        })
        .collect()
}

/// Render every page (cross-links rewritten for `pages`) plus the adapter's
/// auxiliary files. Pure — the caller writes the result.
fn render_with_pages(
    model: &PublishModel,
    publisher: &dyn Publisher,
    pages: &[PathBuf],
) -> Vec<OutputFile> {
    let ref_to_page: HashMap<AdrRef, PathBuf> = model
        .adrs
        .iter()
        .zip(pages)
        .map(|(a, p)| (a.adr_ref.clone(), p.clone()))
        .collect();

    let mut files = Vec::new();
    for (i, adr) in model.adrs.iter().enumerate() {
        let raw = rewrite_published_links(
            &adr.raw,
            &pages[i],
            &model.naming,
            &ref_to_page,
            adr.category.as_deref(),
        );
        let body = strip_h1(&raw);
        let ctx = PageCtx {
            adr,
            order: i,
            raw: &raw,
            body: &body,
        };
        files.push(OutputFile {
            path: pages[i].clone(),
            contents: publisher.render_page(&ctx),
        });
    }
    files.extend(publisher.aux_files(model, pages));
    files
}

/// Rewrite a body's relative `.md` cross-links so the published tree is
/// self-contained: a link to another *published* ADR is retargeted to that
/// page's path (relative to this page); a link to any other `.md` ADR (not in
/// the published set — e.g. a still-proposed decision) is **unlinked**, keeping
/// the label text. External URLs and anchors are left byte-for-byte.
///
/// Pure and hostile-input-safe — `tests/parsers.rs` / `tests/fuzz_parsers.rs`
/// drive it over arbitrary content (multibyte, lone `\r`, adversarial brackets),
/// so it is `pub`.
pub fn rewrite_published_links(
    content: &str,
    source_page: &Path,
    naming: &NamingScheme,
    ref_to_page: &HashMap<AdrRef, PathBuf>,
    source_category: Option<&str>,
) -> String {
    let source_dir = source_page.parent().unwrap_or(Path::new(""));
    let bytes = content.as_bytes();
    let mut out = String::with_capacity(content.len());
    let mut last = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b']'
            && i + 1 < bytes.len()
            && bytes[i + 1] == b'('
            && let Some(rel) = content[i + 2..].find(')')
        {
            let tstart = i + 2;
            let tend = tstart + rel;
            let target = &content[tstart..tend];
            if links::is_relative_md(target) {
                let anchor = target.split_once('#').map(|(_, a)| a);
                match naming
                    .ref_in_link_from(target, source_category)
                    .and_then(|r| ref_to_page.get(&r))
                {
                    // Published ADR → retarget the link to its page.
                    Some(page) => {
                        let mut newt = links::rel_link(source_dir, page);
                        if let Some(a) = anchor {
                            newt.push('#');
                            newt.push_str(a);
                        }
                        out.push_str(&content[last..tstart]);
                        out.push_str(&newt);
                        last = tend;
                        i = tend + 1;
                        continue;
                    }
                    // A `.md` ADR link that isn't published → unlink (keep label).
                    None => {
                        // The label's `[` must sit in the not-yet-emitted region
                        // (`lb >= last`). With nested/adjacent brackets the nearest
                        // `[` can be one we already consumed (`lb < last`), which
                        // would make `content[last..lb]` a backwards range — leave
                        // such a link untouched instead of panicking.
                        if let Some(lb) = content[..i].rfind('[')
                            && lb >= last
                            && !content[lb..i].contains('\n')
                        {
                            out.push_str(&content[last..lb]);
                            out.push_str(&content[lb + 1..i]);
                            last = tend + 1;
                            i = tend + 1;
                            continue;
                        }
                    }
                }
            }
            i = tend + 1;
            continue;
        }
        i += 1;
    }
    out.push_str(&content[last..]);
    out
}

/// Drop a markdown body's leading `# …` H1 (and one following blank line), so a
/// front-matter target doesn't render the title twice. Line endings normalize to
/// `\n`. A body with no H1 is returned with endings normalized.
pub fn strip_h1(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut removed_h1 = false;
    let mut eat_blank = false;
    for line in raw.lines() {
        if !removed_h1 && line.starts_with("# ") {
            removed_h1 = true;
            eat_blank = true;
            continue;
        }
        if eat_blank {
            eat_blank = false;
            if line.trim().is_empty() {
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// The output directory segment for an ADR's category, when categorized.
pub(crate) fn category_segment(adr: &PublishedAdr) -> Option<String> {
    adr.category.as_deref().map(crate::naming::slugify)
}

/// Double-quote a string for YAML / JSON / TOML front matter (escaping `"`, `\`,
/// and newlines) — titles and labels can contain `:` and quotes.
pub(crate) fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A small fixture model (two accepted ADRs; categorized into `data`/`arch` when
/// asked) for the per-adapter writer tests.
#[cfg(test)]
pub(crate) fn test_model(categorized: bool) -> PublishModel {
    let mk =
        |slug: &str, reference: &str, title: &str, cat: Option<&str>, body: &str| PublishedAdr {
            reference: reference.to_string(),
            adr_ref: AdrRef::Number(slug.get(..4).and_then(|d| d.parse().ok()).unwrap_or(0)),
            slug: slug.to_string(),
            title: title.to_string(),
            category: cat.map(str::to_string),
            date: Some("2026-05-31".to_string()),
            raw: format!("# {title}\n\n## Status\n\nAccepted\n\n{body}\n"),
        };
    let adrs = vec![
        mk(
            "0001-use-postgres",
            "ADR-0001",
            "Use PostgreSQL",
            categorized.then_some("data"),
            "Pick PG.",
        ),
        mk(
            "0002-hexagonal",
            "ADR-0002",
            "Hexagonal architecture",
            categorized.then_some("arch"),
            "Layer it.",
        ),
    ];
    let sections = build_sections(&adrs);
    PublishModel {
        adrs,
        sections,
        naming: NamingScheme::Sequential,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::StoreOptions;

    /// Seed a store with two accepted ADRs (one links to the other) + one
    /// proposed ADR, returning the store handle's tempdir + root.
    fn seed() -> (tempfile::TempDir, Store) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("adrs");
        let store = Store::open_or_create_with(&root, StoreOptions::default()).unwrap();
        std::fs::create_dir_all(root.join("accepted")).unwrap();
        std::fs::create_dir_all(root.join("proposed")).unwrap();
        std::fs::write(
            root.join("accepted/0001-use-postgres.md"),
            "# ADR-0001: Use PostgreSQL\n\n## Status\n\nAccepted\n\n## Context\n\nWe pick PG.\n",
        )
        .unwrap();
        std::fs::write(
            root.join("accepted/0002-hexagonal.md"),
            "# ADR-0002: Hexagonal architecture\n\n## Status\n\nAccepted\n\n## Context\n\nSee [ADR-0001](../accepted/0001-use-postgres.md) and [ADR-0003](../proposed/0003-maybe.md).\n",
        )
        .unwrap();
        std::fs::write(
            root.join("proposed/0003-maybe.md"),
            "# ADR-0003: Maybe\n\n## Status\n\nProposed\n",
        )
        .unwrap();
        (tmp, store)
    }

    #[test]
    fn model_holds_only_accepted_in_order() {
        let (_tmp, store) = seed();
        let model = build_model(&store).unwrap();
        assert_eq!(model.adrs.len(), 2);
        assert_eq!(model.adrs[0].reference, "ADR-0001");
        assert_eq!(model.adrs[1].reference, "ADR-0002");
        // No categories → one flat section over both.
        assert_eq!(model.sections.len(), 1);
        assert_eq!(model.sections[0].indices, vec![0, 1]);
    }

    #[test]
    fn cross_link_to_published_is_retargeted_unpublished_is_unlinked() {
        let (_tmp, store) = seed();
        let model = build_model(&store).unwrap();
        // A flat target where page i lives at `<slug>.md`.
        let pages: Vec<PathBuf> = model
            .adrs
            .iter()
            .map(|a| PathBuf::from(format!("{}.md", a.slug)))
            .collect();
        let ref_to_page: HashMap<AdrRef, PathBuf> = model
            .adrs
            .iter()
            .zip(&pages)
            .map(|(a, p)| (a.adr_ref.clone(), p.clone()))
            .collect();
        let rewritten = rewrite_published_links(
            &model.adrs[1].raw,
            &pages[1],
            &model.naming,
            &ref_to_page,
            None,
        );
        // ADR-0001 is published → link retargeted to its page (same dir).
        assert!(
            rewritten.contains("[ADR-0001](./0001-use-postgres.md)"),
            "retargeted: {rewritten}"
        );
        // ADR-0003 is only proposed → unlinked to plain label text.
        assert!(rewritten.contains("ADR-0003"), "label kept: {rewritten}");
        assert!(
            !rewritten.contains("0003-maybe.md"),
            "unpublished link dropped: {rewritten}"
        );
    }

    #[test]
    fn strip_h1_drops_heading_and_blank() {
        let body = strip_h1("# ADR-0001: T\n\n## Status\n\nAccepted\n");
        assert!(!body.contains("# ADR-0001"));
        assert!(body.starts_with("## Status"));
    }

    #[test]
    fn rewrite_handles_nested_brackets_without_panicking() {
        // Regression (harden): nested/adjacent links — `[[x](0001-a.md)](0002-b.md)`
        // — made the unlink branch compute `content[last..lb]` with `last > lb`
        // (the nearest `[` was already consumed), a backwards-range panic.
        let empty: HashMap<AdrRef, PathBuf> = HashMap::new();
        let out = rewrite_published_links(
            "[[x](0001-a.md)](0002-b.md)",
            Path::new("p/page.md"),
            &NamingScheme::Sequential,
            &empty,
            None,
        );
        // No panic is the property; the cleanly-labeled inner link is unlinked.
        assert!(out.contains('x'), "{out}");
    }

    #[test]
    fn publish_is_idempotent() {
        let (_tmp, store) = seed();
        let out = tempfile::tempdir().unwrap();
        for target in [
            PublishTarget::Static,
            PublishTarget::Mdbook,
            PublishTarget::Mkdocs,
            PublishTarget::Hugo,
            PublishTarget::Docusaurus,
            PublishTarget::Jekyll,
        ] {
            let dir = out.path().join(target.to_string());
            let first = publish(&store, target, &dir, true).unwrap();
            let snap1 = snapshot(&dir);
            // Re-running writes the same bytes.
            let _ = publish(&store, target, &dir, true).unwrap();
            let snap2 = snapshot(&dir);
            assert_eq!(snap1, snap2, "{target} not idempotent");
            assert_eq!(first.written, 2, "{target} wrote both accepted ADRs");
        }
    }

    /// Sorted `(relpath, contents)` of every file under `dir`, for snapshotting.
    fn snapshot(dir: &Path) -> Vec<(String, String)> {
        let mut out = Vec::new();
        collect(dir, dir, &mut out);
        out.sort();
        out
    }
    fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect(root, &p, out);
            } else {
                let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
                let contents = std::fs::read_to_string(&p).unwrap_or_default();
                out.push((rel, contents));
            }
        }
    }
}
