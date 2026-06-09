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
/// being rewritten; `resolve(target)` returns the canonical absolute path of the
/// ADR that link target refers to, or `None` to leave the link untouched
/// (non-ADR target, unknown id, or an ambiguous duplicate). Resolving the link
/// target → ADR is the caller's job (it routes through the naming seam), so this
/// engine stays scheme-agnostic. Non-ADR, external, and anchor-only links are
/// kept byte-for-byte. Returns the rewritten content and the number of links
/// changed.
pub fn rewrite_links(
    content: &str,
    source_dir: &Path,
    resolve: impl Fn(&str) -> Option<PathBuf>,
) -> (String, usize) {
    let mut out = String::with_capacity(content.len());
    let mut last = 0;
    let mut changed = 0;
    for_each_link(content, |target, start, end| {
        if !is_relative_md(target) {
            return;
        }
        let Some(abs) = resolve(target) else {
            return;
        };
        let anchor = target.split_once('#').map(|(_, a)| a);
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

/// Resolve a relative POSIX `target` against `base_dir` (both repo-root-relative,
/// `/`-separated), collapsing `.`/`..`. `None` if `..` escapes above the repo root
/// (can't be a blob path).
fn join_normalize(base_dir: &str, target: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for seg in base_dir.split('/').chain(target.split('/')) {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

/// Whether `target` is a *local relative* link (a repo file path) — not an
/// external URL/scheme (`http:`, `mailto:`…), an in-page anchor (`#…`), or a
/// root-absolute path (`/…`).
fn is_local_relative(target: &str) -> bool {
    if target.is_empty() || target.starts_with('#') || target.starts_with('/') {
        return false;
    }
    // A scheme has a `:` before the first `/` or `#` (so `foo.md#a:b` stays local).
    !target
        .split(['/', '#'])
        .next()
        .unwrap_or(target)
        .contains(':')
}

/// Rewrite every *relative* markdown link target in `content` to an absolute
/// `blob_base/<path>` URL, so the links resolve outside the repo file tree — e.g.
/// when an ADR's review-kickoff doc is posted as a GitHub PR or Linear issue
/// **comment**, where relative `.md` links would otherwise dangle. `source_dir` is
/// the doc's directory relative to the repo root; `blob_base` is the provider's web
/// blob URL at the base branch (`https://github.com/owner/repo/blob/main`).
/// External URLs, `mailto:`, in-page `#anchors`, and root-absolute `/paths` are left
/// untouched, as is any `..` that escapes the repo root. Pure (the I/O — resolving
/// the repo root + web base — is the forge caller's).
pub fn absolutize_links(content: &str, source_dir: &str, blob_base: &str) -> String {
    let base = blob_base.trim_end_matches('/');
    let mut out = String::with_capacity(content.len());
    let mut last = 0;
    for_each_link(content, |target, start, end| {
        if !is_local_relative(target) {
            return;
        }
        let (path, anchor) = match target.split_once('#') {
            Some((p, a)) => (p, Some(a)),
            None => (target, None),
        };
        let Some(norm) = join_normalize(source_dir, path) else {
            return; // escapes the repo root → leave it
        };
        let mut newt = format!("{base}/{norm}");
        if let Some(a) = anchor {
            newt.push('#');
            newt.push_str(a);
        }
        out.push_str(&content[last..start]);
        out.push_str(&newt);
        last = end;
    });
    out.push_str(&content[last..]);
    out
}

/// Rewrite every cross-ADR link `[label](target)` whose target **basename**
/// equals `old_base` so it points at `new_base` and its label's `old_label`
/// becomes `new_label`. Used by `adroit renumber` to retarget *and* relabel the
/// inbound references to a single renamed file — matching by basename so a
/// duplicate-numbered sibling (different slug) is left untouched. Returns the
/// rewritten content and the number of links changed.
pub fn relabel_links_to(
    content: &str,
    old_base: &str,
    new_base: &str,
    old_label: &str,
    new_label: &str,
) -> (String, usize) {
    let bytes = content.as_bytes();
    let mut out = String::with_capacity(content.len());
    let mut last = 0;
    let mut count = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b']'
            && i + 1 < bytes.len()
            && bytes[i + 1] == b'('
            && let Some(rel) = content[i + 2..].find(')')
        {
            let tstart = i + 2;
            let tend = tstart + rel;
            let target = &content[tstart..tend];
            let base = target
                .split('#')
                .next()
                .unwrap_or(target)
                .rsplit('/')
                .next()
                .unwrap_or(target);
            // The `[label]` is between the nearest preceding `[` and this `]`,
            // on the same line (ADR link labels don't contain `[` or newlines).
            let label_open = content[..i].rfind('[');
            if base == old_base
                && let Some(lb) = label_open
                && !content[lb..i].contains('\n')
            {
                let label = &content[lb + 1..i];
                out.push_str(&content[last..lb + 1]);
                out.push_str(&label.replace(old_label, new_label));
                out.push_str("](");
                out.push_str(&target.replacen(old_base, new_base, 1));
                out.push(')');
                last = tend + 1;
                count += 1;
            }
            i = tend + 1;
            continue;
        }
        i += 1;
    }
    out.push_str(&content[last..]);
    (out, count)
}

/// Scan `content` for markdown link targets `](target)` and invoke `f` with the
/// target text and its byte span `[start, end)` (exclusive of the parens). The
/// `]` `(` `)` delimiters are ASCII, so byte indexing stays on char boundaries.
pub(crate) fn for_each_link<'a>(content: &'a str, mut f: impl FnMut(&'a str, usize, usize)) {
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

    // Resolver over a fixed by-status tree rooted at /r. Mirrors the sequential
    // caller: extract the ADR number from the target, then map it to a path.
    fn resolve(target: &str) -> Option<PathBuf> {
        match number_in_target(target)? {
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

    #[test]
    fn absolutize_makes_relative_links_blob_urls() {
        let base = "https://github.com/o/r/blob/main";
        let doc = "[ADR](0004-x.md) [readme](../README.md) [guide](../../guides/g.md) \
                   [ext](https://x/y) [anchor](#s) [root](/etc/x)";
        let out = absolutize_links(doc, "docs/adr/proposed", base);
        assert!(
            out.contains("[ADR](https://github.com/o/r/blob/main/docs/adr/proposed/0004-x.md)")
        );
        assert!(out.contains("[readme](https://github.com/o/r/blob/main/docs/adr/README.md)"));
        assert!(out.contains("[guide](https://github.com/o/r/blob/main/docs/guides/g.md)"));
        // External / anchor / root-absolute left untouched.
        assert!(out.contains("[ext](https://x/y)"));
        assert!(out.contains("[anchor](#s)"));
        assert!(out.contains("[root](/etc/x)"));
    }

    #[test]
    fn absolutize_preserves_anchor_and_skips_escaping_paths() {
        let base = "https://github.com/o/r/blob/main";
        assert_eq!(
            absolutize_links("[a](0004-x.md#decision)", "adr", base),
            "[a](https://github.com/o/r/blob/main/adr/0004-x.md#decision)"
        );
        // `..` above the repo root can't be represented → left untouched.
        assert_eq!(
            absolutize_links("[a](../../outside.md)", "adr", base),
            "[a](../../outside.md)"
        );
        // mailto: is a scheme, not a local path.
        assert_eq!(
            absolutize_links("[m](mailto:x@y.z)", "adr", base),
            "[m](mailto:x@y.z)"
        );
    }

    #[test]
    fn absolutize_is_a_noop_when_nothing_is_relative() {
        let base = "https://github.com/o/r/blob/main";
        let doc = "[ext](https://x/y) text [anchor](#s) no links here";
        assert_eq!(absolutize_links(doc, "adr", base), doc);
    }

    #[test]
    fn relabel_links_to_matches_basename_only() {
        // Two links share the label `ADR-0009` but point at different files
        // (a duplicate number with different slugs). Only the basename-matching
        // one is retargeted + relabeled; the sibling is left untouched.
        let doc = "see [ADR-0009](../proposed/0009-adopt-crossplane.md) and \
                   [ADR-0009](../accepted/0009-dex.md).";
        let (out, n) = relabel_links_to(
            doc,
            "0009-adopt-crossplane.md",
            "0021-adopt-crossplane.md",
            "ADR-0009",
            "ADR-0021",
        );
        assert_eq!(n, 1);
        assert!(out.contains("[ADR-0021](../proposed/0021-adopt-crossplane.md)"));
        assert!(
            out.contains("[ADR-0009](../accepted/0009-dex.md)"),
            "sibling untouched: {out}"
        );
    }
}
