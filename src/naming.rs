//! The ADR identity / naming **seam**.
//!
//! ALL scheme-specific logic lives here. The rest of the codebase depends only
//! on [`AdrRef`] + [`NamingScheme`] and never branches on the scheme — so adding
//! or changing a scheme means editing only this file (plus its tests), never the
//! ~12 consumer modules (store / query / format / index / surfaces).
//!
//! - [`AdrRef`] is the scheme-agnostic *display / reference* identity (the
//!   canonical UUID `adr::AdrId` is separate and unchanged).
//! - [`NamingScheme`] is the config enum; its methods encapsulate how each scheme
//!   assigns, parses, names, displays, links, and scopes ADR identifiers.

use std::path::Path;

use serde::{Deserialize, Serialize};
use time::Date;
use uuid::Uuid;

/// A scheme-agnostic display / reference identity for an ADR.
///
/// `Number` backs the sequential and per-category schemes; `Slug` backs the
/// date (`YYYYMMDD-title`) and uuid schemes (its identity is the filename stem).
///
/// Serializes **untagged** so it round-trips cleanly in YAML frontmatter:
/// `Number(9)` ⇄ `9` (byte-identical with the old numeric fields), `Slug(s)` ⇄
/// the bare string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AdrRef {
    Number(u32),
    Slug(String),
}

impl AdrRef {
    pub fn as_number(&self) -> Option<u32> {
        match self {
            AdrRef::Number(n) => Some(*n),
            AdrRef::Slug(_) => None,
        }
    }

    pub fn as_slug(&self) -> Option<&str> {
        match self {
            AdrRef::Slug(s) => Some(s),
            AdrRef::Number(_) => None,
        }
    }

    /// The canonical **addressing** token — the string a user/URL passes to
    /// reach this ADR, which [`NamingScheme::parse_ref`] round-trips back: the
    /// bare number for numeric schemes, the slug/uuid for slug schemes. (Distinct
    /// from the *display* string, e.g. `ADR-0009` or a shortened uuid.)
    pub fn addr(&self) -> String {
        match self {
            AdrRef::Number(n) => n.to_string(),
            AdrRef::Slug(s) => s.clone(),
        }
    }
}

/// Uniqueness scope a scheme enforces for collision checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Identifiers must be unique across the whole repo (sequential/date/uuid).
    Global,
    /// Identifiers must be unique only within a directory (per-category/MADR).
    PerDir,
}

/// How ADR identifiers are formed. A config enum (serde / clap / strum) whose
/// methods own every scheme's behavior — re-exported by `config` for the
/// `Config`/`StoreOptions` fields.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    Serialize,
    Deserialize,
    strum::Display,
    strum::EnumString,
    clap::ValueEnum,
)]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum NamingScheme {
    /// Global zero-padded `NNNN` (the default). Human-friendly; collision-prone
    /// across branches.
    #[default]
    Sequential,
    /// `YYYYMMDD-title` slug (log4brains-style). Collision-free; no number.
    Date,
    /// A persisted UUID. Collision-free; not human-sortable.
    Uuid,
    /// Per-directory local `NNNN` (MADR categories): numbers unique within a
    /// category, not globally.
    PerCategory,
}

impl NamingScheme {
    /// Uniqueness scope for [`crate::store`]/`check` collision detection.
    pub fn scope(&self) -> Scope {
        match self {
            NamingScheme::PerCategory => Scope::PerDir,
            _ => Scope::Global,
        }
    }

    /// `true` when this scheme's identity is a single global sequential number
    /// — i.e. `adroit renumber`/`review` apply and the CLI accepts a bare number.
    /// Per-category is *not* globally numeric: its identity is a `category/NNNN`
    /// composite (the number is only unique within its category).
    pub fn is_numeric(&self) -> bool {
        matches!(self, NamingScheme::Sequential)
    }

    /// Assign a fresh identity for a new ADR, given the refs already present in
    /// the relevant scope. `today` / `fresh_uuid` are passed in so this stays
    /// pure (and unit-testable); schemes that don't need them ignore them.
    pub fn assign(
        &self,
        existing: &[AdrRef],
        title: &str,
        today: Date,
        fresh_uuid: Uuid,
    ) -> AdrRef {
        match self {
            // Sequential: global max + 1. (Per-category numbering is computed by
            // the store, which knows the category directory — see
            // `Store::next_ref` — so `assign` is only the global-number path.)
            NamingScheme::Sequential | NamingScheme::PerCategory => {
                let max = existing
                    .iter()
                    .filter_map(AdrRef::as_number)
                    .max()
                    .unwrap_or(0);
                AdrRef::Number(max + 1)
            }
            NamingScheme::Date => {
                let base = format!("{}-{}", ymd(today), slugify(title));
                AdrRef::Slug(dedup(base, existing))
            }
            NamingScheme::Uuid => AdrRef::Slug(fresh_uuid.simple().to_string()),
        }
    }

    /// The per-category composite identity `category/NNNN` for a fresh ADR in
    /// `category`, given the numbers already present in that category. Only the
    /// `per_category` scheme uses this (the store supplies the category + scope).
    pub fn assign_in_category(&self, category: &str, existing_numbers: &[u32]) -> AdrRef {
        let next = existing_numbers.iter().copied().max().unwrap_or(0) + 1;
        AdrRef::Slug(percat_id(category, next))
    }

    /// Parse an ADR's identity from its file path + content. `None` if it can't
    /// be determined under this scheme.
    pub fn parse(&self, path: &Path, content: &str) -> Option<AdrRef> {
        match self {
            NamingScheme::Sequential => {
                // Prefer the `# ADR-NNNN:` heading, else the filename's number.
                heading_number(content)
                    .or_else(|| leading_number(path))
                    .map(AdrRef::Number)
            }
            // Per-category identity is `<parent-dir>/<NNNN>` — the directory is
            // the category and the number is local to it.
            NamingScheme::PerCategory => {
                let n = heading_number(content).or_else(|| leading_number(path))?;
                let category = parent_dir_name(path)?;
                Some(AdrRef::Slug(percat_id(&category, n)))
            }
            // Date identity is the whole filename stem (`YYYYMMDD-title`).
            NamingScheme::Date => stem(path).map(AdrRef::Slug),
            // Uuid identity is just the leading uuid (the filename appends a
            // human title slug after it); split it back off so the parsed ref
            // matches what `assign` produced.
            NamingScheme::Uuid => stem(path)
                .map(|s| s.split('-').next().unwrap_or(&s).to_string())
                .map(AdrRef::Slug),
        }
    }

    /// Parse a user-supplied CLI identifier into a ref under this scheme.
    ///
    /// Numeric schemes accept `9`, `0009`, or `ADR-0009`; slug schemes accept
    /// the filename stem (date) or the uuid / its prefix (uuid), with a trailing
    /// `.md` tolerated. `None` if the input can't be a ref for this scheme.
    pub fn parse_ref(&self, input: &str) -> Option<AdrRef> {
        let t = input.trim();
        if t.is_empty() {
            return None;
        }
        if self.is_numeric() {
            let digits = t
                .strip_prefix("ADR-")
                .or_else(|| t.strip_prefix("adr-"))
                .unwrap_or(t);
            leading_digits(digits).map(AdrRef::Number)
        } else if matches!(self, NamingScheme::PerCategory) {
            // `category/NNNN` (or `category/N`) — normalize the number to 4 digits
            // so `infra/1` matches the stored `infra/0001`.
            let (cat, num) = t.rsplit_once('/')?;
            let n = leading_digits(num)?;
            Some(AdrRef::Slug(percat_id(cat, n)))
        } else {
            let stem = t.strip_suffix(".md").unwrap_or(t);
            Some(AdrRef::Slug(stem.to_string()))
        }
    }

    /// Whether a stored ref satisfies a query ref (for `find_path_by_ref`).
    /// Exact for every scheme except uuid, where a unique leading prefix of the
    /// uuid is accepted (so the displayed `ADR-<short>` can be typed back).
    pub fn ref_matches(&self, stored: &AdrRef, query: &AdrRef) -> bool {
        match (self, stored, query) {
            (NamingScheme::Uuid, AdrRef::Slug(s), AdrRef::Slug(q)) => {
                !q.is_empty() && s.starts_with(q.as_str())
            }
            _ => stored == query,
        }
    }

    /// The on-disk filename for an ADR with this ref and title. (For
    /// per-category the directory is the category, so the filename is just the
    /// local `NNNN-title.md`.)
    pub fn filename(&self, r: &AdrRef, title: &str) -> String {
        match (self, r) {
            (NamingScheme::Date, AdrRef::Slug(s)) => format!("{s}.md"),
            (NamingScheme::Uuid, AdrRef::Slug(s)) => format!("{s}-{}.md", slugify(title)),
            (NamingScheme::PerCategory, AdrRef::Slug(s)) => {
                format!("{:04}-{}.md", percat_number(s).unwrap_or(0), slugify(title))
            }
            (_, AdrRef::Number(n)) => format!("{n:04}-{}.md", slugify(title)),
            // Defensive: ref/scheme mismatch — name by the slug.
            (_, AdrRef::Slug(s)) => format!("{s}.md"),
        }
    }

    /// How the ref is shown to humans (lists, headings, `adroit show`).
    pub fn display(&self, r: &AdrRef) -> String {
        match r {
            AdrRef::Number(n) => format!("ADR-{n:04}"),
            // Uuid is long; show a short prefix. Date slug is already readable.
            // Take the first 8 *chars* (not bytes) so a crafted non-hex slug can't
            // panic by slicing inside a multibyte char — a real uuid is ASCII hex,
            // so this stays byte-identical for it.
            AdrRef::Slug(s) if matches!(self, NamingScheme::Uuid) => {
                let short: String = s.chars().take(8).collect();
                format!("ADR-{short}")
            }
            AdrRef::Slug(s) => s.clone(),
        }
    }

    /// The H1 heading line for an ADR — also identity-shaped, so it lives here
    /// (consumers / templates route through this instead of hardcoding
    /// `# ADR-NNNN:`). Numeric schemes get `# ADR-NNNN: Title`; slug schemes
    /// (date/uuid) get a plain `# Title` (log4brains-style, identity in the
    /// filename).
    pub fn heading(&self, r: &AdrRef, title: &str) -> String {
        match r {
            AdrRef::Number(n) => format!("# ADR-{n:04}: {title}"),
            // Per-category carries the local number in the heading.
            AdrRef::Slug(s) if matches!(self, NamingScheme::PerCategory) => {
                format!("# ADR-{:04}: {title}", percat_number(s).unwrap_or(0))
            }
            AdrRef::Slug(_) => format!("# {title}"),
        }
    }

    /// The label used inside a cross-ADR markdown link `[label](target)`.
    pub fn link_label(&self, r: &AdrRef) -> String {
        self.display(r)
    }

    /// Resolve a supersession reference from the fragment that follows
    /// `Superseded by` / `Supersedes` in a markdown `## Status` region — either a
    /// `[label](target)` link (resolved from the target via [`ref_in_link`]) or a
    /// bare token (`ADR-0009` or a slug, resolved via [`parse_ref`]).
    ///
    /// [`ref_in_link`]: Self::ref_in_link
    /// [`parse_ref`]: Self::parse_ref
    pub fn ref_in_note(&self, fragment: &str) -> Option<AdrRef> {
        if let Some(open) = fragment.find("](") {
            let after = &fragment[open + 2..];
            let target = after.split(')').next().unwrap_or(after);
            return self.ref_in_link(target);
        }
        let token = fragment
            .trim()
            .trim_start_matches('[')
            .split([']', ' ', ')', ','])
            .next()
            .unwrap_or("")
            .trim();
        if token.is_empty() {
            None
        } else {
            self.parse_ref(token)
        }
    }

    /// Extract the ADR ref a relative link target points at (filename-based), so
    /// relink/check can match links to ADRs without knowing the scheme.
    pub fn ref_in_link(&self, target: &str) -> Option<AdrRef> {
        let file = target.split('#').next().unwrap_or(target);
        let name = file.rsplit('/').next().unwrap_or(file);
        let stem = name.strip_suffix(".md").unwrap_or(name);
        if stem.is_empty() {
            return None;
        }
        if self.is_numeric() {
            leading_digits(stem).map(AdrRef::Number)
        } else if matches!(self, NamingScheme::PerCategory) {
            // The category is the link's immediate parent directory; the number
            // is the filename's leading digits → `category/NNNN`.
            let path = file.split('#').next().unwrap_or(file);
            let category = path
                .rsplit('/')
                .nth(1)
                .filter(|c| !c.is_empty() && *c != "..")?;
            let n = leading_digits(stem)?;
            Some(AdrRef::Slug(percat_id(category, n)))
        } else {
            Some(AdrRef::Slug(stem.to_string()))
        }
    }
}

/// The canonical per-category composite id: `category/NNNN` (number zero-padded).
fn percat_id(category: &str, number: u32) -> String {
    format!("{category}/{number:04}")
}

/// The local number from a `category/NNNN` composite id.
fn percat_number(id: &str) -> Option<u32> {
    leading_digits(id.rsplit('/').next()?)
}

/// The parent directory's name (the category), for a per-category ADR path.
fn parent_dir_name(path: &Path) -> Option<String> {
    Some(path.parent()?.file_name()?.to_str()?.to_string())
}

// --- shared helpers (scheme-agnostic) --------------------------------------

/// Kebab-case a title: lowercase, non-alphanumerics → spaces, words joined by
/// `-`. Mirrors the original `store::filename` slug logic.
fn slugify(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
}

/// `YYYYMMDD` for a date.
fn ymd(d: Date) -> String {
    format!("{}{:02}{:02}", d.year(), u8::from(d.month()), d.day())
}

/// Make `base` unique among `existing` Slug refs by appending `-2`, `-3`, …
fn dedup(base: String, existing: &[AdrRef]) -> String {
    let taken = |s: &str| existing.iter().any(|r| r.as_slug() == Some(s));
    if !taken(&base) {
        return base;
    }
    (2..)
        .map(|i| format!("{base}-{i}"))
        .find(|s| !taken(s))
        .unwrap()
}

/// Leading zero-padded number from a path's filename (`0006-foo.md` → 6).
fn leading_number(path: &Path) -> Option<u32> {
    leading_digits(path.file_name()?.to_str()?)
}

fn leading_digits(s: &str) -> Option<u32> {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// Number from a `# ADR-NNNN: Title` heading.
fn heading_number(content: &str) -> Option<u32> {
    for line in content.lines() {
        let t = line.trim_start_matches('#').trim();
        if let Some(rest) = t.strip_prefix("ADR-").or_else(|| t.strip_prefix("adr-")) {
            return leading_digits(rest);
        }
    }
    None
}

/// Filename without the `.md` extension.
fn stem(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    Some(name.strip_suffix(".md").unwrap_or(name).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Month;

    fn date(y: i32, m: Month, d: u8) -> Date {
        Date::from_calendar_date(y, m, d).unwrap()
    }
    fn uuid() -> Uuid {
        Uuid::from_u128(0x1234_5678_9abc_def0_1234_5678_9abc_def0)
    }

    #[test]
    fn scope_and_is_numeric() {
        assert_eq!(NamingScheme::Sequential.scope(), Scope::Global);
        assert_eq!(NamingScheme::Date.scope(), Scope::Global);
        assert_eq!(NamingScheme::PerCategory.scope(), Scope::PerDir);
        assert!(NamingScheme::Sequential.is_numeric());
        // Per-category is per-dir scoped and addressed by a `category/NNNN`
        // composite, so it is NOT a single global number.
        assert!(!NamingScheme::PerCategory.is_numeric());
        assert!(!NamingScheme::Date.is_numeric());
        assert!(!NamingScheme::Uuid.is_numeric());
    }

    #[test]
    fn sequential_round_trip() {
        let s = NamingScheme::Sequential;
        let existing = [AdrRef::Number(1), AdrRef::Number(4)];
        assert_eq!(
            s.assign(&existing, "x", date(2026, Month::June, 1), uuid()),
            AdrRef::Number(5)
        );
        let r = AdrRef::Number(9);
        assert_eq!(
            s.filename(&r, "Adopt Crossplane!"),
            "0009-adopt-crossplane.md"
        );
        assert_eq!(s.display(&r), "ADR-0009");
        assert_eq!(s.link_label(&r), "ADR-0009");
        assert_eq!(
            s.ref_in_link("../accepted/0009-adopt-crossplane.md"),
            Some(AdrRef::Number(9))
        );
        assert_eq!(
            s.parse(Path::new("proposed/0009-x.md"), "# ADR-0009: X\n"),
            Some(AdrRef::Number(9))
        );
        // Falls back to the filename number when the heading lacks one.
        assert_eq!(
            s.parse(Path::new("0007-x.md"), "# X\n"),
            Some(AdrRef::Number(7))
        );
    }

    #[test]
    fn date_scheme_is_collision_free_and_dedups() {
        let s = NamingScheme::Date;
        let r = s.assign(&[], "Adopt Crossplane", date(2026, Month::June, 1), uuid());
        assert_eq!(r, AdrRef::Slug("20260601-adopt-crossplane".into()));
        assert_eq!(s.filename(&r, "ignored"), "20260601-adopt-crossplane.md");
        assert_eq!(s.display(&r), "20260601-adopt-crossplane");
        assert_eq!(
            s.ref_in_link("../accepted/20260601-adopt-crossplane.md"),
            Some(AdrRef::Slug("20260601-adopt-crossplane".into()))
        );
        assert_eq!(
            s.parse(Path::new("x/20260601-adopt-crossplane.md"), ""),
            Some(AdrRef::Slug("20260601-adopt-crossplane".into()))
        );
        // Same day + title → suffixed, so it never collides.
        let dup = s.assign(&[r], "Adopt Crossplane", date(2026, Month::June, 1), uuid());
        assert_eq!(dup, AdrRef::Slug("20260601-adopt-crossplane-2".into()));
    }

    #[test]
    fn uuid_scheme() {
        let s = NamingScheme::Uuid;
        let r = s.assign(&[], "Adopt Crossplane", date(2026, Month::June, 1), uuid());
        assert_eq!(r, AdrRef::Slug("123456789abcdef0123456789abcdef0".into()));
        assert_eq!(
            s.filename(&r, "Adopt Crossplane"),
            "123456789abcdef0123456789abcdef0-adopt-crossplane.md"
        );
        assert_eq!(s.display(&r), "ADR-12345678"); // short prefix
        // Parsing the written filename recovers the *bare* uuid (the title slug
        // after it is dropped), so the parsed ref equals what `assign` produced.
        assert_eq!(
            s.parse(
                Path::new("x/123456789abcdef0123456789abcdef0-adopt-crossplane.md"),
                ""
            ),
            Some(r.clone())
        );
        // Addressable by a unique leading prefix of the uuid.
        assert!(s.ref_matches(&r, &AdrRef::Slug("12345678".into())));
        assert!(!s.ref_matches(&r, &AdrRef::Slug("ffff".into())));
    }

    #[test]
    fn uuid_display_tolerates_multibyte_slug() {
        // Regression (hardening blitz parser fuzz): `display` shortened a uuid slug
        // by slicing the first 8 *bytes*, which panics when byte 8 lands inside a
        // multibyte char (a crafted id / filename). It now takes 8 chars instead.
        let s = NamingScheme::Uuid;
        // A real (hex) uuid still shortens to exactly 8 chars, byte-identical.
        assert_eq!(
            s.display(&AdrRef::Slug("123456789abcdef0".into())),
            "ADR-12345678"
        );
        // A non-hex / multibyte slug must not panic.
        let _ = s.display(&AdrRef::Slug("a𐀀𐀀".into()));
        assert_eq!(s.display(&AdrRef::Slug("éè".into())), "ADR-éè");
    }

    #[test]
    fn ref_in_note_resolves_link_and_bare_token() {
        let seq = NamingScheme::Sequential;
        assert_eq!(
            seq.ref_in_note(" [ADR-0006](../accepted/0006-adopt-adrs.md)"),
            Some(AdrRef::Number(6))
        );
        assert_eq!(seq.ref_in_note(" ADR-0006"), Some(AdrRef::Number(6)));

        let date = NamingScheme::Date;
        assert_eq!(
            date.ref_in_note(" [20260601-x](../accepted/20260601-x.md)"),
            Some(AdrRef::Slug("20260601-x".into()))
        );
        assert_eq!(
            date.ref_in_note(" 20260601-x"),
            Some(AdrRef::Slug("20260601-x".into()))
        );
    }

    #[test]
    fn addr_round_trips_through_parse_ref() {
        let seq = NamingScheme::Sequential;
        let r = AdrRef::Number(9);
        assert_eq!(r.addr(), "9");
        assert_eq!(seq.parse_ref(&r.addr()), Some(r));

        let date = NamingScheme::Date;
        let r = AdrRef::Slug("20260601-x".into());
        assert_eq!(r.addr(), "20260601-x");
        assert_eq!(date.parse_ref(&r.addr()), Some(r));
    }

    #[test]
    fn parse_ref_accepts_human_input() {
        let seq = NamingScheme::Sequential;
        assert_eq!(seq.parse_ref("9"), Some(AdrRef::Number(9)));
        assert_eq!(seq.parse_ref("0009"), Some(AdrRef::Number(9)));
        assert_eq!(seq.parse_ref("ADR-0009"), Some(AdrRef::Number(9)));
        assert_eq!(seq.parse_ref("  12 "), Some(AdrRef::Number(12)));
        assert_eq!(seq.parse_ref("nope"), None);

        let date = NamingScheme::Date;
        assert_eq!(
            date.parse_ref("20260601-adopt-x"),
            Some(AdrRef::Slug("20260601-adopt-x".into()))
        );
        // A trailing `.md` (e.g. tab-completed) is tolerated.
        assert_eq!(
            date.parse_ref("20260601-adopt-x.md"),
            Some(AdrRef::Slug("20260601-adopt-x".into()))
        );
    }

    #[test]
    fn per_category_composite_identity() {
        let s = NamingScheme::PerCategory;
        assert_eq!(s.scope(), Scope::PerDir);
        assert!(!s.is_numeric());

        // Per-directory numbering → `category/NNNN`.
        assert_eq!(
            s.assign_in_category("data", &[1, 2]),
            AdrRef::Slug("data/0003".into())
        );
        assert_eq!(
            s.assign_in_category("infra", &[]),
            AdrRef::Slug("infra/0001".into())
        );

        let r = AdrRef::Slug("data/0003".into());
        // Filename is the local number (the dir is the category).
        assert_eq!(s.filename(&r, "Use Kafka!"), "0003-use-kafka.md");
        // Display / heading carry the composite id / local number.
        assert_eq!(s.display(&r), "data/0003");
        assert_eq!(s.heading(&r, "Use Kafka"), "# ADR-0003: Use Kafka");
        // Parse from a path: parent dir = category, filename = number.
        assert_eq!(
            s.parse(
                Path::new("repo/data/0003-use-kafka.md"),
                "# ADR-0003: Use Kafka\n"
            ),
            Some(AdrRef::Slug("data/0003".into()))
        );
        // CLI input accepts `data/3` and normalizes to `data/0003`.
        assert_eq!(
            s.parse_ref("data/3"),
            Some(AdrRef::Slug("data/0003".into()))
        );
        // A cross-category link resolves category from the target's parent dir.
        assert_eq!(
            s.ref_in_link("../infra/0001-use-terraform.md"),
            Some(AdrRef::Slug("infra/0001".into()))
        );
    }

    #[test]
    fn heading_is_identity_shaped() {
        assert_eq!(
            NamingScheme::Sequential.heading(&AdrRef::Number(9), "Adopt X"),
            "# ADR-0009: Adopt X"
        );
        // Slug schemes carry identity in the filename, so the heading is plain.
        assert_eq!(
            NamingScheme::Date.heading(&AdrRef::Slug("20260601-adopt-x".into()), "Adopt X"),
            "# Adopt X"
        );
    }
}
