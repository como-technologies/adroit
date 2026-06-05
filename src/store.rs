use std::path::{Path, PathBuf};

use crate::adr::{Adr, Number, Status};
use crate::config::{DateSource, Layout, RelinkScope};
use crate::format::{self, Format};
use crate::naming::{AdrRef, NamingScheme};

/// Errors that can occur during ADR storage operations.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("ADR directory not found: {0}")]
    NotFound(PathBuf),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("failed to parse ADR: {0}")]
    Parse(String),

    #[error("no ADR found with number {0}")]
    NumberNotFound(Number),
}

/// Outcome of a [`Store::relink`] pass.
#[derive(Debug, Clone, Default)]
pub struct RelinkReport {
    /// Number of files whose content was (or would be) rewritten.
    pub files_changed: usize,
    /// Total cross-ADR links rewritten across those files.
    pub links_rewritten: usize,
    /// Files (relative to the store root) that changed — for dry-run display.
    pub changed_files: Vec<PathBuf>,
}

/// What an ADR directory looks like on disk, inferred from the files present
/// (independent of how a [`Store`] is configured). `None` for a dimension when
/// the repo is empty or it can't be determined.
#[derive(Debug, Clone, Copy, Default)]
pub struct Detected {
    pub layout: Option<Layout>,
    pub format: Option<Format>,
}

/// A planned (or applied) profile migration — see [`Store::migrate`].
#[derive(Debug, Default)]
pub struct MigrateReport {
    pub layout_change: Option<(Layout, Layout)>,
    pub format_change: Option<(Format, Format)>,
    /// Number of ADR files migrated.
    pub files: usize,
    /// `(from, to)` (relative to the store root) for each file that moves.
    pub moves: Vec<(PathBuf, PathBuf)>,
    /// `true` if the migration was applied (not just planned).
    pub applied: bool,
    /// Cross-ADR links rewritten afterward.
    pub links_rewritten: usize,
}

impl MigrateReport {
    /// `true` when source and target profiles already match (nothing to do).
    pub fn is_noop(&self) -> bool {
        self.layout_change.is_none() && self.format_change.is_none()
    }
}

/// Which typed relational link to add/remove via [`Store::set_links_ref`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    RelatesTo,
    DependsOn,
    Refines,
}

/// Outcome of [`Store::renumber`].
#[derive(Debug, Default)]
pub struct RenumberReport {
    pub from: u32,
    pub to: u32,
    /// Files rewritten (the renamed ADR + every file with an inbound reference).
    pub files_updated: usize,
}

/// How a [`Store`] is configured to serialize and lay out ADRs.
#[derive(Debug, Clone, Default)]
pub struct StoreOptions {
    pub format: Format,
    pub layout: Layout,
    /// Map from status to directory name (used by `by_status` layout).
    /// Resolved by the caller from config; empty falls back to lowercase names.
    pub status_dir: std::collections::BTreeMap<Status, String>,
    /// Age (in days) past which a still-`Proposed` ADR is flagged review-due
    /// even with no explicit `review_by`. `None` disables age-based flagging
    /// (deadline-only). Carried from config so the shared query layer can apply
    /// it identically across surfaces.
    pub review_overdue_days: Option<u32>,
    /// Where the query layer reads ADR dates/lifecycle from (carried from config).
    pub date_source: DateSource,
    /// How ADR identifiers / filenames are formed (carried from config). Drives
    /// `write`/`read` identity + filename via the `naming` seam.
    pub naming: NamingScheme,
    /// How much a status-change *move* auto-relinks (carried from config). Only
    /// `set_status_at` consults this; `relink`/`renumber`/`migrate` are always
    /// full-scope.
    pub relink_scope: RelinkScope,
}

impl StoreOptions {
    /// Build options matching the original behaviour: flat + frontmatter.
    pub fn flat_frontmatter() -> Self {
        Self {
            format: Format::Frontmatter,
            layout: Layout::Flat,
            status_dir: std::collections::BTreeMap::new(),
            review_overdue_days: None,
            date_source: DateSource::Auto,
            naming: NamingScheme::Sequential,
            relink_scope: RelinkScope::All,
        }
    }

    fn dir_name(&self, status: Status) -> String {
        self.status_dir
            .get(&status)
            .cloned()
            .unwrap_or_else(|| status.to_string().to_lowercase())
    }
}

/// Generate the canonical sequential filename for an ADR: `0001-some-title.md`.
/// Production code now routes filename generation through the naming seam
/// (`NamingScheme::filename`); this is retained as a test-only guard that the
/// sequential format stays byte-identical.
#[cfg(test)]
fn filename(number: Number, title: &str) -> String {
    let slug: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        format!("{number}.md")
    } else {
        format!("{number}-{slug}.md")
    }
}

/// Parse the leading zero-padded number from a filename like `0006-foo.md`.
fn number_from_filename(path: &Path) -> Option<Number> {
    let name = path.file_name()?.to_str()?;
    let digits: String = name.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<u32>().ok().map(Number::new)
}

/// True if this directory entry is an ADR file (`*.md`, not `README.md`).
fn is_adr_file(path: &Path) -> bool {
    if path.extension().is_none_or(|ext| ext != "md") {
        return false;
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    !name.eq_ignore_ascii_case("README.md") && !name.eq_ignore_ascii_case("adr-template.md")
}

/// Manages reading and writing ADRs on disk.
#[derive(Debug)]
pub struct Store {
    /// Root directory containing the ADR files (the `adrs/` dir).
    root: PathBuf,
    opts: StoreOptions,
}

impl Store {
    /// Open an existing ADR store at the given path with default options.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        Self::open_with(root, StoreOptions::default())
    }

    /// Open an existing ADR store with explicit options.
    pub fn open_with(root: impl Into<PathBuf>, opts: StoreOptions) -> Result<Self, StoreError> {
        let root = root.into();
        if !root.is_dir() {
            return Err(StoreError::NotFound(root));
        }
        Ok(Self { root, opts })
    }

    /// Open an ADR store with default options, creating the dir if missing.
    pub fn open_or_create(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        Self::open_or_create_with(root, StoreOptions::default())
    }

    /// Open an ADR store with explicit options, creating the dir if missing.
    pub fn open_or_create_with(
        root: impl Into<PathBuf>,
        opts: StoreOptions,
    ) -> Result<Self, StoreError> {
        let root = root.into();
        if !root.is_dir() {
            std::fs::create_dir_all(&root)?;
        }
        Ok(Self { root, opts })
    }

    /// Return the root path of this store.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Return the store options.
    pub fn options(&self) -> &StoreOptions {
        &self.opts
    }

    /// The on-disk serialization format this store uses. Lets the query layer
    /// decide date precedence (an authored frontmatter `created:` is meaningful;
    /// a markdown ADR has none, so git is authoritative there).
    pub fn format(&self) -> Format {
        self.opts.format
    }

    /// The directory a given status maps to (absolute path under root).
    /// In `by_category` the status doesn't pick the directory (the category
    /// does), so this falls back to the root — callers that place/move files in
    /// `by_category` use [`Store::category_dir`] instead.
    pub fn status_dir(&self, status: Status) -> PathBuf {
        match self.opts.layout {
            Layout::Flat | Layout::ByCategory => self.root.clone(),
            Layout::ByStatus => self.root.join(self.opts.dir_name(status)),
        }
    }

    /// The directory for a category under the `by_category` layout.
    pub fn category_dir(&self, category: &str) -> PathBuf {
        self.root.join(category)
    }

    /// Where a file at `path` should live after a status change. In `by_status`
    /// this is the new status's directory (a move); in `flat`/`by_category` the
    /// file stays put (status is content-encoded, the directory is fixed).
    fn status_target_dir(&self, path: &Path, status: Status) -> PathBuf {
        match self.opts.layout {
            Layout::ByStatus => self.status_dir(status),
            Layout::Flat | Layout::ByCategory => path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.root.clone()),
        }
    }

    /// Map a file's parent directory name back to a status, if it matches one.
    ///
    /// Returns `None` in `flat` layout (no directory-implied status) or when the
    /// parent directory doesn't match a known status dir. Exposed for callers
    /// like `adroit check` that compare the directory-implied status against the
    /// status declared in a file's `## Status` section.
    pub fn dir_status(&self, path: &Path) -> Option<Status> {
        self.dir_status_inner(path)
    }

    /// Map a file's parent directory name back to a status, if it matches one.
    fn dir_status_inner(&self, path: &Path) -> Option<Status> {
        // Flat has no directory-implied status; by_category's directory is the
        // category, so status comes from the `## Status` section, not the dir.
        if matches!(self.opts.layout, Layout::Flat | Layout::ByCategory) {
            return None;
        }
        let dir = path.parent()?.file_name()?.to_str()?;
        Status::ALL
            .into_iter()
            .find(|s| self.opts.dir_name(*s).eq_ignore_ascii_case(dir))
    }

    /// List all ADR files in the store, sorted by number then name.
    pub fn list_files(&self) -> Result<Vec<PathBuf>, StoreError> {
        let mut files: Vec<PathBuf> = match self.opts.layout {
            Layout::Flat => read_md_files(&self.root)?,
            Layout::ByStatus => {
                let mut all = Vec::new();
                for status in Status::ALL {
                    let dir = self.root.join(self.opts.dir_name(status));
                    if dir.is_dir() {
                        all.extend(read_md_files(&dir)?);
                    }
                }
                all
            }
            // Every immediate subdirectory is a category; collect ADRs from each.
            Layout::ByCategory => {
                let mut all = Vec::new();
                for dir in immediate_subdirs(&self.root) {
                    all.extend(read_md_files(&dir)?);
                }
                all
            }
        };
        files.sort_by(|a, b| {
            let na = number_from_filename(a);
            let nb = number_from_filename(b);
            na.cmp(&nb).then_with(|| a.cmp(b))
        });
        Ok(files)
    }

    /// Return the next available ADR number: max across all dirs + 1.
    pub fn next_number(&self) -> Result<Number, StoreError> {
        let files = self.list_files()?;
        let max = files
            .iter()
            .filter_map(|p| number_from_filename(p).map(|n| n.get()))
            .max()
            .unwrap_or(0);
        Ok(Number::new(max + 1))
    }

    /// The identifier the next new ADR would be assigned, under the configured
    /// naming scheme. (`title`/`fresh_uuid` feed the date/uuid schemes.)
    pub fn next_ref(&self, title: &str, fresh_uuid: uuid::Uuid) -> Result<AdrRef, StoreError> {
        let existing: Vec<AdrRef> = self
            .list_with_paths()?
            .iter()
            .map(|(_, a)| a.reference())
            .collect();
        Ok(self
            .opts
            .naming
            .assign(&existing, title, today_local(), fresh_uuid))
    }

    /// The identifier the next new ADR in `category` would be assigned, under the
    /// `by_category` layout (per-directory local numbering): `category/NNNN`.
    pub fn next_ref_in_category(&self, category: &str) -> AdrRef {
        let nums = self.numbers_in_category(category);
        self.opts.naming.assign_in_category(category, &nums)
    }

    /// The local ADR numbers already used in `category`'s directory.
    fn numbers_in_category(&self, category: &str) -> Vec<u32> {
        read_md_files(&self.category_dir(category))
            .unwrap_or_default()
            .iter()
            .filter_map(|p| number_from_filename(p).map(|n| n.get()))
            .collect()
    }

    /// Write an ADR to disk using the configured naming scheme + format/layout.
    /// Assigns an identity (via the scheme) if the ADR doesn't have one yet.
    pub fn write(&self, adr: &mut Adr) -> Result<PathBuf, StoreError> {
        if adr.number.is_none() && adr.slug.is_none() {
            let r = if self.opts.layout == Layout::ByCategory {
                let category = adr.category.as_deref().ok_or_else(|| {
                    StoreError::Parse("by_category layout requires a category".into())
                })?;
                self.next_ref_in_category(category)
            } else {
                self.next_ref(&adr.title, adr.id.uuid())?
            };
            apply_ref(adr, r);
        }
        let r = adr.reference();
        let content = format::serialize(adr, self.opts.format)
            .map_err(|e| StoreError::Parse(e.to_string()))?;
        let dir = match (self.opts.layout, adr.category.as_deref()) {
            (Layout::ByCategory, Some(category)) => self.category_dir(category),
            _ => self.status_dir(adr.status),
        };
        if !dir.is_dir() {
            std::fs::create_dir_all(&dir)?;
        }
        let path = dir.join(self.opts.naming.filename(&r, &adr.title));
        std::fs::write(&path, content)?;
        Ok(path)
    }

    /// Read a single ADR from a file path, setting its identity per the scheme.
    pub fn read(&self, path: &Path) -> Result<Adr, StoreError> {
        let content = std::fs::read_to_string(path)?;
        let dir_status = self.dir_status_inner(path);
        let mut adr = format::deserialize(&content, self.opts.format, dir_status, self.opts.naming)
            .map_err(|e| StoreError::Parse(e.to_string()))?;
        if let Some(r) = self.opts.naming.parse(path, &content) {
            apply_ref(&mut adr, r);
        }
        // Under by_category the parent directory is the ADR's category.
        if self.opts.layout == Layout::ByCategory {
            adr.category = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .map(str::to_string);
        }
        // For per_category markdown, a same-category supersession link carries no
        // category segment, so `parse_markdown` (category-blind) can't resolve it.
        // Re-resolve both directions with this ADR's category.
        if self.opts.naming == NamingScheme::PerCategory && self.opts.format == Format::Markdown {
            let (supersedes, superseded_by) = format::parse_markdown_section_supersession(
                &content,
                self.opts.naming,
                adr.category.as_deref(),
            );
            adr.supersedes = supersedes;
            adr.superseded_by = superseded_by;
        }
        Ok(adr)
    }

    /// Find an ADR's file by its scheme identity (number or slug/uuid prefix).
    pub fn find_path_by_ref(&self, r: &AdrRef) -> Result<PathBuf, StoreError> {
        self.list_files()?
            .into_iter()
            .find(|p| {
                self.opts
                    .naming
                    .parse(p, "")
                    .is_some_and(|stored| self.opts.naming.ref_matches(&stored, r))
            })
            .ok_or_else(|| {
                StoreError::Parse(format!(
                    "no ADR found with id {}",
                    self.opts.naming.display(r)
                ))
            })
    }

    /// Find the file path for an ADR by its sequential number.
    pub fn find_path_by_number(&self, number: Number) -> Result<PathBuf, StoreError> {
        self.list_files()?
            .into_iter()
            .find(|p| number_from_filename(p) == Some(number))
            .ok_or(StoreError::NumberNotFound(number))
    }

    /// List all ADRs in the store, parsed from disk.
    pub fn list(&self) -> Result<Vec<Adr>, StoreError> {
        Ok(self
            .list_with_paths()?
            .into_iter()
            .map(|(_, adr)| adr)
            .collect())
    }

    /// List all ADRs paired with their on-disk file path. The query layer needs
    /// the path to look up each ADR's git history (creation date + lifecycle).
    pub fn list_with_paths(&self) -> Result<Vec<(PathBuf, Adr)>, StoreError> {
        self.list_files()?
            .into_iter()
            .map(|p| {
                let adr = self.read(&p)?;
                Ok((p, adr))
            })
            .collect()
    }

    /// Rewrite every cross-ADR relative link across the store so it points at
    /// the current location of the ADR it references (see [`crate::links`]).
    ///
    /// Idempotent: a file whose links are already canonical is left
    /// byte-identical and not rewritten, so calling this after a status-change
    /// move only touches the links that move actually invalidated. Duplicate
    /// ADR numbers are skipped (ambiguous — surfaced by `adroit check`).
    ///
    /// With `apply == false` nothing is written — the returned report describes
    /// what *would* change (for `adroit relink --dry-run`).
    pub fn relink(&self, apply: bool) -> Result<RelinkReport, StoreError> {
        let entries = self.list_with_paths()?;
        let by_ref = Self::link_resolver_map(&entries);

        let mut report = RelinkReport::default();
        for (path, _) in &entries {
            let dir = path.parent().unwrap_or_else(|| Path::new(""));
            let original = std::fs::read_to_string(path)?;
            let (rewritten, changed) = crate::links::rewrite_links(&original, dir, |target| {
                self.opts
                    .naming
                    .ref_in_link(target)
                    .and_then(|r| by_ref.get(&r).cloned())
            });
            if changed > 0 && rewritten != original {
                if apply {
                    std::fs::write(path, &rewritten)?;
                }
                report.files_changed += 1;
                report.links_rewritten += changed;
                report.changed_files.push(rel_to(&self.root, path));
            }
        }
        Ok(report)
    }

    /// Map each ADR's scheme identity to its current file, so a link target's
    /// ref (via the seam) resolves to where that ADR now lives. Identities seen
    /// more than once are ambiguous duplicates and are left out (their links are
    /// kept byte-for-byte and flagged by `check`). Shared by [`relink`] and
    /// [`relink_one`].
    fn link_resolver_map(entries: &[(PathBuf, Adr)]) -> std::collections::HashMap<AdrRef, PathBuf> {
        let mut seen: std::collections::HashMap<AdrRef, usize> = std::collections::HashMap::new();
        for (_, adr) in entries {
            *seen.entry(adr.reference()).or_default() += 1;
        }
        let mut by_ref: std::collections::HashMap<AdrRef, PathBuf> =
            std::collections::HashMap::new();
        for (path, adr) in entries {
            let r = adr.reference();
            if seen.get(&r) == Some(&1) {
                by_ref.insert(r, path.clone());
            }
        }
        by_ref
    }

    /// Relink ONLY `path`'s own outbound links (so the moved file stays
    /// internally valid), without touching any other file. Returns whether it
    /// was rewritten. This is the `self`-scoped counterpart to [`relink`]: a
    /// status-change move under `relink_scope = self` fixes the moved ADR's own
    /// links but leaves inbound links in neighbors for a post-merge `relink`, so
    /// a status-change PR touches only the ADR it is about.
    pub fn relink_one(&self, path: &Path) -> Result<bool, StoreError> {
        let entries = self.list_with_paths()?;
        let by_ref = Self::link_resolver_map(&entries);
        let dir = path.parent().unwrap_or_else(|| Path::new(""));
        let original = std::fs::read_to_string(path)?;
        let (rewritten, changed) = crate::links::rewrite_links(&original, dir, |target| {
            self.opts
                .naming
                .ref_in_link(target)
                .and_then(|r| by_ref.get(&r).cloned())
        });
        if changed > 0 && rewritten != original {
            std::fs::write(path, &rewritten)?;
            return Ok(true);
        }
        Ok(false)
    }

    /// Infer the on-disk layout + format from the files actually present,
    /// independent of how this store is configured.
    pub fn detect_profile(&self) -> Detected {
        let root_adrs = adr_files_in(&self.root);
        // ADRs under status-named subdirs (⇒ by_status) vs other subdirs (⇒
        // by_category — the directory is an area, not a status).
        let mut status_adrs = Vec::new();
        let mut category_adrs = Vec::new();
        for d in immediate_subdirs(&self.root) {
            let name = d.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let is_status = Status::ALL
                .into_iter()
                .any(|s| self.opts.dir_name(s).eq_ignore_ascii_case(name));
            if is_status {
                status_adrs.extend(adr_files_in(&d));
            } else {
                category_adrs.extend(adr_files_in(&d));
            }
        }
        let layout = if !status_adrs.is_empty() {
            Some(Layout::ByStatus)
        } else if !category_adrs.is_empty() {
            Some(Layout::ByCategory)
        } else if !root_adrs.is_empty() {
            Some(Layout::Flat)
        } else {
            None
        };
        let sample = status_adrs
            .first()
            .or_else(|| category_adrs.first())
            .or_else(|| root_adrs.first());
        let format = sample
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|c| {
                if c.trim_start().starts_with("---") {
                    Format::Frontmatter
                } else {
                    Format::Markdown
                }
            });
        Detected { layout, format }
    }

    /// If the on-disk profile disagrees with this store's configured layout /
    /// format, return a human explanation; otherwise `None`. Callers refuse to
    /// operate on a mismatch (it would hide ADRs or corrupt numbering) and point
    /// at `adroit migrate`.
    pub fn profile_mismatch(&self) -> Option<String> {
        let d = self.detect_profile();
        let mut diffs = Vec::new();
        if let Some(l) = d.layout
            && l != self.opts.layout
        {
            diffs.push(format!(
                "laid out as `{l}` but configured for `{}`",
                self.opts.layout
            ));
        }
        if let Some(f) = d.format
            && f != self.opts.format
        {
            diffs.push(format!(
                "written as `{f}` but configured for `{}`",
                self.opts.format
            ));
        }
        if diffs.is_empty() {
            return None;
        }
        Some(format!(
            "this ADR directory is {}. Run `adroit migrate` to convert it, or set \
             --layout / --format (or config / .env) to match — refusing to run to \
             avoid hiding ADRs or corrupting numbering.",
            diffs.join(", and ")
        ))
    }

    /// Plan (or, with `apply`, perform) a migration of the repo on disk to this
    /// store's configured layout/format. The source profile is auto-detected;
    /// a layout-only change moves files verbatim, a format change re-serializes,
    /// and cross-ADR links are fixed afterward via [`Store::relink`].
    pub fn migrate(&self, apply: bool) -> Result<MigrateReport, StoreError> {
        let detected = self.detect_profile();
        let src_layout = detected.layout.unwrap_or(self.opts.layout);
        let src_format = detected.format.unwrap_or(self.opts.format);

        // Converting to/from by_category would require (re)assigning categories
        // and per-category numbers — out of scope for the verbatim file-move
        // migrator. Refuse cleanly rather than mangle the repo.
        if Layout::ByCategory == src_layout || Layout::ByCategory == self.opts.layout {
            if src_layout == self.opts.layout {
                return Ok(MigrateReport::default()); // same profile → no-op
            }
            return Err(StoreError::Parse(
                "migrating to/from the by_category layout is not supported; \
                 reorganize categories by hand"
                    .into(),
            ));
        }

        let mut report = MigrateReport::default();
        if src_layout != self.opts.layout {
            report.layout_change = Some((src_layout, self.opts.layout));
        }
        if src_format != self.opts.format {
            report.format_change = Some((src_format, self.opts.format));
        }
        if report.is_noop() {
            return Ok(report);
        }

        // Read every ADR through a source-profile view of the same directory.
        let src = Store {
            root: self.root.clone(),
            opts: StoreOptions {
                format: src_format,
                layout: src_layout,
                status_dir: self.opts.status_dir.clone(),
                review_overdue_days: self.opts.review_overdue_days,
                date_source: self.opts.date_source,
                naming: self.opts.naming,
                relink_scope: self.opts.relink_scope,
            },
        };
        let entries = src.list_with_paths()?;
        report.files = entries.len();

        // Target path per ADR (filenames preserved), guarding collisions.
        let mut planned: Vec<(PathBuf, PathBuf, Adr)> = Vec::new();
        let mut claimed: std::collections::HashMap<PathBuf, PathBuf> =
            std::collections::HashMap::new();
        for (src_path, adr) in entries {
            let file_name = src_path
                .file_name()
                .map(|n| n.to_owned())
                .expect("ADR path has a filename");
            let target = self.status_dir(adr.status).join(&file_name);
            if let Some(other) = claimed.insert(target.clone(), src_path.clone()) {
                return Err(StoreError::Parse(format!(
                    "migration would collide: {} and {} both map to {}",
                    other.display(),
                    src_path.display(),
                    target.display()
                )));
            }
            if target != src_path {
                report
                    .moves
                    .push((rel_to(&self.root, &src_path), rel_to(&self.root, &target)));
            }
            planned.push((src_path, target, adr));
        }

        if !apply {
            return Ok(report);
        }

        let reserialize = report.format_change.is_some();
        for (src_path, target, adr) in planned {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if reserialize {
                let content = format::serialize(&adr, self.opts.format)
                    .map_err(|e| StoreError::Parse(e.to_string()))?;
                std::fs::write(&target, content)?;
                if target != src_path {
                    std::fs::remove_file(&src_path)?;
                }
            } else if target != src_path {
                // Layout-only: move the bytes verbatim (no reformat).
                std::fs::rename(&src_path, &target)?;
            }
        }
        report.applied = true;
        report.links_rewritten = self.relink(true)?.links_rewritten;
        Ok(report)
    }

    /// Renumber a sequential ADR from `old` to `new`: rename the file (slug
    /// preserved), rewrite its heading, retarget + relabel every inbound
    /// reference, then relink. Resolves a duplicate-number collision. `file`
    /// disambiguates when two files share `old`. Errors if `new` is taken, `old`
    /// is missing, or `old` is ambiguous without `file`.
    pub fn renumber(
        &self,
        old: Number,
        new: Number,
        file: Option<&Path>,
    ) -> Result<RenumberReport, StoreError> {
        let candidates: Vec<PathBuf> = self
            .list_files()?
            .into_iter()
            .filter(|p| number_from_filename(p) == Some(old))
            .collect();
        let old_path = match file {
            Some(f) if candidates.iter().any(|c| c == f) => f.to_path_buf(),
            Some(f) => {
                return Err(StoreError::Parse(format!(
                    "{} is not an ADR-{old} file",
                    f.display()
                )));
            }
            None => match candidates.as_slice() {
                [] => return Err(StoreError::NumberNotFound(old)),
                [one] => one.clone(),
                many => {
                    let list = many
                        .iter()
                        .map(|p| rel_to(&self.root, p).display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(StoreError::Parse(format!(
                        "ADR-{old} is ambiguous ({} files) — pass --file <path>: {list}",
                        many.len()
                    )));
                }
            },
        };
        if self
            .list_files()?
            .iter()
            .any(|p| number_from_filename(p) == Some(new))
        {
            return Err(StoreError::Parse(format!("ADR-{new} already exists")));
        }

        let old_base = old_path
            .file_name()
            .and_then(|n| n.to_str())
            .expect("ADR path has a filename")
            .to_string();
        let ndigits = old_base.chars().take_while(|c| c.is_ascii_digit()).count();
        let new_base = format!("{:04}{}", new.get(), &old_base[ndigits..]);
        let new_path = old_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(&new_base);
        let old_label = format!("ADR-{:04}", old.get());
        let new_label = format!("ADR-{:04}", new.get());

        std::fs::rename(&old_path, &new_path)?;
        let mut report = RenumberReport {
            from: old.get(),
            to: new.get(),
            files_updated: 0,
        };

        // The renamed file: update its own heading / self-references.
        let own = std::fs::read_to_string(&new_path)?;
        let own_new = own.replace(&old_label, &new_label);
        if own_new != own {
            std::fs::write(&new_path, own_new)?;
            report.files_updated += 1;
        }

        // Every other file: retarget + relabel inbound links to this ADR.
        for path in self.list_files()? {
            if path == new_path {
                continue;
            }
            let content = std::fs::read_to_string(&path)?;
            let (mut rewritten, mut n) = crate::links::relabel_links_to(
                &content, &old_base, &new_base, &old_label, &new_label,
            );
            // Frontmatter ADRs carry their supersession + typed-link refs as bare
            // numbers in the YAML block, not as markdown links, so the relabel
            // above can't reach them — remap them through the model so a renumber
            // doesn't strand e.g. another ADR's `superseded_by: <old>`.
            if self.opts.format == Format::Frontmatter
                && let Some(remapped) = crate::frontmatter::remap_numeric_refs(&rewritten, old, new)
            {
                rewritten = remapped;
                n += 1;
            }
            if n > 0 && rewritten != content {
                std::fs::write(&path, rewritten)?;
                report.files_updated += 1;
            }
        }

        // Canonicalize any relative-path drift left by the move.
        self.relink(true)?;
        Ok(report)
    }

    /// Change an ADR's status. In `by_status` markdown mode this MOVES the file
    /// to the matching status dir and rewrites the `## Status` section
    /// (minimal-diff). Returns the new path.
    pub fn set_status(&self, number: Number, new_status: Status) -> Result<PathBuf, StoreError> {
        let path = self.find_path_by_number(number)?;
        self.set_status_at(path, new_status, None)
    }

    /// Like [`set_status`] but addressed by the scheme's [`AdrRef`] (so date/uuid
    /// ADRs, which have no number, can change status from the CLI).
    pub fn set_status_ref(&self, r: &AdrRef, new_status: Status) -> Result<PathBuf, StoreError> {
        let path = self.find_path_by_ref(r)?;
        self.set_status_at(path, new_status, None)
    }

    /// Mark `old` as superseded by `new` (both addressed by scheme identity),
    /// moving it to the superseded dir and writing a `Superseded by
    /// [<label>](...)` status with a relative link.
    pub fn supersede(&self, new: &AdrRef, old: &AdrRef) -> Result<PathBuf, StoreError> {
        // Validate the new ADR exists before mutating the old one.
        let new_path = self.find_path_by_ref(new)?;
        let old_path = self.find_path_by_ref(old)?;
        // The supersession link must be relative to where the OLD ADR ends up: it
        // moves to the superseded dir under by_status, but STAYS in its current
        // directory under flat / by_category. Computing it from the superseded dir
        // unconditionally produced a broken link in by_category (a spurious
        // category segment). Route through the canonical `links::rel_link`.
        let from_dir = self.status_target_dir(&old_path, Status::Superseded);
        let link = crate::links::rel_link(&from_dir, &new_path);
        self.set_status_at(old_path, Status::Superseded, Some((new.clone(), link)))
    }

    /// Core status-change at a known path (shared by the number- and ref-keyed
    /// public entry points). `supersede` carries the superseding ADR's
    /// [`AdrRef`] and the relative link to it, for the `## Status` rewrite.
    fn set_status_at(
        &self,
        path: PathBuf,
        new_status: Status,
        supersede: Option<(AdrRef, String)>,
    ) -> Result<PathBuf, StoreError> {
        match self.opts.format {
            Format::Frontmatter => {
                let mut adr = self.read(&path)?;
                adr.status = new_status;
                if let Some((new, _)) = &supersede {
                    adr.superseded_by = Some(new.clone());
                }
                // Flat / by_category: rewrite in place. by_status: move then write.
                let target_dir = self.status_target_dir(&path, new_status);
                if !target_dir.is_dir() {
                    std::fs::create_dir_all(&target_dir)?;
                }
                let content = format::serialize(&adr, self.opts.format)
                    .map_err(|e| StoreError::Parse(e.to_string()))?;
                let new_path =
                    target_dir.join(path.file_name().map(|n| n.to_owned()).unwrap_or_else(|| {
                        self.opts
                            .naming
                            .filename(&adr.reference(), &adr.title)
                            .into()
                    }));
                std::fs::write(&new_path, content)?;
                if new_path != path {
                    std::fs::remove_file(&path)?;
                    // The file moved dirs — reconcile relative links per `relink_scope`.
                    self.relink_after_move(&new_path)?;
                }
                Ok(new_path)
            }
            Format::Markdown => {
                let original = std::fs::read_to_string(&path)?;
                let label = supersede
                    .as_ref()
                    .map(|(r, _)| self.opts.naming.link_label(r));
                let supersede_ref = supersede
                    .as_ref()
                    .map(|(_, link)| (label.as_deref().unwrap_or(""), link.as_str()));
                let rewritten = format::rewrite_status(&original, new_status, supersede_ref);

                let target_dir = self.status_target_dir(&path, new_status);
                if !target_dir.is_dir() {
                    std::fs::create_dir_all(&target_dir)?;
                }
                let file_name = path
                    .file_name()
                    .map(|n| n.to_owned())
                    .expect("ADR path has a filename");
                let new_path = target_dir.join(&file_name);
                std::fs::write(&new_path, rewritten)?;
                if new_path != path {
                    std::fs::remove_file(&path)?;
                    // The file moved dirs — reconcile relative links per `relink_scope`.
                    self.relink_after_move(&new_path)?;
                }
                Ok(new_path)
            }
        }
    }

    /// Reconcile cross-ADR links after a status-change move, honoring
    /// `relink_scope`: `all` heals every inbound link (single-author default),
    /// `self` fixes only the moved file's own links (a status-change PR then
    /// touches only its own ADR), and `none` defers everything to a later
    /// `adroit relink` on `main`. The explicit `adroit relink` command and
    /// `renumber`/`migrate` are always full-scope and never go through here.
    fn relink_after_move(&self, moved: &Path) -> Result<(), StoreError> {
        match self.opts.relink_scope {
            RelinkScope::All => {
                self.relink(true)?;
            }
            RelinkScope::SelfOnly => {
                self.relink_one(moved)?;
            }
            RelinkScope::None => {}
        }
        Ok(())
    }

    /// Replace ONLY an ADR's markdown body, preserving everything the format
    /// profile owns (frontmatter / `## Status` / banner / status dir).
    ///
    /// This is the single write path for the in-TUI body editor. It mirrors
    /// [`set_status`]/[`supersede`]: read the ADR through the store, mutate one
    /// field (`body`), and re-serialize through the existing
    /// [`format::serialize`] path so the on-disk profile stays consistent. The
    /// status — and therefore the file location — is left untouched. Returns the
    /// path written.
    pub fn set_body(&self, number: Number, new_body: &str) -> Result<PathBuf, StoreError> {
        let path = self.find_path_by_number(number)?;
        self.set_body_at(path, new_body)
    }

    /// Like [`set_body`] but addressed by the scheme's [`AdrRef`].
    pub fn set_body_ref(&self, r: &AdrRef, new_body: &str) -> Result<PathBuf, StoreError> {
        let path = self.find_path_by_ref(r)?;
        self.set_body_at(path, new_body)
    }

    fn set_body_at(&self, path: PathBuf, new_body: &str) -> Result<PathBuf, StoreError> {
        let mut adr = self.read(&path)?;
        adr.body = new_body.to_string();
        let content = format::serialize(&adr, self.opts.format)
            .map_err(|e| StoreError::Parse(e.to_string()))?;
        std::fs::write(&path, content)?;
        Ok(path)
    }

    /// Set (or clear) an ADR's optional `review_by` deadline, format-preserving.
    ///
    /// Mirrors [`set_body`]/[`set_status`]: in the frontmatter profile the field
    /// is updated through the `Adr` model and re-serialized; in the markdown
    /// profile only the `Review by:` line in the `## Status` region is rewritten
    /// (or inserted/removed), leaving every other byte untouched. Passing `None`
    /// clears it. Returns the path written (the file does not move).
    pub fn set_review_by(
        &self,
        number: Number,
        review_by: Option<crate::adr::ReviewBy>,
    ) -> Result<PathBuf, StoreError> {
        let path = self.find_path_by_number(number)?;
        self.set_review_by_at(path, review_by)
    }

    /// Like [`set_review_by`] but addressed by the scheme's [`AdrRef`].
    pub fn set_review_by_ref(
        &self,
        r: &AdrRef,
        review_by: Option<crate::adr::ReviewBy>,
    ) -> Result<PathBuf, StoreError> {
        let path = self.find_path_by_ref(r)?;
        self.set_review_by_at(path, review_by)
    }

    /// Add or remove a typed relational link on the ADR addressed by `source`.
    /// A frontmatter-profile feature (the links are structured fields); errors
    /// under the markdown profile. Adding validates that `target` exists.
    pub fn set_links_ref(
        &self,
        source: &AdrRef,
        kind: LinkKind,
        target: &AdrRef,
        remove: bool,
    ) -> Result<PathBuf, StoreError> {
        if self.opts.format != Format::Frontmatter {
            return Err(StoreError::Parse(
                "typed links require the frontmatter format (run `adroit migrate --format \
                 frontmatter`)"
                    .into(),
            ));
        }
        if !remove {
            self.find_path_by_ref(target)?; // refuse linking to a missing ADR
        }
        let path = self.find_path_by_ref(source)?;
        let mut adr = self.read(&path)?;
        let links = match kind {
            LinkKind::RelatesTo => &mut adr.relates_to,
            LinkKind::DependsOn => &mut adr.depends_on,
            LinkKind::Refines => &mut adr.refines,
        };
        if remove {
            links.retain(|r| r != target);
        } else if !links.contains(target) {
            links.push(target.clone());
        }
        let content = format::serialize(&adr, self.opts.format)
            .map_err(|e| StoreError::Parse(e.to_string()))?;
        std::fs::write(&path, content)?;
        Ok(path)
    }

    fn set_review_by_at(
        &self,
        path: PathBuf,
        review_by: Option<crate::adr::ReviewBy>,
    ) -> Result<PathBuf, StoreError> {
        match self.opts.format {
            Format::Frontmatter => {
                let mut adr = self.read(&path)?;
                adr.review_by = review_by;
                let content = format::serialize(&adr, self.opts.format)
                    .map_err(|e| StoreError::Parse(e.to_string()))?;
                std::fs::write(&path, content)?;
            }
            Format::Markdown => {
                let original = std::fs::read_to_string(&path)?;
                let rewritten = format::rewrite_review_by(&original, review_by);
                std::fs::write(&path, rewritten)?;
            }
        }
        Ok(path)
    }
}

/// Immediate subdirectories of `dir` (one level), sorted by name. Used by the
/// `by_category` layout, where each subdirectory is a category.
fn immediate_subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// Read `*.md` ADR files (excluding README.md / adr-template.md) from a dir.
fn read_md_files(dir: &Path) -> Result<Vec<PathBuf>, StoreError> {
    Ok(std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| is_adr_file(p))
        .collect())
}

/// ADR files (`NNNN-*.md`, excluding README/template) directly in `dir` — i.e.
/// only those whose name carries a leading number, so stray `.md` notes don't
/// trip layout detection.
fn adr_files_in(dir: &Path) -> Vec<PathBuf> {
    read_md_files(dir)
        .unwrap_or_default()
        .into_iter()
        .filter(|p| number_from_filename(p).is_some())
        .collect()
}

/// `path` relative to `root` (for display), or `path` itself if not under it.
fn rel_to(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

/// Apply a scheme-assigned [`AdrRef`] to an ADR's identity (public wrapper for
/// callers that assign the ref before `write`, e.g. to render the heading).
pub fn apply_ref_pub(adr: &mut Adr, r: &AdrRef) {
    apply_ref(adr, r.clone());
}

/// Apply a scheme-assigned [`AdrRef`] to an ADR's identity fields.
fn apply_ref(adr: &mut Adr, r: AdrRef) {
    match r {
        AdrRef::Number(n) => {
            adr.number = Some(Number::new(n));
            adr.slug = None;
        }
        AdrRef::Slug(s) => {
            adr.slug = Some(s);
            adr.number = None;
        }
    }
}

/// Today's local date (UTC fallback), for the date naming scheme.
fn today_local() -> time::Date {
    if let Some(d) = crate::config::today_override() {
        return d;
    }
    time::OffsetDateTime::now_local()
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
        .date()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adr::Adr;

    fn flat() -> StoreOptions {
        StoreOptions::flat_frontmatter()
    }

    #[test]
    fn filename_format() {
        assert_eq!(
            filename(Number::new(1), "Use PostgreSQL for primary datastore"),
            "0001-use-postgresql-for-primary-datastore.md"
        );
    }

    #[test]
    fn filename_zero_pads() {
        assert_eq!(filename(Number::new(42), "Something"), "0042-something.md");
    }

    #[test]
    fn filename_strips_punctuation() {
        assert_eq!(
            filename(Number::new(3), "GraphQL vs. REST"),
            "0003-graphql-vs-rest.md"
        );
    }

    #[test]
    fn open_or_create_creates_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let adr_dir = tmp.path().join("adr");
        let store = Store::open_or_create_with(&adr_dir, flat()).unwrap();
        assert!(store.root().is_dir());
    }

    #[test]
    fn open_missing_directory_errors() {
        let result = Store::open("/tmp/adroit-does-not-exist");
        assert!(result.is_err());
    }

    #[test]
    fn write_and_list_round_trip_flat() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_or_create_with(tmp.path().join("adr"), flat()).unwrap();

        let mut adr = Adr::new("Use PostgreSQL").unwrap();
        store.write(&mut adr).unwrap();

        let files = store.list_files().unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("0001-use-postgresql.md"));
    }

    #[test]
    fn write_assigns_number_lazily() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_or_create_with(tmp.path().join("adr"), flat()).unwrap();

        let mut adr = Adr::new("Lazy numbering").unwrap();
        assert!(adr.number.is_none());

        store.write(&mut adr).unwrap();
        assert_eq!(adr.number, Some(Number::new(1)));
    }

    #[test]
    fn write_produces_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_or_create_with(tmp.path().join("adr"), flat()).unwrap();

        let mut adr = Adr::new("Use PostgreSQL").unwrap();
        let path = store.write(&mut adr).unwrap();
        let content = std::fs::read_to_string(path).unwrap();

        assert!(content.starts_with("---\n"));
        assert!(content.contains("id:"));
        assert!(content.contains("status: Proposed"));
    }

    #[test]
    fn write_then_read_round_trip_flat() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_or_create_with(tmp.path().join("adr"), flat()).unwrap();

        let mut adr = Adr::new("Use PostgreSQL").unwrap();
        let path = store.write(&mut adr).unwrap();
        let parsed = store.read(&path).unwrap();

        assert_eq!(parsed.id, adr.id);
        assert_eq!(parsed.number, adr.number);
        assert_eq!(parsed.title, adr.title);
        assert_eq!(parsed.status, adr.status);
        assert_eq!(parsed.created, adr.created);
    }

    #[test]
    fn next_number_starts_at_one() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_or_create_with(tmp.path().join("adr"), flat()).unwrap();
        assert_eq!(store.next_number().unwrap(), Number::new(1));
    }

    #[test]
    fn next_number_increments_flat() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_or_create_with(tmp.path().join("adr"), flat()).unwrap();
        store.write(&mut Adr::new("First").unwrap()).unwrap();
        store.write(&mut Adr::new("Second").unwrap()).unwrap();
        assert_eq!(store.next_number().unwrap(), Number::new(3));
    }

    #[test]
    fn find_path_by_number_found() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_or_create_with(tmp.path().join("adr"), flat()).unwrap();
        store.write(&mut Adr::new("First").unwrap()).unwrap();
        store.write(&mut Adr::new("Second").unwrap()).unwrap();

        let path = store.find_path_by_number(Number::new(2)).unwrap();
        assert!(path.ends_with("0002-second.md"));
    }

    #[test]
    fn find_path_by_number_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_or_create_with(tmp.path().join("adr"), flat()).unwrap();
        assert!(store.find_path_by_number(Number::new(99)).is_err());
    }

    #[test]
    fn list_returns_parsed_adrs_flat() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_or_create_with(tmp.path().join("adr"), flat()).unwrap();
        store.write(&mut Adr::new("First").unwrap()).unwrap();
        store.write(&mut Adr::new("Second").unwrap()).unwrap();

        let adrs = store.list().unwrap();
        assert_eq!(adrs.len(), 2);
        assert_eq!(adrs[0].title, "First");
        assert_eq!(adrs[1].title, "Second");
    }

    // ---- by_status markdown layout ----

    fn md_store(root: &Path) -> Store {
        Store::open_or_create_with(root, StoreOptions::default()).unwrap()
    }

    fn write_md(store: &Store, status: Status, number: u32, title: &str, body: &str) -> PathBuf {
        let dir = store.status_dir(status);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(filename(Number::new(number), title));
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn by_status_lists_across_dirs_skipping_readme() {
        let tmp = tempfile::tempdir().unwrap();
        let store = md_store(tmp.path());
        write_md(
            &store,
            Status::Accepted,
            6,
            "Adopt ADRs",
            "# ADR-0006: Adopt ADRs\n\n## Status\n\nAccepted\n",
        );
        write_md(
            &store,
            Status::Proposed,
            11,
            "Repo Strategy",
            "# ADR-0011: Repo Strategy\n\n## Status\n\nProposed\n",
        );
        // README in a status dir must be ignored.
        std::fs::write(store.status_dir(Status::Proposed).join("README.md"), "# x").unwrap();

        let adrs = store.list().unwrap();
        assert_eq!(adrs.len(), 2);
        assert_eq!(store.next_number().unwrap(), Number::new(12));
    }

    #[test]
    fn by_status_number_collision_across_dirs_is_graceful() {
        let tmp = tempfile::tempdir().unwrap();
        let store = md_store(tmp.path());
        write_md(
            &store,
            Status::Accepted,
            9,
            "Thing",
            "# ADR-0009: Thing\n\n## Status\n\nAccepted\n",
        );
        write_md(
            &store,
            Status::Proposed,
            9,
            "Other",
            "# ADR-0009: Other\n\n## Status\n\nProposed\n",
        );
        // Both parse; no panic. next_number is 10.
        let adrs = store.list().unwrap();
        assert_eq!(adrs.len(), 2);
        assert_eq!(store.next_number().unwrap(), Number::new(10));
    }

    #[test]
    fn set_status_moves_and_rewrites_markdown() {
        let tmp = tempfile::tempdir().unwrap();
        let store = md_store(tmp.path());
        let original = "# ADR-0006: Adopt ADRs\n\n> State: Proposed\n\n## Status\n\nProposed\n\n## Context\n\nBody.\n";
        write_md(&store, Status::Proposed, 6, "Adopt ADRs", original);

        let new_path = store.set_status(Number::new(6), Status::Accepted).unwrap();
        assert!(new_path.starts_with(store.status_dir(Status::Accepted)));
        assert!(
            !store
                .status_dir(Status::Proposed)
                .join("0006-adopt-adrs.md")
                .exists()
        );

        let content = std::fs::read_to_string(&new_path).unwrap();
        assert!(content.contains("> State: Accepted"));
        assert!(content.contains("\n## Status\n\nAccepted\n"));
        assert!(content.contains("Body."));
    }

    #[test]
    fn supersede_moves_old_and_writes_link() {
        let tmp = tempfile::tempdir().unwrap();
        let store = md_store(tmp.path());
        write_md(
            &store,
            Status::Accepted,
            6,
            "New Decision",
            "# ADR-0006: New Decision\n\n## Status\n\nAccepted\n",
        );
        write_md(
            &store,
            Status::Accepted,
            2,
            "Old Decision",
            "# ADR-0002: Old Decision\n\n> State: Accepted\n\n## Status\n\nAccepted\n",
        );

        let new_path = store
            .supersede(&AdrRef::Number(6), &AdrRef::Number(2))
            .unwrap();
        assert!(new_path.starts_with(store.status_dir(Status::Superseded)));
        let content = std::fs::read_to_string(&new_path).unwrap();
        assert!(content.contains("Superseded by [ADR-0006](../accepted/0006-new-decision.md)"));
        assert!(content.contains("> State: Superseded"));
    }

    #[test]
    fn set_body_rewrites_only_the_body_preserving_structure() {
        let tmp = tempfile::tempdir().unwrap();
        let store = md_store(tmp.path());
        // A real-shaped MADR document: H1 + banner + `## Status` + body sections.
        let original = "# ADR-0006: Adopt ADRs as Team Decision Process\n\
\n\
> State: Accepted\n\
\n\
## Status\n\
\n\
Accepted\n\
\n\
## Context and Problem Statement\n\
\n\
We need a consistent way.\n";
        let path = write_md(
            &store,
            Status::Accepted,
            6,
            "Adopt ADRs as Team Decision Process",
            original,
        );

        // The editor loads the whole markdown document as the body. Edit only
        // the Context prose; the header/banner/status lines are untouched.
        let edited = original.replace(
            "We need a consistent way.",
            "We need a consistent way.\n\nAnd now an extra paragraph.",
        );
        let written = store.set_body(Number::new(6), &edited).unwrap();
        assert_eq!(written, path); // status unchanged -> same file location

        let after = std::fs::read_to_string(&written).unwrap();
        // The new prose is present.
        assert!(after.contains("And now an extra paragraph."));
        // The format-owned lines are byte-identical to the original.
        for line in [
            "# ADR-0006: Adopt ADRs as Team Decision Process",
            "> State: Accepted",
            "## Status",
        ] {
            assert!(
                after.lines().any(|l| l == line),
                "line `{line}` must survive unchanged"
            );
        }
        // The `## Status` value line is still exactly "Accepted".
        assert!(after.contains("\n## Status\n\nAccepted\n"));
        // Single trailing newline preserved.
        assert!(after.ends_with('\n'));
        assert!(!after.ends_with("\n\n"));
    }

    #[test]
    fn set_body_unchanged_is_byte_identical() {
        let tmp = tempfile::tempdir().unwrap();
        let store = md_store(tmp.path());
        let original = "# ADR-0006: Adopt ADRs\n\n> State: Accepted\n\n## Status\n\nAccepted\n\n## Context\n\nBody.\n";
        let path = write_md(&store, Status::Accepted, 6, "Adopt ADRs", original);

        // Loading via the store and saving the same body must not change a byte.
        let adr = store.read(&path).unwrap();
        let written = store.set_body(Number::new(6), &adr.body).unwrap();
        let after = std::fs::read_to_string(&written).unwrap();
        assert_eq!(after, original);
    }

    #[test]
    fn set_review_by_markdown_inserts_and_clears_preserving_structure() {
        use crate::adr::ReviewBy;
        let tmp = tempfile::tempdir().unwrap();
        let store = md_store(tmp.path());
        let original = "# ADR-0006: Adopt ADRs\n\n> State: Proposed\n\n## Status\n\nProposed\n\n## Context\n\nBody.\n";
        let path = write_md(&store, Status::Proposed, 6, "Adopt ADRs", original);

        let rb: ReviewBy = "2026-07-01".parse().unwrap();
        let written = store.set_review_by(Number::new(6), Some(rb)).unwrap();
        assert_eq!(written, path); // status unchanged -> same location
        let after = std::fs::read_to_string(&written).unwrap();
        assert!(after.contains("Review by: 2026-07-01"));
        assert!(after.contains("## Context"));

        // Reading it back through the store surfaces the date.
        let adr = store.read(&written).unwrap();
        assert_eq!(adr.review_by, Some(rb));

        // Clearing removes the line and restores the original bytes.
        store.set_review_by(Number::new(6), None).unwrap();
        let cleared = std::fs::read_to_string(&written).unwrap();
        assert_eq!(cleared, original);
    }

    #[test]
    fn set_review_by_frontmatter_round_trips() {
        use crate::adr::ReviewBy;
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_or_create_with(tmp.path().join("adr"), flat()).unwrap();
        store
            .write(&mut Adr::new("Use PostgreSQL").unwrap())
            .unwrap();

        let rb: ReviewBy = "2026-09-09".parse().unwrap();
        store.set_review_by(Number::new(1), Some(rb)).unwrap();
        let path = store.find_path_by_number(Number::new(1)).unwrap();
        let adr = store.read(&path).unwrap();
        assert_eq!(adr.review_by, Some(rb));
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("review_by: 2026-09-09"));
    }

    #[test]
    fn round_trip_real_unchanged_markdown_is_byte_identical() {
        let tmp = tempfile::tempdir().unwrap();
        let store = md_store(tmp.path());
        let original = "# ADR-0006: Adopt ADRs as Team Decision Process\n\n> State: Accepted\n\n## Status\n\nAccepted\n\n## Context and Problem Statement\n\nWe need a consistent way.\n";
        let path = write_md(
            &store,
            Status::Accepted,
            6,
            "Adopt ADRs as Team Decision Process",
            original,
        );

        // Re-applying the same status must not change a single byte.
        let new_path = store.set_status(Number::new(6), Status::Accepted).unwrap();
        assert_eq!(new_path, path);
        let after = std::fs::read_to_string(&new_path).unwrap();
        assert_eq!(after, original);
    }

    // --- profile detection / mismatch / migrate ---

    fn flat_store(root: &Path) -> Store {
        Store::open_or_create_with(
            root,
            StoreOptions {
                layout: Layout::Flat,
                ..StoreOptions::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn detect_profile_reads_by_status_markdown() {
        let tmp = tempfile::tempdir().unwrap();
        let store = md_store(tmp.path());
        write_md(
            &store,
            Status::Proposed,
            1,
            "X",
            "# ADR-0001: X\n\n## Status\n\nProposed\n",
        );
        let d = store.detect_profile();
        assert_eq!(d.layout, Some(Layout::ByStatus));
        assert_eq!(d.format, Some(Format::Markdown));
        assert!(store.profile_mismatch().is_none());
    }

    #[test]
    fn detect_profile_empty_dir_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let store = md_store(tmp.path());
        let d = store.detect_profile();
        assert!(d.layout.is_none() && d.format.is_none());
        assert!(store.profile_mismatch().is_none());
    }

    #[test]
    fn profile_mismatch_flags_flat_config_on_by_status_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let by_status = md_store(tmp.path());
        write_md(
            &by_status,
            Status::Accepted,
            1,
            "X",
            "# ADR-0001: X\n\n## Status\n\nAccepted\n",
        );
        let msg = flat_store(tmp.path()).profile_mismatch().unwrap();
        assert!(
            msg.contains("by_status") && msg.contains("flat"),
            "got: {msg}"
        );
    }

    #[test]
    fn migrate_by_status_to_flat_moves_files_and_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let by_status = md_store(tmp.path());
        write_md(
            &by_status,
            Status::Proposed,
            1,
            "One",
            "# ADR-0001: One\n\n## Status\n\nProposed\n",
        );
        write_md(
            &by_status,
            Status::Accepted,
            2,
            "Two",
            "# ADR-0002: Two\n\n## Status\n\nAccepted\n",
        );

        let flat = flat_store(tmp.path());
        let report = flat.migrate(true).unwrap();
        assert!(report.applied);
        assert_eq!(report.layout_change, Some((Layout::ByStatus, Layout::Flat)));
        assert!(tmp.path().join("0001-one.md").exists());
        assert!(tmp.path().join("0002-two.md").exists());
        assert!(!tmp.path().join("proposed/0001-one.md").exists());
        // Now that the repo matches, a second run is a no-op.
        assert!(flat.migrate(false).unwrap().is_noop());
    }

    #[test]
    fn migrate_filename_collision_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let by_status = md_store(tmp.path());
        // Two ADRs share a filename in different status dirs -> one flat path.
        write_md(
            &by_status,
            Status::Proposed,
            9,
            "Dup",
            "# ADR-0009: Dup\n\n## Status\n\nProposed\n",
        );
        write_md(
            &by_status,
            Status::Accepted,
            9,
            "Dup",
            "# ADR-0009: Dup\n\n## Status\n\nAccepted\n",
        );
        assert!(
            flat_store(tmp.path()).migrate(false).is_err(),
            "two 0009-dup.md must refuse to collide into one flat path"
        );
    }

    #[test]
    fn relink_dry_run_reports_without_writing() {
        let tmp = tempfile::tempdir().unwrap();
        let store = md_store(tmp.path());
        let p1 = write_md(
            &store,
            Status::Proposed,
            1,
            "A",
            "# ADR-0001: A\n\n## Status\n\nProposed\n\nSee [ADR-0002](../proposed/0002-b.md).\n",
        );
        write_md(
            &store,
            Status::Accepted,
            2,
            "B",
            "# ADR-0002: B\n\n## Status\n\nAccepted\n",
        );
        let before = std::fs::read_to_string(&p1).unwrap();
        let r = store.relink(false).unwrap();
        assert_eq!(r.files_changed, 1);
        assert_eq!(
            std::fs::read_to_string(&p1).unwrap(),
            before,
            "dry run must not write"
        );
        store.relink(true).unwrap();
        assert!(
            std::fs::read_to_string(&p1)
                .unwrap()
                .contains("../accepted/0002-b.md")
        );
    }
}
