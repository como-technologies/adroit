use serde::{Deserialize, Serialize};

use crate::adr::{AdrId, Created, Number, Status};

/// The YAML frontmatter fields persisted to disk.
///
/// Separate from `Adr` because the core model has runtime-only fields
/// (`git_sha`, `body`) that don't belong in the YAML block.
#[derive(Debug, Serialize, Deserialize)]
struct Frontmatter {
    id: AdrId,
    number: Number,
    title: String,
    status: Status,
    created: Created,
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
        title: fm.title,
        status: fm.status,
        created: fm.created,
        body: body.trim_end().to_owned(),
        git_sha: None,
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
}
