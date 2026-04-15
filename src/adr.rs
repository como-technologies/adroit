use std::fmt;

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};
use time::OffsetDateTime;
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

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// The lifecycle status of an Architecture Decision Record.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Display, EnumString, Serialize, Deserialize,
)]
#[strum(ascii_case_insensitive)]
pub enum Status {
    #[default]
    Proposed,
    Accepted,
    Deprecated,
    Superseded,
}

// ---------------------------------------------------------------------------
// AdrError
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum AdrError {
    #[error("ADR title must not be empty")]
    EmptyTitle,
}

// ---------------------------------------------------------------------------
// Adr
// ---------------------------------------------------------------------------

/// A single Architecture Decision Record.
#[derive(Debug, Clone)]
pub struct Adr {
    /// Canonical unique identifier (UUID v4).
    pub id: AdrId,
    /// Cosmetic sequential display number. `None` until assigned by the store on write.
    pub number: Option<Number>,
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
            title,
            status: Status::default(),
            created: Created::default(),
            body: String::new(),
            git_sha: None,
        })
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
    }

    #[test]
    fn status_parse_invalid() {
        assert!("invalid".parse::<Status>().is_err());
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
