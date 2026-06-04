//! `adroit publish`: export the **accepted** ADR set to an output directory — a
//! static-dir publisher (sibling to `index`, but for the published-docs side).
//! Confluence / Notion / Hugo / Docusaurus adapters are future work; this
//! always-compiled core needs no network. Idempotent: re-running overwrites the
//! same files and rewrites `index.md`.

use std::path::{Path, PathBuf};

use crate::adr::Status;
use crate::query::{self, Filter};
use crate::store::Store;

/// Outcome of a [`publish`] run.
#[derive(Debug, Default)]
pub struct PublishReport {
    /// Number of ADR files written (or that would be, on a dry run).
    pub written: usize,
    /// `(title, filename)` of each published ADR, in list order.
    pub files: Vec<(String, String)>,
    /// The output directory.
    pub out: PathBuf,
}

/// Copy every accepted ADR to `out/` and (re)generate an `index.md`. With
/// `apply == false` nothing is written — the report describes what would happen.
pub fn publish(store: &Store, out: &Path, apply: bool) -> anyhow::Result<PublishReport> {
    let rows = query::summaries(
        store,
        &Filter {
            status: Some(Status::Accepted),
            ..Default::default()
        },
    )?;
    let naming = store.options().naming;

    let mut report = PublishReport {
        out: out.to_path_buf(),
        ..Default::default()
    };
    if apply {
        std::fs::create_dir_all(out)?;
    }
    let mut index = String::from("# Decision log\n\nPublished accepted ADRs.\n\n");
    for row in &rows {
        let Some(r) = naming.parse_ref(&row.address) else {
            continue;
        };
        let path = store.find_path_by_ref(&r)?;
        let file = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("adr.md")
            .to_string();
        index.push_str(&format!("- [{}: {}]({file})\n", row.reference, row.title));
        if apply {
            let content = std::fs::read_to_string(&path)?;
            std::fs::write(out.join(&file), content)?;
        }
        report.written += 1;
        report.files.push((row.title.clone(), file));
    }
    if apply {
        std::fs::write(out.join("index.md"), index)?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::StoreOptions;

    #[test]
    fn publishes_accepted_adrs_and_index() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("adrs");
        let store = Store::open_or_create_with(&root, StoreOptions::default()).unwrap();

        // One accepted, one proposed; only the accepted should be published.
        std::fs::create_dir_all(root.join("accepted")).unwrap();
        std::fs::create_dir_all(root.join("proposed")).unwrap();
        std::fs::write(
            root.join("accepted/0001-use-pg.md"),
            "# ADR-0001: Use PG\n\n## Status\n\nAccepted\n",
        )
        .unwrap();
        std::fs::write(
            root.join("proposed/0002-maybe.md"),
            "# ADR-0002: Maybe\n\n## Status\n\nProposed\n",
        )
        .unwrap();

        let out = tmp.path().join("site");
        let report = publish(&store, &out, true).unwrap();
        assert_eq!(report.written, 1);
        assert!(out.join("0001-use-pg.md").exists());
        assert!(!out.join("0002-maybe.md").exists());
        let index = std::fs::read_to_string(out.join("index.md")).unwrap();
        assert!(index.contains("Use PG"));
        assert!(!index.contains("Maybe"));
    }

    #[test]
    fn dry_run_writes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("adrs");
        let store = Store::open_or_create_with(&root, StoreOptions::default()).unwrap();
        std::fs::create_dir_all(root.join("accepted")).unwrap();
        std::fs::write(
            root.join("accepted/0001-x.md"),
            "# ADR-0001: X\n\n## Status\n\nAccepted\n",
        )
        .unwrap();
        let out = tmp.path().join("site");
        let report = publish(&store, &out, false).unwrap();
        assert_eq!(report.written, 1);
        assert!(!out.exists());
    }
}
