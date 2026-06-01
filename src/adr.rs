use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};
use time::format_description::well_known::Iso8601;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Newtypes
// ---------------------------------------------------------------------------

/// Canonical unique identifier for an ADR (UUID v4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AdrId(Uuid);

impl AdrId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// The underlying UUID (used by the `uuid` naming scheme).
    pub fn uuid(self) -> Uuid {
        self.0
    }
}

impl Default for AdrId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for AdrId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Cosmetic sequential display number (e.g. 1, 2, 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Number(u32);

impl Number {
    pub fn new(n: u32) -> Self {
        Self(n)
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

impl Default for Number {
    fn default() -> Self {
        Self(1)
    }
}

impl fmt::Display for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}", self.0)
    }
}

/// UTC timestamp of when an ADR was created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Created(#[serde(with = "time::serde::rfc3339")] OffsetDateTime);

impl Created {
    pub fn now() -> Self {
        Self(OffsetDateTime::now_utc())
    }

    pub fn get(self) -> OffsetDateTime {
        self.0
    }
}

impl Default for Created {
    fn default() -> Self {
        Self::now()
    }
}

impl fmt::Display for Created {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.0
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|_| fmt::Error)?
        )
    }
}

/// A review deadline for a still-proposed ADR (a plain calendar date).
///
/// Serialized as an ISO-8601 `YYYY-MM-DD` string in both on-disk profiles, so
/// it round-trips identically through YAML frontmatter and the markdown
/// `Review by:` line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ReviewBy(#[serde(with = "review_by_format")] Date);

impl ReviewBy {
    pub fn new(date: Date) -> Self {
        Self(date)
    }

    pub fn get(self) -> Date {
        self.0
    }
}

impl fmt::Display for ReviewBy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.0.format(&Iso8601::DATE).map_err(|_| fmt::Error)?
        )
    }
}

impl FromStr for ReviewBy {
    type Err = AdrError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Date::parse(s.trim(), &Iso8601::DATE)
            .map(Self)
            .map_err(|_| AdrError::BadReviewDate(s.to_string()))
    }
}

/// Serialize/deserialize a [`Date`] as an ISO-8601 `YYYY-MM-DD` string.
mod review_by_format {
    use super::*;
    use serde::{Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(date: &Date, s: S) -> Result<S::Ok, S::Error> {
        let text = date
            .format(&Iso8601::DATE)
            .map_err(serde::ser::Error::custom)?;
        s.serialize_str(&text)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Date, D::Error> {
        let text = String::deserialize(d)?;
        Date::parse(text.trim(), &Iso8601::DATE).map_err(D::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// The lifecycle status of an Architecture Decision Record.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    Display,
    EnumString,
    Serialize,
    Deserialize,
)]
#[strum(ascii_case_insensitive)]
pub enum Status {
    #[default]
    Proposed,
    Accepted,
    Rejected,
    Deprecated,
    Superseded,
}

impl Status {
    /// All statuses in lifecycle order. Useful for iterating layout dirs
    /// and rendering grouped indexes.
    pub const ALL: [Status; 5] = [
        Status::Proposed,
        Status::Accepted,
        Status::Rejected,
        Status::Superseded,
        Status::Deprecated,
    ];
}

// ---------------------------------------------------------------------------
// AdrError
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum AdrError {
    #[error("ADR title must not be empty")]
    EmptyTitle,
    #[error("invalid review date '{0}', expected ISO-8601 YYYY-MM-DD")]
    BadReviewDate(String),
}

// ---------------------------------------------------------------------------
// Adr
// ---------------------------------------------------------------------------

/// A single Architecture Decision Record.
#[derive(Debug, Clone)]
pub struct Adr {
    /// Canonical unique identifier (UUID v4).
    pub id: AdrId,
    /// Cosmetic sequential display number. `None` until assigned by the store on
    /// write — and always `None` under the slug-based (date/uuid) naming schemes.
    pub number: Option<Number>,
    /// Slug identity for the date / uuid naming schemes (the filename stem).
    /// `None` under the numeric (sequential / per-category) schemes.
    pub slug: Option<String>,
    /// Short title describing the decision.
    pub title: String,
    /// Current lifecycle status.
    pub status: Status,
    /// When this ADR was first created (UTC).
    pub created: Created,
    /// Free-form body in Markdown (everything after the frontmatter).
    pub body: String,
    /// The git commit SHA this ADR was last seen at.
    /// Not persisted to disk — populated at read time from git when available.
    pub git_sha: Option<String>,
    /// Reference to an ADR that this record supersedes (the older decision).
    /// Scheme-agnostic ([`AdrRef`]) so it works under date/uuid naming too.
    pub supersedes: Option<crate::naming::AdrRef>,
    /// Reference to an ADR that supersedes this record (the newer decision).
    pub superseded_by: Option<crate::naming::AdrRef>,
    /// Optional review deadline. When a still-`Proposed` ADR is past this date
    /// it is flagged as review-due by the query layer.
    pub review_by: Option<ReviewBy>,
}

impl Adr {
    /// Create a new ADR with only a title. All other fields use defaults.
    /// The `number` is left as `None` — the store assigns it on write.
    pub fn new(title: impl Into<String>) -> Result<Self, AdrError> {
        let title = title.into();
        if title.trim().is_empty() {
            return Err(AdrError::EmptyTitle);
        }
        Ok(Self {
            id: AdrId::default(),
            number: None,
            slug: None,
            title,
            status: Status::default(),
            created: Created::default(),
            body: String::new(),
            git_sha: None,
            supersedes: None,
            superseded_by: None,
            review_by: None,
        })
    }

    /// This ADR's scheme-agnostic display/reference identity. `Number` when a
    /// sequential number is set, else the `Slug` (date/uuid). An unassigned ADR
    /// (neither set, before the store writes it) reports `Number(0)`.
    pub fn reference(&self) -> crate::naming::AdrRef {
        match (self.number, &self.slug) {
            (Some(n), _) => crate::naming::AdrRef::Number(n.get()),
            (None, Some(s)) => crate::naming::AdrRef::Slug(s.clone()),
            (None, None) => crate::naming::AdrRef::Number(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_adr_defaults() {
        let adr = Adr::new("Use PostgreSQL").unwrap();
        assert_eq!(adr.status, Status::Proposed);
        assert!(adr.number.is_none());
        assert!(adr.body.is_empty());
        assert!(adr.git_sha.is_none());
    }

    #[test]
    fn new_adr_empty_title_errors() {
        assert!(Adr::new("").is_err());
        assert!(Adr::new("   ").is_err());
    }

    #[test]
    fn status_display() {
        assert_eq!(Status::Proposed.to_string(), "Proposed");
        assert_eq!(Status::Superseded.to_string(), "Superseded");
    }

    #[test]
    fn status_default_is_proposed() {
        assert_eq!(Status::default(), Status::Proposed);
    }

    #[test]
    fn adr_id_uniqueness() {
        let a = AdrId::new();
        let b = AdrId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn adr_id_display() {
        let id = AdrId::new();
        let s = id.to_string();
        // UUID v4 hyphenated format: 8-4-4-4-12
        assert_eq!(s.len(), 36);
        assert_eq!(s.chars().filter(|c| *c == '-').count(), 4);
    }

    #[test]
    fn number_display_zero_pads() {
        assert_eq!(Number::new(1).to_string(), "0001");
        assert_eq!(Number::new(42).to_string(), "0042");
        assert_eq!(Number::new(9999).to_string(), "9999");
    }

    #[test]
    fn number_default() {
        assert_eq!(Number::default().get(), 1);
    }

    #[test]
    fn status_parses_case_insensitive() {
        assert_eq!("accepted".parse::<Status>().unwrap(), Status::Accepted);
        assert_eq!("PROPOSED".parse::<Status>().unwrap(), Status::Proposed);
        assert_eq!("Deprecated".parse::<Status>().unwrap(), Status::Deprecated);
        assert_eq!("superseded".parse::<Status>().unwrap(), Status::Superseded);
        assert_eq!("rejected".parse::<Status>().unwrap(), Status::Rejected);
    }

    #[test]
    fn status_parse_invalid() {
        assert!("invalid".parse::<Status>().is_err());
    }

    #[test]
    fn review_by_round_trips_iso_date() {
        let rb: ReviewBy = "2026-06-15".parse().unwrap();
        assert_eq!(rb.to_string(), "2026-06-15");
        assert_eq!(rb.get().year(), 2026);
    }

    #[test]
    fn review_by_rejects_bad_date() {
        assert!("not-a-date".parse::<ReviewBy>().is_err());
        assert!("2026/06/15".parse::<ReviewBy>().is_err());
    }

    #[test]
    fn created_display_is_rfc3339() {
        let c = Created::now();
        let s = c.to_string();
        // RFC 3339 contains 'T' separator and ends with 'Z' for UTC
        assert!(s.contains('T'));
        assert!(s.ends_with('Z'));
    }
}
