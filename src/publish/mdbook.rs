//! The `mdbook` target: an mdBook source tree — `book.toml`, `src/SUMMARY.md`
//! (category sections become `# Header` groups), a landing `src/README.md`, and
//! the ADR pages under `src/`. mdBook keys each page off its H1, so pages are
//! emitted verbatim.

use std::path::{Path, PathBuf};

use super::{OutputFile, PageCtx, PublishModel, PublishedAdr, Publisher};

pub struct MdBook;

impl Publisher for MdBook {
    fn page_path(&self, adr: &PublishedAdr) -> PathBuf {
        PathBuf::from(format!("src/{}.md", adr.slug))
    }

    fn render_page(&self, ctx: &PageCtx) -> String {
        ctx.raw.to_string()
    }

    fn aux_files(&self, model: &PublishModel, pages: &[PathBuf]) -> Vec<OutputFile> {
        let mut summary = String::from("# Summary\n\n[Decision Log](README.md)\n\n");
        for section in &model.sections {
            if let Some(name) = &section.name {
                summary.push_str(&format!("# {name}\n\n"));
            }
            for &i in &section.indices {
                let a = &model.adrs[i];
                // SUMMARY.md lives in src/, so reference pages relative to it.
                let rel = pages[i]
                    .strip_prefix("src")
                    .unwrap_or(&pages[i])
                    .to_string_lossy();
                summary.push_str(&format!("- [{}](./{rel})\n", a.title));
            }
            summary.push('\n');
        }
        vec![
            OutputFile {
                path: PathBuf::from("book.toml"),
                contents: "[book]\ntitle = \"Decision Log\"\nsrc = \"src\"\n".to_string(),
            },
            OutputFile {
                path: Path::new("src").join("README.md"),
                contents: "# Decision Log\n\nAccepted architecture decisions.\n".to_string(),
            },
            OutputFile {
                path: Path::new("src").join("SUMMARY.md"),
                contents: summary,
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publish::test_model;

    #[test]
    fn emits_book_toml_and_summary_listing_pages() {
        let m = test_model(false);
        assert_eq!(
            MdBook.page_path(&m.adrs[0]),
            PathBuf::from("src/0001-use-postgres.md")
        );
        let pages: Vec<PathBuf> = m.adrs.iter().map(|a| MdBook.page_path(a)).collect();
        let aux = MdBook.aux_files(&m, &pages);
        assert!(aux.iter().any(|f| f.path == PathBuf::from("book.toml")));
        let summary = aux
            .iter()
            .find(|f| f.path == Path::new("src").join("SUMMARY.md"))
            .unwrap();
        assert!(
            summary
                .contents
                .contains("[Hexagonal architecture](./0002-hexagonal.md)")
        );
    }
}
