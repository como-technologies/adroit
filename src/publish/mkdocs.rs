//! The `mkdocs` target: a MkDocs project — `mkdocs.yml` (a `nav:` grouped by
//! category) and the ADR pages under `docs/`. MkDocs keys each page off its H1,
//! so pages are emitted verbatim.

use std::path::PathBuf;

use super::{OutputFile, PageCtx, PublishModel, PublishedAdr, Publisher, category_segment, quote};

pub struct MkDocs;

impl Publisher for MkDocs {
    fn page_path(&self, adr: &PublishedAdr) -> PathBuf {
        match category_segment(adr) {
            Some(cat) => PathBuf::from(format!("docs/{cat}/{}.md", adr.slug)),
            None => PathBuf::from(format!("docs/{}.md", adr.slug)),
        }
    }

    fn render_page(&self, ctx: &PageCtx) -> String {
        ctx.raw.to_string()
    }

    fn aux_files(&self, model: &PublishModel, pages: &[PathBuf]) -> Vec<OutputFile> {
        // nav paths are relative to docs/.
        let nav_path = |i: usize| {
            pages[i]
                .strip_prefix("docs")
                .unwrap_or(&pages[i])
                .to_string_lossy()
                .into_owned()
        };
        let mut yml = String::from("site_name: Decision Log\nnav:\n  - Home: index.md\n");
        for section in &model.sections {
            match &section.name {
                Some(name) => {
                    yml.push_str(&format!("  - {}:\n", quote(name)));
                    for &i in &section.indices {
                        yml.push_str(&format!(
                            "      - {}: {}\n",
                            quote(&model.adrs[i].title),
                            nav_path(i)
                        ));
                    }
                }
                None => {
                    for &i in &section.indices {
                        yml.push_str(&format!(
                            "  - {}: {}\n",
                            quote(&model.adrs[i].title),
                            nav_path(i)
                        ));
                    }
                }
            }
        }
        vec![
            OutputFile {
                path: PathBuf::from("docs/index.md"),
                contents: "# Decision Log\n\nAccepted architecture decisions.\n".to_string(),
            },
            OutputFile {
                path: PathBuf::from("mkdocs.yml"),
                contents: yml,
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publish::test_model;

    #[test]
    fn nests_pages_under_category_and_builds_nav() {
        let m = test_model(true);
        assert_eq!(
            MkDocs.page_path(&m.adrs[0]),
            PathBuf::from("docs/data/0001-use-postgres.md")
        );
        let pages: Vec<PathBuf> = m.adrs.iter().map(|a| MkDocs.page_path(a)).collect();
        let aux = MkDocs.aux_files(&m, &pages);
        let yml = aux
            .iter()
            .find(|f| f.path == PathBuf::from("mkdocs.yml"))
            .unwrap();
        assert!(yml.contents.contains("- \"data\":"), "{}", yml.contents);
        assert!(yml.contents.contains("data/0001-use-postgres.md"));
    }
}
