//! Regenerate the ADR section of an mdBook `SUMMARY.md`.
//!
//! The SUMMARY groups ADRs by status under `## <Status>` headings, e.g.
//!
//! ```text
//! ## Accepted
//!
//! - [ADR-0006: Use PostgreSQL](./adrs/accepted/0006-use-postgresql.md)
//! ```
//!
//! Only the status-grouped block is regenerated. Everything before the first
//! managed `## <Status>` heading (the `# Summary`, introduction links, and the
//! `# Architecture Decision Records` / `[ADR Process]` lines) is preserved
//! verbatim, as is any trailing content after the managed block.

use std::path::Path;

use crate::adr::Status;
use crate::store::Store;

/// An ADR entry rendered into the index.
struct Entry {
    number: crate::adr::Number,
    title: String,
    /// Path relative to the SUMMARY.md location, e.g. `./adrs/accepted/0006-x.md`.
    link: String,
}

/// Build the status-grouped markdown block for the given ADRs.
///
/// `link_prefix` is prepended to each ADR's path relative to the store root,
/// e.g. `./adrs` so links become `./adrs/accepted/0006-x.md`.
pub fn render_block(store: &Store, link_prefix: &str) -> Result<String, crate::store::StoreError> {
    let files = store.list_files()?;
    let mut by_status: Vec<(Status, Vec<Entry>)> =
        Status::ALL.iter().map(|s| (*s, Vec::new())).collect();

    for path in files {
        let adr = store.read(&path)?;
        let Some(number) = adr.number else { continue };
        let rel = path
            .strip_prefix(store.root())
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let link = format!("{}/{}", link_prefix.trim_end_matches('/'), rel);
        let entry = Entry {
            number,
            title: adr.title,
            link,
        };
        if let Some(bucket) = by_status.iter_mut().find(|(s, _)| *s == adr.status) {
            bucket.1.push(entry);
        }
    }

    let mut out = String::new();
    for (status, mut entries) in by_status {
        if entries.is_empty() {
            continue;
        }
        entries.sort_by_key(|e| e.number);
        out.push_str(&format!("## {status}\n\n"));
        for e in entries {
            out.push_str(&format!("- [ADR-{}: {}]({})\n", e.number, e.title, e.link));
        }
        out.push('\n');
    }
    Ok(out.trim_end().to_string())
}

/// Splice a freshly rendered status block into existing SUMMARY.md content,
/// preserving the non-ADR header and any trailing material.
pub fn splice(existing: &str, block: &str) -> String {
    let status_names: Vec<String> = Status::ALL.iter().map(|s| format!("## {s}")).collect();

    let lines: Vec<&str> = existing.lines().collect();
    // Find the first managed `## <Status>` heading.
    let first = lines.iter().position(|l| {
        let t = l.trim();
        status_names.iter().any(|h| h.eq_ignore_ascii_case(t))
    });

    let Some(first) = first else {
        // No managed block yet: append after a blank line.
        let mut out = existing.trim_end().to_string();
        out.push_str("\n\n");
        out.push_str(block);
        out.push('\n');
        return out;
    };

    // Find the end of the managed region: the last line that belongs to a
    // managed status section (heading, blank, or `- [` entry following one).
    let mut last = first;
    let mut i = first;
    let mut in_managed = false;
    while i < lines.len() {
        let t = lines[i].trim();
        if status_names.iter().any(|h| h.eq_ignore_ascii_case(t)) {
            in_managed = true;
            last = i;
        } else if in_managed && (t.is_empty() || t.starts_with("- [ADR-")) {
            last = i;
        } else if in_managed && !t.is_empty() {
            // A non-blank, non-entry line ends the managed region.
            break;
        }
        i += 1;
    }

    let head = lines[..first].join("\n");
    let tail: String = if last + 1 < lines.len() {
        let rest: Vec<&str> = lines[last + 1..]
            .iter()
            .skip_while(|l| l.trim().is_empty())
            .copied()
            .collect();
        rest.join("\n")
    } else {
        String::new()
    };

    let mut out = head.trim_end().to_string();
    out.push_str("\n\n");
    out.push_str(block);
    if !tail.is_empty() {
        out.push_str("\n\n");
        out.push_str(tail.trim_end());
    }
    out.push('\n');
    out
}

/// Regenerate the SUMMARY.md at `summary_path`, preserving non-ADR parts.
/// Returns the new SUMMARY content (also written to disk).
pub fn regenerate(store: &Store, summary_path: &Path, link_prefix: &str) -> anyhow::Result<String> {
    let block = render_block(store, link_prefix)?;
    let existing = std::fs::read_to_string(summary_path).unwrap_or_default();
    let updated = splice(&existing, &block);
    std::fs::write(summary_path, &updated)?;
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splice_replaces_managed_block_preserving_header() {
        let existing = "# Summary\n\n[Introduction](./README.md)\n\n# Architecture Decision Records\n\n- [ADR Process](./adrs/README.md)\n\n## Proposed\n\n- [ADR-0001: Old](./adrs/proposed/0001-old.md)\n";
        let block = "## Accepted\n\n- [ADR-0006: New](./adrs/accepted/0006-new.md)";
        let out = splice(existing, block);
        assert!(out.contains("# Summary"));
        assert!(out.contains("[Introduction](./README.md)"));
        assert!(out.contains("- [ADR Process](./adrs/README.md)"));
        assert!(out.contains("## Accepted"));
        assert!(out.contains("ADR-0006: New"));
        // The stale proposed entry is gone.
        assert!(!out.contains("ADR-0001: Old"));
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn splice_appends_when_no_managed_block() {
        let existing = "# Summary\n\n[Introduction](./README.md)\n";
        let block = "## Proposed\n\n- [ADR-0011: Repo](./adrs/proposed/0011-repo.md)";
        let out = splice(existing, block);
        assert!(out.contains("# Summary"));
        assert!(out.contains("## Proposed"));
    }
}
