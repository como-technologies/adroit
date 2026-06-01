//! Cross-ADR link integrity: keep relative markdown links between ADRs pointing
//! at each file's *current* location, so a status change (which moves a file
//! between by-status directories) doesn't break `[..](../proposed/X.md)`
//! references in other ADRs — or the moved file's own outbound links.
//!
//! These are pure functions. The orchestration (read every ADR, rewrite the
//! ones that changed, write them back) lives in [`crate::store::Store::relink`].

use std::path::{Path, PathBuf};

/// Relative link from `from_dir` to `to_file`, POSIX `/`-separated. Same-dir
/// targets keep a `./` prefix (matching the prevailing repo style); deeper
/// targets emit `../` segments. Both paths should share a common ancestor.
pub fn rel_link(from_dir: &Path, to_file: &Path) -> String {
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
    let joined = parts.join("/");
    if ups == 0 {
        format!("./{joined}")
    } else {
        joined
    }
}

/// Extract an ADR number from a link target's filename, accepting `ADR-0006`,
/// `.../0006-foo.md`, or a bare `0006` (an optional `#anchor` is ignored).
pub fn number_in_target(target: &str) -> Option<u32> {
    let path = target.split('#').next().unwrap_or(target);
    let segment = path.rsplit('/').next().unwrap_or(path);
    let segment = segment
        .strip_prefix("ADR-")
        .or_else(|| segment.strip_prefix("adr-"))
        .unwrap_or(segment);
    let digits: String = segment.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u32>().ok()
}

/// Whether `target` is a candidate cross-ADR link: a *relative* `.md` path
/// (optionally with `#anchor`) — not an external URL, an absolute path, or an
/// anchor-only reference.
pub fn is_relative_md(target: &str) -> bool {
    if target.is_empty() || target.starts_with('#') || target.starts_with('/') {
        return false;
    }
    if target.contains("://") || target.starts_with("mailto:") {
        return false;
    }
    let path = target.split('#').next().unwrap_or(target);
    path.ends_with(".md")
}

/// The relative-`.md` link targets in `content` (for validation in `check`).
pub fn relative_md_targets(content: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for_each_link(content, |target, _start, _end| {
        if is_relative_md(target) {
            out.push(target);
        }
    });
    out
}

/// Rewrite every cross-ADR relative link in `content` to point at the current
/// location of the ADR it references. `source_dir` is the directory of the file
/// being rewritten; `resolve(number)` returns the canonical absolute path of
/// that ADR's file, or `None` to leave the link untouched (unknown number, or
/// an ambiguous duplicate). Non-ADR, external, and anchor-only links are kept
/// byte-for-byte. Returns the rewritten content and the number of links changed.
pub fn rewrite_links(
    content: &str,
    source_dir: &Path,
    resolve: impl Fn(u32) -> Option<PathBuf>,
) -> (String, usize) {
    let mut out = String::with_capacity(content.len());
    let mut last = 0;
    let mut changed = 0;
    for_each_link(content, |target, start, end| {
        if !is_relative_md(target) {
            return;
        }
        let Some(num) = number_in_target(target) else {
            return;
        };
        let Some(abs) = resolve(num) else {
            return;
        };
        let (pathpart, anchor) = match target.split_once('#') {
            Some((_, a)) => (target, Some(a)),
            None => (target, None),
        };
        let _ = pathpart;
        let mut newt = rel_link(source_dir, &abs);
        if let Some(a) = anchor {
            newt.push('#');
            newt.push_str(a);
        }
        if newt != target {
            out.push_str(&content[last..start]);
            out.push_str(&newt);
            last = end;
            changed += 1;
        }
    });
    out.push_str(&content[last..]);
    (out, changed)
}

/// Scan `content` for markdown link targets `](target)` and invoke `f` with the
/// target text and its byte span `[start, end)` (exclusive of the parens). The
/// `]` `(` `)` delimiters are ASCII, so byte indexing stays on char boundaries.
fn for_each_link<'a>(content: &'a str, mut f: impl FnMut(&'a str, usize, usize)) {
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b']'
            && i + 1 < bytes.len()
            && bytes[i + 1] == b'('
            && let Some(rel_end) = content[i + 2..].find(')')
        {
            let start = i + 2;
            let end = start + rel_end;
            f(&content[start..end], start, end);
            i = end + 1;
            continue;
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Resolver over a fixed by-status tree rooted at /r.
    fn resolve(n: u32) -> Option<PathBuf> {
        match n {
            3 => Some(PathBuf::from("/r/accepted/0003-thing.md")),
            6 => Some(PathBuf::from("/r/accepted/0006-other.md")),
            _ => None, // 9 is "duplicate"/unknown -> untouched
        }
    }

    fn rewrite(content: &str, dir: &str) -> (String, usize) {
        rewrite_links(content, Path::new(dir), resolve)
    }

    #[test]
    fn rewrites_inbound_link_to_new_dir() {
        // A proposed ADR links to ADR-0003 at its OLD location; 0003 now lives in
        // accepted/ -> the link is rewritten relative to the source dir.
        let (out, n) = rewrite("see [x](../proposed/0003-thing.md).", "/r/proposed");
        assert_eq!(n, 1);
        assert_eq!(out, "see [x](../accepted/0003-thing.md).");
    }

    #[test]
    fn rewrites_same_dir_with_dot_slash() {
        let (out, n) = rewrite("see [x](0003-thing.md)", "/r/accepted");
        assert_eq!(n, 1);
        assert_eq!(out, "see [x](./0003-thing.md)");
    }

    #[test]
    fn preserves_anchor() {
        let (out, n) = rewrite("[x](../proposed/0006-other.md#decision)", "/r/proposed");
        assert_eq!(n, 1);
        assert_eq!(out, "[x](../accepted/0006-other.md#decision)");
    }

    #[test]
    fn skips_external_anchor_and_non_adr() {
        let doc = "[a](https://example.com/0003-x.md) [b](#section) \
                   [c](../../guides/adr-review-process.md) [d](mailto:x@y.z)";
        let (out, n) = rewrite(doc, "/r/proposed");
        assert_eq!(n, 0);
        assert_eq!(out, doc);
    }

    #[test]
    fn skips_unknown_or_duplicate_number() {
        // 9 resolves to None (treated as duplicate/unknown) -> left untouched.
        let (out, n) = rewrite("[x](../proposed/0009-dup.md)", "/r/proposed");
        assert_eq!(n, 0);
        assert_eq!(out, "[x](../proposed/0009-dup.md)");
    }

    #[test]
    fn idempotent_second_pass() {
        let once = rewrite("[x](../proposed/0003-thing.md)", "/r/proposed").0;
        let (twice, n) = rewrite(&once, "/r/proposed");
        assert_eq!(n, 0, "already-canonical link must not change");
        assert_eq!(twice, once);
    }

    #[test]
    fn relative_md_targets_lists_only_relative_md() {
        let doc = "[a](../accepted/0003-x.md) [b](https://x/y.md) [c](#s) [d](0006-z.md)";
        assert_eq!(
            relative_md_targets(doc),
            vec!["../accepted/0003-x.md", "0006-z.md"]
        );
    }
}
