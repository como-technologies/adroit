//! The default `static` target: a plain directory of the accepted ADR markdown
//! files plus a generated, category-grouped `index.md`.

use std::path::PathBuf;

use super::{OutputFile, PageCtx, PublishModel, PublishedAdr, Publisher};

pub struct StaticDir;

impl Publisher for StaticDir {
    fn page_path(&self, adr: &PublishedAdr) -> PathBuf {
        PathBuf::from(format!("{}.md", adr.slug))
    }

    fn render_page(&self, ctx: &PageCtx) -> String {
        // Verbatim markdown — cross-links already rewritten.
        ctx.raw.to_string()
    }

    fn aux_files(&self, model: &PublishModel, pages: &[PathBuf]) -> Vec<OutputFile> {
        let mut index = String::from("# Decision log\n\nPublished accepted ADRs.\n");
        for section in &model.sections {
            match &section.name {
                Some(name) => index.push_str(&format!("\n## {name}\n\n")),
                None => index.push('\n'),
            }
            for &i in &section.indices {
                let a = &model.adrs[i];
                let file = pages[i].to_string_lossy();
                index.push_str(&format!("- [{}: {}]({file})\n", a.reference, a.title));
            }
        }
        vec![OutputFile {
            path: PathBuf::from("index.md"),
            contents: index,
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publish::test_model;

    #[test]
    fn copies_pages_flat_and_lists_them_in_index() {
        let m = test_model(false);
        assert_eq!(
            StaticDir.page_path(&m.adrs[0]),
            PathBuf::from("0001-use-postgres.md")
        );
        let pages: Vec<PathBuf> = m.adrs.iter().map(|a| StaticDir.page_path(a)).collect();
        let aux = StaticDir.aux_files(&m, &pages);
        let index = &aux[0];
        assert_eq!(index.path, PathBuf::from("index.md"));
        assert!(
            index
                .contents
                .contains("[ADR-0001: Use PostgreSQL](0001-use-postgres.md)"),
            "{}",
            index.contents
        );
    }
}
