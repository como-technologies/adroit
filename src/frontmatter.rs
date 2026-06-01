use serde::{Deserialize, Serialize};

use crate::adr::{AdrId, Created, Number, ReviewBy, Status};

/// The YAML frontmatter fields persisted to disk.
///
/// Separate from `Adr` because the core model has runtime-only fields
/// (`git_sha`, `body`) that don't belong in the YAML block.
///
/// The supersession and review fields are optional and only serialized when
/// present (`skip_serializing_if`), so existing files stay clean and legacy
/// files without them still parse (`#[serde(default)]`).
#[derive(Debug, Serialize, Deserialize)]
struct Frontmatter {
    id: AdrId,
    number: Number,
    title: String,
    status: Status,
    created: Created,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    supersedes: Option<Number>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    superseded_by: Option<Number>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    review_by: Option<ReviewBy>,
}

/// Render an ADR as a frontmatter + body string for writing to disk.
///
/// Returns an error if the ADR's `number` has not been assigned.
pub fn serialize(adr: &crate::adr::Adr) -> anyhow::Result<String> {
    let number = adr
        .number
        .ok_or_else(|| anyhow::anyhow!("ADR number must be assigned before serializing"))?;

    let fm = Frontmatter {
        id: adr.id,
        number,
        title: adr.title.clone(),
        status: adr.status,
        created: adr.created,
        supersedes: adr.supersedes,
        superseded_by: adr.superseded_by,
        review_by: adr.review_by,
    };

    let yaml = serde_yaml_ng::to_string(&fm)?;
    let mut out = String::from("---\n");
    out.push_str(&yaml);
    out.push_str("---\n");
    if !adr.body.is_empty() {
        out.push('\n');
        out.push_str(&adr.body);
        out.push('\n');
    }
    Ok(out)
}

/// Parse a frontmatter + body string back into an Adr.
///
/// The `git_sha` field is left as `None` — the caller can populate it.
pub fn deserialize(input: &str) -> anyhow::Result<crate::adr::Adr> {
    let (yaml, body) = split_frontmatter(input)?;
    let fm: Frontmatter = serde_yaml_ng::from_str(yaml)?;
    Ok(crate::adr::Adr {
        id: fm.id,
        number: Some(fm.number),
        slug: None,
        title: fm.title,
        status: fm.status,
        created: fm.created,
        body: body.trim_end().to_owned(),
        git_sha: None,
        supersedes: fm.supersedes,
        superseded_by: fm.superseded_by,
        review_by: fm.review_by,
    })
}

/// Split a document into (frontmatter_yaml, body) at the `---` delimiters.
fn split_frontmatter(input: &str) -> anyhow::Result<(&str, &str)> {
    let trimmed = input.trim_start();
    if !trimmed.starts_with("---") {
        anyhow::bail!("missing opening frontmatter delimiter `---`");
    }
    let after_open = trimmed[3..].trim_start_matches(['\r', '\n']);
    let close_pos = after_open
        .find("\n---")
        .ok_or_else(|| anyhow::anyhow!("missing closing frontmatter delimiter `---`"))?;
    let yaml = &after_open[..close_pos];
    let rest = &after_open[close_pos + 4..]; // skip "\n---"
    let body = rest.trim_start_matches(['\r', '\n']);
    Ok((yaml, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adr::{Adr, Number, Status};

    fn sample_adr() -> Adr {
        let mut adr = Adr::new("Use PostgreSQL").unwrap();
        adr.number = Some(Number::new(1));
        adr
    }

    #[test]
    fn round_trip() {
        let adr = sample_adr();
        let text = serialize(&adr).unwrap();
        let parsed = deserialize(&text).unwrap();

        assert_eq!(parsed.id, adr.id);
        assert_eq!(parsed.number, adr.number);
        assert_eq!(parsed.title, adr.title);
        assert_eq!(parsed.status, adr.status);
        assert_eq!(parsed.created, adr.created);
    }

    #[test]
    fn round_trip_with_body() {
        let mut adr = sample_adr();
        adr.body = "## Context\n\nWe need a database.\n\n## Decision\n\nPostgreSQL.".to_string();
        let text = serialize(&adr).unwrap();
        let parsed = deserialize(&text).unwrap();

        assert_eq!(parsed.body, adr.body);
    }

    #[test]
    fn serialize_requires_number() {
        let adr = Adr::new("No number yet").unwrap();
        assert!(serialize(&adr).is_err());
    }

    #[test]
    fn deserialize_missing_opening_delimiter() {
        let input = "no frontmatter here";
        assert!(deserialize(input).is_err());
    }

    #[test]
    fn deserialize_missing_closing_delimiter() {
        let input = "---\nid: bad\n";
        assert!(deserialize(input).is_err());
    }

    #[test]
    fn serialized_format_has_delimiters() {
        let adr = sample_adr();
        let text = serialize(&adr).unwrap();
        assert!(text.starts_with("---\n"));
        assert!(text.contains("\n---\n"));
    }

    #[test]
    fn all_statuses_round_trip() {
        for status in [
            Status::Proposed,
            Status::Accepted,
            Status::Rejected,
            Status::Deprecated,
            Status::Superseded,
        ] {
            let mut adr = sample_adr();
            adr.status = status;
            let text = serialize(&adr).unwrap();
            let parsed = deserialize(&text).unwrap();
            assert_eq!(parsed.status, status);
        }
    }

    #[test]
    fn empty_body_round_trip() {
        let adr = sample_adr();
        assert!(adr.body.is_empty());
        let text = serialize(&adr).unwrap();
        let parsed = deserialize(&text).unwrap();
        assert!(parsed.body.is_empty());
    }

    #[test]
    fn supersession_fields_round_trip() {
        let mut adr = sample_adr();
        adr.supersedes = Some(Number::new(2));
        adr.superseded_by = Some(Number::new(9));
        let text = serialize(&adr).unwrap();
        assert!(text.contains("supersedes:"));
        assert!(text.contains("superseded_by:"));
        let parsed = deserialize(&text).unwrap();
        assert_eq!(parsed.supersedes, Some(Number::new(2)));
        assert_eq!(parsed.superseded_by, Some(Number::new(9)));
    }

    #[test]
    fn supersession_fields_absent_when_none() {
        let adr = sample_adr();
        let text = serialize(&adr).unwrap();
        // Clean files: optional fields are not emitted when unset.
        assert!(!text.contains("supersedes:"));
        assert!(!text.contains("superseded_by:"));
        assert!(!text.contains("review_by:"));
    }

    #[test]
    fn review_by_round_trips() {
        use crate::adr::ReviewBy;
        let mut adr = sample_adr();
        adr.review_by = Some("2026-07-01".parse::<ReviewBy>().unwrap());
        let text = serialize(&adr).unwrap();
        assert!(text.contains("review_by: 2026-07-01"));
        let parsed = deserialize(&text).unwrap();
        assert_eq!(parsed.review_by, adr.review_by);
    }
}
