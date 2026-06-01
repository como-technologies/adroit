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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

    /// `true` when this scheme's identity is a (re-assignable) sequential number
    /// — i.e. `adroit renumber` applies.
    pub fn is_numeric(&self) -> bool {
        matches!(self, NamingScheme::Sequential | NamingScheme::PerCategory)
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

    /// Parse an ADR's identity from its file path + content. `None` if it can't
    /// be determined under this scheme.
    pub fn parse(&self, path: &Path, content: &str) -> Option<AdrRef> {
        match self {
            NamingScheme::Sequential | NamingScheme::PerCategory => {
                // Prefer the `# ADR-NNNN:` heading, else the filename's number.
                heading_number(content)
                    .or_else(|| leading_number(path))
                    .map(AdrRef::Number)
            }
            // Date / uuid identity is the filename stem.
            NamingScheme::Date | NamingScheme::Uuid => stem(path).map(AdrRef::Slug),
        }
    }

    /// The on-disk filename for an ADR with this ref and title.
    pub fn filename(&self, r: &AdrRef, title: &str) -> String {
        match (self, r) {
            (NamingScheme::Date, AdrRef::Slug(s)) => format!("{s}.md"),
            (NamingScheme::Uuid, AdrRef::Slug(s)) => format!("{s}-{}.md", slugify(title)),
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
            AdrRef::Slug(s) if matches!(self, NamingScheme::Uuid) => {
                format!("ADR-{}", &s[..s.len().min(8)])
            }
            AdrRef::Slug(s) => s.clone(),
        }
    }

    /// The label used inside a cross-ADR markdown link `[label](target)`.
    pub fn link_label(&self, r: &AdrRef) -> String {
        self.display(r)
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
        } else {
            Some(AdrRef::Slug(stem.to_string()))
        }
    }
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
        assert!(NamingScheme::PerCategory.is_numeric());
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
    }

    #[test]
    fn per_category_is_numeric_but_per_dir() {
        let s = NamingScheme::PerCategory;
        assert_eq!(s.scope(), Scope::PerDir);
        // Numbering works like sequential within the given scope.
        assert_eq!(
            s.assign(
                &[AdrRef::Number(2)],
                "x",
                date(2026, Month::June, 1),
                uuid()
            ),
            AdrRef::Number(3)
        );
    }
}
