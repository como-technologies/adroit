//! The `hugo` target: a Hugo content section under `content/adr/` — pages carry
//! TOML front matter (`title` / `date` / `weight`) and the H1 is dropped (the
//! title lives in front matter); each category becomes a sub-section with its own
//! `_index.md`.

use std::path::PathBuf;

use super::{OutputFile, PageCtx, PublishModel, PublishedAdr, Publisher, category_segment, quote};

pub struct Hugo;

impl Publisher for Hugo {
    fn page_path(&self, adr: &PublishedAdr) -> PathBuf {
        match category_segment(adr) {
            Some(cat) => PathBuf::from(format!("content/adr/{cat}/{}.md", adr.slug)),
            None => PathBuf::from(format!("content/adr/{}.md", adr.slug)),
        }
    }

    fn render_page(&self, ctx: &PageCtx) -> String {
        let mut page = String::from("+++\n");
        page.push_str(&format!("title = {}\n", quote(&ctx.adr.title)));
        if let Some(date) = &ctx.adr.date {
            page.push_str(&format!("date = {}\n", quote(date)));
        }
        page.push_str(&format!("weight = {}\n", ctx.order + 1));
        page.push_str("+++\n\n");
        page.push_str(ctx.body);
        page
    }

    fn aux_files(&self, model: &PublishModel, _pages: &[PathBuf]) -> Vec<OutputFile> {
        let mut files = vec![OutputFile {
            path: PathBuf::from("content/adr/_index.md"),
            contents: "+++\ntitle = \"Decision Log\"\n+++\n\nAccepted architecture decisions.\n"
                .to_string(),
        }];
        for (n, section) in model.sections.iter().enumerate() {
            if let Some(name) = &section.name {
                let cat = crate::naming::slugify(name);
                files.push(OutputFile {
                    path: PathBuf::from(format!("content/adr/{cat}/_index.md")),
                    contents: format!("+++\ntitle = {}\nweight = {}\n+++\n", quote(name), n + 1),
                });
            }
        }
        files
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publish::{strip_h1, test_model};

    #[test]
    fn front_matter_drops_h1_and_sections_get_index() {
        let m = test_model(true);
        let body = strip_h1(&m.adrs[0].raw);
        let ctx = PageCtx {
            adr: &m.adrs[0],
            order: 0,
            raw: &m.adrs[0].raw,
            body: &body,
        };
        let page = Hugo.render_page(&ctx);
        assert!(page.starts_with("+++\n"));
        assert!(page.contains("title = \"Use PostgreSQL\""));
        assert!(page.contains("weight = 1"));
        assert!(!page.contains("# Use PostgreSQL"), "H1 dropped: {page}");
        let aux = Hugo.aux_files(&m, &[]);
        assert!(
            aux.iter()
                .any(|f| f.path == PathBuf::from("content/adr/data/_index.md"))
        );
    }
}
