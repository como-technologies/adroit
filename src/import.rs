//! Seed proposed ADRs from an external artifact — the **Ingest** seam of the
//! portfolio loop (Assess → Prescribe; issue #18).
//!
//! An [`assessments`](https://github.com/como-technologies/assessments) export is a maturity
//! model — `Domain → Practice → Question` leaves, each carrying *context*,
//! *value*, and *risk*. Each **practice** implies a decision the team has to make,
//! so this module turns each practice into a **proposed** draft ADR: the
//! practice's context becomes the problem statement, its value/risk become
//! decision drivers, and its questions become recorded signals. The assessment
//! thus *becomes the decision backlog* instead of dying in a doc.
//!
//! The mapping is **mechanical** — no AI, no network, deterministic — so it always
//! works offline and is fully testable. (Drafting richer prose from the seed is a
//! later `--ai` enhancement; the seeded ADR is a starting point a human refines.)
//!
//! adroit does not depend on the `assessments` crate: the structs here mirror the
//! *export shape* (the contract), with every field defaulted so a partial or
//! evolving export still parses and unknown fields are ignored.

use std::path::Path;

use serde::Deserialize;

/// Marks an ADR body seeded from an assessment (analogous to the AI-draft marker),
/// so a reviewer knows the prose is a mechanical starting point, not a decision.
pub const SEED_MARKER: &str = "<!-- adroit:seeded-from-assessment -->";

/// An `assessments` export (the subset adroit consumes). Every field defaults, so
/// a partial export still parses; unknown fields (`id`, timestamps, …) are ignored.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Assessment {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub domains: Vec<Domain>,
}

/// A domain groups related practices and carries its own context/value/risk.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Domain {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub risk: String,
    #[serde(default)]
    pub practices: Vec<Practice>,
}

/// A practice — the unit that implies a decision (one practice → one seed ADR).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Practice {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub risk: String,
    #[serde(default)]
    pub questions: Vec<Question>,
    #[serde(default)]
    pub effort: Option<String>,
}

/// A diagnostic question under a practice; its text becomes a recorded signal.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Question {
    #[serde(default)]
    pub text: String,
    /// `positive` / `negative` in the export; kept for future filtering, unused today.
    #[serde(default)]
    pub polarity: Option<String>,
}

/// The mechanical mapping of one practice to a seed ADR — pure data, no store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedDraft {
    /// ADR title — the practice name verbatim (identity stays mechanical; the
    /// author renames during review).
    pub title: String,
    /// Source domain (drives the `by_category` category and the provenance note).
    pub domain: String,
    /// Source assessment name (provenance).
    pub assessment: String,
    /// Problem statement — the practice context, falling back to the domain's.
    pub context: String,
    /// Decision-driver lines built from the practice's value / risk / effort.
    pub drivers: Vec<String>,
    /// The practice's question texts, recorded as assessment signals.
    pub signals: Vec<String>,
}

/// Parse an assessment export, dispatching on the file extension: `.json` → JSON,
/// `.toml` → TOML, anything else → YAML (a JSON superset, so it accepts JSON too).
/// All three are `assessments` export formats and deserialize the same structs.
pub fn parse_assessment(text: &str, path: &Path) -> anyhow::Result<Assessment> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "json" => serde_json::from_str(text)
            .map_err(|e| anyhow::anyhow!("parsing assessment JSON ({}): {e}", path.display())),
        "toml" => toml::from_str(text)
            .map_err(|e| anyhow::anyhow!("parsing assessment TOML ({}): {e}", path.display())),
        _ => serde_yaml_ng::from_str(text)
            .map_err(|e| anyhow::anyhow!("parsing assessment YAML ({}): {e}", path.display())),
    }
}

/// Map an assessment to seed drafts — one per practice (practices with a blank
/// name are skipped, since an ADR needs a title). Pure; safe to unit-test.
pub fn seed_drafts(a: &Assessment) -> Vec<SeedDraft> {
    let mut out = Vec::new();
    for d in &a.domains {
        for p in &d.practices {
            let title = p.name.trim();
            if title.is_empty() {
                continue;
            }
            let context = if p.context.trim().is_empty() {
                d.context.trim()
            } else {
                p.context.trim()
            }
            .to_string();

            let mut drivers = Vec::new();
            if !p.value.trim().is_empty() {
                drivers.push(format!("**Why it matters:** {}", p.value.trim()));
            }
            if !p.risk.trim().is_empty() {
                drivers.push(format!("**Risk if unaddressed:** {}", p.risk.trim()));
            }
            if let Some(e) = p.effort.as_deref().map(str::trim).filter(|e| !e.is_empty()) {
                drivers.push(format!("**Estimated effort:** {e}"));
            }

            let signals = p
                .questions
                .iter()
                .map(|q| q.text.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect();

            out.push(SeedDraft {
                title: title.to_string(),
                domain: d.name.trim().to_string(),
                assessment: a.name.trim().to_string(),
                context,
                drivers,
                signals,
            });
        }
    }
    out
}

/// Build the ADR body fragment for a seed — everything from `## Context` onward,
/// with the context and drivers filled and the remaining MADR sections left as
/// their authoring prompts. Begins with [`SEED_MARKER`] and is spliced over a
/// freshly-created ADR's template body (the mechanical heading/status are kept),
/// exactly like an AI draft.
pub fn seed_fragment(s: &SeedDraft) -> String {
    let mut out = String::new();
    out.push_str(SEED_MARKER);

    out.push_str("\n\n## Context and Problem Statement\n\n");
    if s.context.is_empty() {
        out.push_str(
            "_What situation is forcing a decision? (The assessment left this blank — fill it in.)_",
        );
    } else {
        out.push_str(&s.context);
    }
    out.push_str(&format!(
        "\n\n> Seeded from assessment \"{}\" — domain \"{}\" → practice \"{}\".",
        s.assessment, s.domain, s.title
    ));
    if !s.signals.is_empty() {
        out.push_str("\n\nThe assessment flagged:\n");
        for sig in &s.signals {
            out.push_str(&format!("- {sig}\n"));
        }
    }

    out.push_str("\n## Decision Drivers\n\n");
    if s.drivers.is_empty() {
        out.push_str(
            "_What should drive the choice? (The assessment recorded no value/risk — fill it in.)_\n",
        );
    } else {
        for d in &s.drivers {
            out.push_str(&format!("- {d}\n"));
        }
    }

    out.push_str(
        "\n## Considered Options\n\n_List the options you actually weighed — at least two, \
         including the one(s) you rejected — so the trade-off is on the record._\n\n",
    );
    out.push_str(
        "## Decision Outcome\n\n_Name the chosen option and the core reason in one line \
         (\"Chosen: **X**, because …\"), then explain how it answers the drivers above._\n\n",
    );
    out.push_str(
        "### Positive Consequences\n\n_What gets better, easier, or safer as a result?_\n\n",
    );
    out.push_str(
        "### Negative Consequences\n\n_What gets worse, harder, or riskier? Every decision has \
         trade-offs — name them honestly._\n\n",
    );
    out.push_str(
        "## Implementation\n\n_How will the decision be carried out — rollout, migration, the \
         follow-up tasks? Draft it later with `adroit plan`, or delete this section if it doesn't \
         apply._\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const SAMPLE_JSON: &str = r#"{
      "name": "Cloud Maturity",
      "description": "Platform readiness",
      "goal": "Modernize",
      "domains": [
        {
          "name": "Security",
          "context": "Domain-level security context",
          "value": "",
          "risk": "",
          "practices": [
            {
              "name": "Secrets management",
              "context": "Secrets are committed to git today.",
              "value": "Leaked credentials are a top breach vector.",
              "risk": "A leak forces a painful rotation across every service.",
              "effort": "M",
              "questions": [
                {"text": "Are secrets stored outside source control?", "polarity": "positive"},
                {"text": "Is there an audited rotation process?", "polarity": "positive"}
              ]
            },
            {
              "name": "",
              "context": "nameless — should be skipped",
              "value": "x",
              "risk": "y",
              "questions": []
            }
          ]
        }
      ]
    }"#;

    #[test]
    fn parses_json_and_maps_one_draft_per_named_practice() {
        let a = parse_assessment(SAMPLE_JSON, Path::new("x.json")).unwrap();
        assert_eq!(a.name, "Cloud Maturity");
        let drafts = seed_drafts(&a);
        // The nameless practice is skipped.
        assert_eq!(drafts.len(), 1);
        let d = &drafts[0];
        assert_eq!(d.title, "Secrets management");
        assert_eq!(d.domain, "Security");
        assert_eq!(d.assessment, "Cloud Maturity");
        assert!(d.context.starts_with("Secrets are committed"));
        assert_eq!(d.signals.len(), 2);
        // value, risk, effort → three driver lines.
        assert_eq!(d.drivers.len(), 3);
        assert!(d.drivers[0].contains("Why it matters"));
        assert!(
            d.drivers
                .iter()
                .any(|l| l.contains("Estimated effort:** M"))
        );
    }

    #[test]
    fn yaml_parses_too() {
        let yaml = "name: Y\ndomains:\n  - name: D\n    context: dc\n    practices:\n      - name: P\n        context: pc\n        value: v\n        risk: r\n";
        let a = parse_assessment(yaml, Path::new("x.yaml")).unwrap();
        let drafts = seed_drafts(&a);
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].title, "P");
    }

    #[test]
    fn toml_parses_too() {
        let toml = r#"
name = "Y"
[[domains]]
name = "D"
context = "dc"
[[domains.practices]]
name = "P"
context = "pc"
value = "v"
risk = "r"
"#;
        let a = parse_assessment(toml, Path::new("x.toml")).unwrap();
        let drafts = seed_drafts(&a);
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].title, "P");
    }

    #[test]
    fn practice_context_falls_back_to_domain_context() {
        let a = parse_assessment(
            r#"{"name":"A","domains":[{"name":"D","context":"domain ctx","practices":[{"name":"P"}]}]}"#,
            Path::new("x.json"),
        )
        .unwrap();
        let drafts = seed_drafts(&a);
        assert_eq!(drafts[0].context, "domain ctx");
        // No value/risk/effort → no driver lines.
        assert!(drafts[0].drivers.is_empty());
    }

    #[test]
    fn fragment_has_marker_provenance_and_madr_sections() {
        let s = SeedDraft {
            title: "Secrets management".into(),
            domain: "Security".into(),
            assessment: "Cloud Maturity".into(),
            context: "Secrets are committed to git today.".into(),
            drivers: vec!["**Why it matters:** breach vector.".into()],
            signals: vec!["Are secrets stored outside source control?".into()],
        };
        let body = seed_fragment(&s);
        assert!(body.starts_with(SEED_MARKER));
        assert!(body.contains("## Context and Problem Statement"));
        assert!(body.contains("Secrets are committed to git today."));
        assert!(body.contains(
            "> Seeded from assessment \"Cloud Maturity\" — domain \"Security\" → practice \"Secrets management\"."
        ));
        assert!(
            body.contains("The assessment flagged:\n- Are secrets stored outside source control?")
        );
        assert!(body.contains("## Decision Drivers\n\n- **Why it matters:** breach vector."));
        assert!(body.contains("## Considered Options"));
        assert!(body.contains("## Decision Outcome"));
        assert!(body.contains("### Negative Consequences"));
        assert!(body.contains("## Implementation"));
        assert!(body.ends_with('\n'));
    }

    #[test]
    fn blank_context_and_drivers_get_prompts() {
        let s = SeedDraft {
            title: "P".into(),
            domain: "D".into(),
            assessment: "A".into(),
            context: String::new(),
            drivers: Vec::new(),
            signals: Vec::new(),
        };
        let body = seed_fragment(&s);
        assert!(body.contains("What situation is forcing a decision?"));
        assert!(body.contains("What should drive the choice?"));
        // No signals block when there are none.
        assert!(!body.contains("The assessment flagged:"));
    }
}
