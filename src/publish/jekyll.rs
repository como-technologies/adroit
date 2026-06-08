//! The `jekyll` target: a Jekyll site — an `adrs` collection under `_adrs/`
//! (pages carry YAML front matter with the H1 dropped), a `_config.yml` that
//! declares the collection with an `/adr/:name/` permalink, and an `index.md`
//! that lists the decisions by their permalink.

use std::path::PathBuf;

use super::{OutputFile, PageCtx, PublishModel, PublishedAdr, Publisher};

pub struct Jekyll;

impl Publisher for Jekyll {
    fn page_path(&self, adr: &PublishedAdr) -> PathBuf {
        PathBuf::from(format!("_adrs/{}.md", adr.slug))
    }

    fn render_page(&self, ctx: &PageCtx) -> String {
        let mut page = String::from("---\n");
        page.push_str(&format!("title: {}\n", super::quote(&ctx.adr.title)));
        if let Some(date) = &ctx.adr.date {
            page.push_str(&format!("date: {date}\n"));
        }
        page.push_str(&format!("nav_order: {}\n", ctx.order + 1));
        page.push_str("layout: page\n---\n\n");
        page.push_str(ctx.body);
        page
    }

    fn aux_files(&self, model: &PublishModel, _pages: &[PathBuf]) -> Vec<OutputFile> {
        let config = "title: Decision Log\ncollections:\n  adrs:\n    output: true\n    permalink: /adr/:name/\n";
        let mut index = String::from("---\ntitle: Decision Log\n---\n\n# Decision Log\n");
        for section in &model.sections {
            match &section.name {
                Some(name) => index.push_str(&format!("\n## {name}\n\n")),
                None => index.push('\n'),
            }
            for &i in &section.indices {
                let a = &model.adrs[i];
                // `:name` in the permalink is the source filename stem (the slug).
                index.push_str(&format!(
                    "- [{}: {}](/adr/{}/)\n",
                    a.reference, a.title, a.slug
                ));
            }
        }
        vec![
            OutputFile {
                path: PathBuf::from("_config.yml"),
                contents: config.to_string(),
            },
            OutputFile {
                path: PathBuf::from("index.md"),
                contents: index,
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publish::{strip_h1, test_model};

    #[test]
    fn collection_pages_and_permalink_index() {
        let m = test_model(false);
        assert_eq!(
            Jekyll.page_path(&m.adrs[0]),
            PathBuf::from("_adrs/0001-use-postgres.md")
        );
        let body = strip_h1(&m.adrs[0].raw);
        let ctx = PageCtx {
            adr: &m.adrs[0],
            order: 0,
            raw: &m.adrs[0].raw,
            body: &body,
        };
        let page = Jekyll.render_page(&ctx);
        assert!(page.contains("layout: page"));
        assert!(page.contains("nav_order: 1"));
        let aux = Jekyll.aux_files(&m, &[]);
        let cfg = aux
            .iter()
            .find(|f| f.path == PathBuf::from("_config.yml"))
            .unwrap();
        assert!(cfg.contents.contains("permalink: /adr/:name/"));
        let index = aux
            .iter()
            .find(|f| f.path == PathBuf::from("index.md"))
            .unwrap();
        assert!(index.contents.contains("(/adr/0001-use-postgres/)"));
    }
}
