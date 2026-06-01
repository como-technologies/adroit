use std::path::{Path, PathBuf};

use crate::adr::{Adr, Number, Status};
use crate::config::{DateSource, Layout};
use crate::format::{self, Format};

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
#[derive(Debug, Clone, Copy, Default)]
pub struct RelinkReport {
    /// Number of files whose content was rewritten.
    pub files_changed: usize,
    /// Total cross-ADR links rewritten across those files.
    pub links_rewritten: usize,
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
        }
    }

    fn dir_name(&self, status: Status) -> String {
        self.status_dir
            .get(&status)
            .cloned()
            .unwrap_or_else(|| status.to_string().to_lowercase())
    }
}

/// Generate the canonical filename for an ADR: `0001-some-title.md`
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
    pub fn status_dir(&self, status: Status) -> PathBuf {
        match self.opts.layout {
            Layout::Flat => self.root.clone(),
            Layout::ByStatus => self.root.join(self.opts.dir_name(status)),
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
        if self.opts.layout == Layout::Flat {
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

    /// Write an ADR to disk using its canonical filename and the configured
    /// format/layout. Assigns a number if the ADR doesn't have one.
    pub fn write(&self, adr: &mut Adr) -> Result<PathBuf, StoreError> {
        if adr.number.is_none() {
            adr.number = Some(self.next_number()?);
        }
        let number = adr.number.expect("number was just assigned above");
        let content = format::serialize(adr, self.opts.format)
            .map_err(|e| StoreError::Parse(e.to_string()))?;
        let dir = self.status_dir(adr.status);
        if !dir.is_dir() {
            std::fs::create_dir_all(&dir)?;
        }
        let path = dir.join(filename(number, &adr.title));
        std::fs::write(&path, content)?;
        Ok(path)
    }

    /// Read a single ADR from a file path.
    pub fn read(&self, path: &Path) -> Result<Adr, StoreError> {
        let content = std::fs::read_to_string(path)?;
        let dir_status = self.dir_status_inner(path);
        format::deserialize(&content, self.opts.format, dir_status)
            .map_err(|e| StoreError::Parse(e.to_string()))
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
    pub fn relink(&self) -> Result<RelinkReport, StoreError> {
        let entries = self.list_with_paths()?;
        // Count numbers so duplicates can be skipped (can't disambiguate them).
        let mut seen: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
        for (_, adr) in &entries {
            if let Some(n) = adr.number {
                *seen.entry(n.get()).or_default() += 1;
            }
        }
        let mut by_number: std::collections::HashMap<u32, PathBuf> =
            std::collections::HashMap::new();
        for (path, adr) in &entries {
            if let Some(n) = adr.number
                && seen.get(&n.get()) == Some(&1)
            {
                by_number.insert(n.get(), path.clone());
            }
        }

        let mut report = RelinkReport::default();
        for (path, _) in &entries {
            let dir = path.parent().unwrap_or_else(|| Path::new(""));
            let original = std::fs::read_to_string(path)?;
            let (rewritten, changed) =
                crate::links::rewrite_links(&original, dir, |n| by_number.get(&n).cloned());
            if changed > 0 && rewritten != original {
                std::fs::write(path, &rewritten)?;
                report.files_changed += 1;
                report.links_rewritten += changed;
            }
        }
        Ok(report)
    }

    /// Change an ADR's status. In `by_status` markdown mode this MOVES the file
    /// to the matching status dir and rewrites the `## Status` section
    /// (minimal-diff). Returns the new path.
    pub fn set_status(&self, number: Number, new_status: Status) -> Result<PathBuf, StoreError> {
        self.set_status_inner(number, new_status, None)
    }

    /// Mark `old` as superseded by `new`, moving it to the superseded dir and
    /// writing a `Superseded by [ADR-<new>](...)` status with a relative link.
    pub fn supersede(&self, new: Number, old: Number) -> Result<PathBuf, StoreError> {
        // Validate the new ADR exists before mutating the old one.
        self.find_path_by_number(new)?;
        let link = self.relative_link(new)?;
        self.set_status_inner(old, Status::Superseded, Some((new, link)))
    }

    fn set_status_inner(
        &self,
        number: Number,
        new_status: Status,
        supersede: Option<(Number, String)>,
    ) -> Result<PathBuf, StoreError> {
        let path = self.find_path_by_number(number)?;

        match self.opts.format {
            Format::Frontmatter => {
                let mut adr = self.read(&path)?;
                adr.status = new_status;
                if let Some((new, _)) = supersede {
                    adr.superseded_by = Some(new);
                }
                // Flat layout: rewrite in place. by_status: move then write.
                let target_dir = self.status_dir(new_status);
                if !target_dir.is_dir() {
                    std::fs::create_dir_all(&target_dir)?;
                }
                let content = format::serialize(&adr, self.opts.format)
                    .map_err(|e| StoreError::Parse(e.to_string()))?;
                let new_path = target_dir.join(
                    path.file_name()
                        .map(|n| n.to_owned())
                        .unwrap_or_else(|| filename(number, &adr.title).into()),
                );
                std::fs::write(&new_path, content)?;
                if new_path != path {
                    std::fs::remove_file(&path)?;
                    // The file moved dirs — fix every relative link to/from it.
                    self.relink()?;
                }
                Ok(new_path)
            }
            Format::Markdown => {
                let original = std::fs::read_to_string(&path)?;
                let supersede_ref = supersede.as_ref().map(|(n, link)| (*n, link.as_str()));
                let rewritten = format::rewrite_status(&original, new_status, supersede_ref);

                let target_dir = self.status_dir(new_status);
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
                    // The file moved dirs — fix every relative link to/from it.
                    self.relink()?;
                }
                Ok(new_path)
            }
        }
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

    /// Compute the relative markdown link from the superseded dir (where the
    /// old ADR will live) to `to`'s current file.
    fn relative_link(&self, to: Number) -> Result<String, StoreError> {
        let to_path = self.find_path_by_number(to)?;
        let from_dir = self.status_dir(Status::Superseded);
        Ok(pathdiff(&from_dir, &to_path))
    }
}

/// Read `*.md` ADR files (excluding README.md / adr-template.md) from a dir.
fn read_md_files(dir: &Path) -> Result<Vec<PathBuf>, StoreError> {
    Ok(std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| is_adr_file(p))
        .collect())
}

/// Compute a relative path from a directory to a target file using `../`.
/// Both inputs should share the same ancestor `root`.
fn pathdiff(from_dir: &Path, to_file: &Path) -> String {
    let from: Vec<_> = from_dir.components().collect();
    let to: Vec<_> = to_file.components().collect();
    let mut i = 0;
    while i < from.len() && i < to.len() && from[i] == to[i] {
        i += 1;
    }
    let ups = from.len() - i;
    let mut parts: Vec<String> = std::iter::repeat_n("..".to_string(), ups).collect();
    for c in &to[i..] {
        parts.push(c.as_os_str().to_string_lossy().into_owned());
    }
    parts.join("/")
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

        let new_path = store.supersede(Number::new(6), Number::new(2)).unwrap();
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
}
