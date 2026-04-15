use std::path::{Path, PathBuf};

use crate::adr::{Adr, Number};

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

/// Generate the canonical filename for an ADR: `0001-some-title.md`
fn filename(number: Number, title: &str) -> String {
    let slug: String = title
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-");
    format!("{number}-{slug}.md")
}

/// Manages reading and writing ADRs on disk.
#[derive(Debug)]
pub struct Store {
    /// Root directory containing the ADR files (e.g. `~/.local/share/adroit/`).
    root: PathBuf,
}

impl Store {
    /// Open an existing ADR store at the given path.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        if !root.is_dir() {
            return Err(StoreError::NotFound(root));
        }
        Ok(Self { root })
    }

    /// Open an ADR store, creating the directory if it doesn't exist.
    pub fn open_or_create(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        if !root.is_dir() {
            std::fs::create_dir_all(&root)?;
        }
        Ok(Self { root })
    }

    /// Return the root path of this store.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// List all ADR files in the store directory, sorted by name.
    pub fn list_files(&self) -> Result<Vec<PathBuf>, StoreError> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&self.root)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "md"))
            .collect();
        files.sort();
        Ok(files)
    }

    /// Return the next available ADR number.
    pub fn next_number(&self) -> Result<Number, StoreError> {
        let files = self.list_files()?;
        let max = files
            .iter()
            .filter_map(|p| {
                p.file_name()?
                    .to_str()?
                    .split('-')
                    .next()?
                    .parse::<u32>()
                    .ok()
            })
            .max()
            .unwrap_or(0);
        Ok(Number::new(max + 1))
    }

    /// Write an ADR to disk using its canonical filename.
    ///
    /// If the ADR's `number` is `None`, assigns the next available number.
    pub fn write(&self, adr: &mut Adr) -> Result<PathBuf, StoreError> {
        if adr.number.is_none() {
            adr.number = Some(self.next_number()?);
        }
        let number = adr.number.expect("number was just assigned above");
        let content =
            crate::frontmatter::serialize(adr).map_err(|e| StoreError::Parse(e.to_string()))?;
        let path = self.root.join(filename(number, &adr.title));
        std::fs::write(&path, content)?;
        Ok(path)
    }

    /// Read a single ADR from a file path.
    pub fn read(&self, path: &Path) -> Result<Adr, StoreError> {
        let content = std::fs::read_to_string(path)?;
        crate::frontmatter::deserialize(&content).map_err(|e| StoreError::Parse(e.to_string()))
    }

    /// Find the file path for an ADR by its sequential number.
    pub fn find_path_by_number(&self, number: Number) -> Result<PathBuf, StoreError> {
        let prefix = format!("{number}-");
        self.list_files()?
            .into_iter()
            .find(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&prefix))
            })
            .ok_or(StoreError::NumberNotFound(number))
    }

    /// List all ADRs in the store, parsed from disk.
    pub fn list(&self) -> Result<Vec<Adr>, StoreError> {
        let files = self.list_files()?;
        files.iter().map(|p| self.read(p)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adr::Adr;

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
    fn open_or_create_creates_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let adr_dir = tmp.path().join("adr");
        let store = Store::open_or_create(&adr_dir).unwrap();
        assert!(store.root().is_dir());
    }

    #[test]
    fn open_missing_directory_errors() {
        let result = Store::open("/tmp/adroit-does-not-exist");
        assert!(result.is_err());
    }

    #[test]
    fn write_and_list_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_or_create(tmp.path().join("adr")).unwrap();

        let mut adr = Adr::new("Use PostgreSQL").unwrap();
        store.write(&mut adr).unwrap();

        let files = store.list_files().unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("0001-use-postgresql.md"));
    }

    #[test]
    fn write_assigns_number_lazily() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_or_create(tmp.path().join("adr")).unwrap();

        let mut adr = Adr::new("Lazy numbering").unwrap();
        assert!(adr.number.is_none());

        store.write(&mut adr).unwrap();
        assert_eq!(adr.number, Some(Number::new(1)));
    }

    #[test]
    fn write_produces_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_or_create(tmp.path().join("adr")).unwrap();

        let mut adr = Adr::new("Use PostgreSQL").unwrap();
        let path = store.write(&mut adr).unwrap();
        let content = std::fs::read_to_string(path).unwrap();

        assert!(content.starts_with("---\n"));
        assert!(content.contains("id:"));
        assert!(content.contains("status: Proposed"));
    }

    #[test]
    fn write_then_read_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_or_create(tmp.path().join("adr")).unwrap();

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
        let store = Store::open_or_create(tmp.path().join("adr")).unwrap();
        assert_eq!(store.next_number().unwrap(), Number::new(1));
    }

    #[test]
    fn next_number_increments() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_or_create(tmp.path().join("adr")).unwrap();
        store.write(&mut Adr::new("First").unwrap()).unwrap();
        store.write(&mut Adr::new("Second").unwrap()).unwrap();
        assert_eq!(store.next_number().unwrap(), Number::new(3));
    }

    #[test]
    fn find_path_by_number_found() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_or_create(tmp.path().join("adr")).unwrap();
        store.write(&mut Adr::new("First").unwrap()).unwrap();
        store.write(&mut Adr::new("Second").unwrap()).unwrap();

        let path = store.find_path_by_number(Number::new(2)).unwrap();
        assert!(path.ends_with("0002-second.md"));
    }

    #[test]
    fn find_path_by_number_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_or_create(tmp.path().join("adr")).unwrap();
        assert!(store.find_path_by_number(Number::new(99)).is_err());
    }

    #[test]
    fn list_returns_parsed_adrs() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_or_create(tmp.path().join("adr")).unwrap();
        store.write(&mut Adr::new("First").unwrap()).unwrap();
        store.write(&mut Adr::new("Second").unwrap()).unwrap();

        let adrs = store.list().unwrap();
        assert_eq!(adrs.len(), 2);
        assert_eq!(adrs[0].title, "First");
        assert_eq!(adrs[1].title, "Second");
    }
}
