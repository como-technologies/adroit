//! `adroit manifest` — a machine-readable catalog of the CLI surface for agents
//! and tooling (issue #17). Three layers, none of which can drift from the binary:
//!
//! 1. **Syntax** — derived from the clap `Command` tree (`crate::cli::Cli`), so
//!    every command / arg / enum / default appears automatically (the same source
//!    `--help` and shell completions use). Feature-gated commands appear only when
//!    compiled in.
//! 2. **Output schemas** — JSON Schema of the `view` types (via `schemars`), so an
//!    agent knows the exact `-o json` shapes; they're the same serde structs that
//!    produce the output.
//! 3. **Semantics** — a small owned table ([`classified`]) the clap tree can't
//!    know (reads/writes, idempotent, lifecycle stage, runtime `requires`, exit
//!    meaning). A coverage test asserts every compiled command has an entry.
//!
//! Behind the default-on `manifest` feature so `--no-default-features` drops
//! `schemars`.

use clap::CommandFactory;
use serde::Serialize;
use serde_json::Value;

/// The full manifest document.
#[derive(Serialize)]
pub struct Manifest {
    tool: &'static str,
    version: &'static str,
    /// Version of the manifest's own shape — bump on a breaking change.
    manifest_schema: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    global_options: Vec<OptionInfo>,
    commands: Vec<CommandInfo>,
    /// JSON Schemas for the `view` types the read verbs emit under `-o json`.
    types: serde_json::Map<String, Value>,
}

#[derive(Serialize)]
struct CommandInfo {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
    stage: &'static str,
    reads: bool,
    writes: bool,
    idempotent: bool,
    /// Expense profile, for rate-limiting / confirmation: `local` (filesystem
    /// only), `provider-call` (an AI/token call), `network` (a forge/remote API),
    /// or `long-running` (runs until stopped, e.g. a server).
    cost: &'static str,
    /// The `-o json` output shape — a `view` type name (look it up in `types`) or a
    /// short description for ad-hoc shapes. `null` when the command has no JSON form.
    #[serde(skip_serializing_if = "Option::is_none")]
    json_output: Option<&'static str>,
    /// Runtime prerequisites: a compiled command may still need an opt-in
    /// (`ai.enabled` + a provider, a configured forge, …) before it will run.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    requires: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit: Option<&'static str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    args: Vec<OptionInfo>,
}

#[derive(Serialize)]
struct OptionInfo {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    long: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    short: Option<String>,
    #[serde(skip_serializing_if = "is_false")]
    positional: bool,
    #[serde(skip_serializing_if = "is_false")]
    required: bool,
    /// A boolean switch (`--flag`, takes no value) rather than `--opt <value>`.
    #[serde(skip_serializing_if = "is_false")]
    flag: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    values: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    env: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    help: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Per-command semantics the clap tree can't express.
struct Meta {
    stage: &'static str,
    reads: bool,
    writes: bool,
    idempotent: bool,
    cost: &'static str,
    json_output: Option<&'static str>,
    requires: &'static [&'static str],
    exit: Option<&'static str>,
}

/// The owned semantics table. `None` ⇒ unclassified (the coverage test fails).
/// A superset is fine — entries for commands not compiled in are simply unused.
#[rustfmt::skip]
fn classified(name: &str) -> Option<Meta> {
    // stage, reads, writes, idempotent, cost, json_output, requires, exit
    // cost ∈ { local, provider-call, network, long-running } — the expense profile
    // an agent rate-limits / confirms on.
    macro_rules! m {
        ($s:expr, $r:expr, $w:expr, $i:expr, $c:expr, $j:expr, $req:expr, $e:expr) => {
            Meta { stage: $s, reads: $r, writes: $w, idempotent: $i, cost: $c, json_output: $j, requires: $req, exit: $e }
        };
    }
    const AI: &[&str] = &["ai", "ai.enabled"];
    const PROVIDER: &str = "provider-call";
    const NET: &str = "network";
    Some(match name {
        // Author a decision
        "new"        => m!("author",  false, true,  false, "local",   None,                 &[],            None),
        "draft"      => m!("author",  false, true,  false, PROVIDER,  None,                 AI,             None),
        "compose"    => m!("author",  false, true,  false, PROVIDER,  None,                 AI,             None),
        "plan"       => m!("author",  true,  false, true,  PROVIDER,  None,                 AI,             None),
        "edit"       => m!("author",  false, true,  false, "local",   None,                 &[],            None),
        "lint"       => m!("author",  true,  false, true,  "local",   Some("LintFinding[]"), &[],           Some("non-zero on mechanical findings (--ai is advisory)")),
        "dedupe"     => m!("author",  true,  false, true,  "local",   Some("Match[]"),      &[],            None),
        "related"    => m!("author",  true,  false, true,  "local",   Some("Match[]"),      &[],            None),
        "link"       => m!("author",  false, true,  true,  "local",   None,                 &[],            None),
        "import"     => m!("author",  false, true,  false, "local",   None,                 &[],            None),
        // Review & decide
        "set-review" => m!("review",  false, true,  true,  "local",   None,                 &[],            None),
        "review"     => m!("review",  true,  false, true,  "local",   None,                 &[],            None),
        "summarize"  => m!("review",  true,  false, true,  PROVIDER,  None,                 AI,             None),
        "set-status" => m!("review",  false, true,  true,  "local",   None,                 &[],            None),
        "supersede"  => m!("review",  false, true,  true,  "local",   None,                 &[],            None),
        // Explore the corpus
        "list"       => m!("explore", true,  false, true,  "local",   Some("AdrSummary[]"), &[],            Some("0 (read-only)")),
        "show"       => m!("explore", true,  false, true,  "local",   Some("AdrDetail"),    &[],            None),
        "status"     => m!("explore", true,  false, true,  "local",   None,                 &[],            None),
        "search"     => m!("explore", true,  false, true,  "local",   Some("AdrSummary[]"), &[],            None),
        "stats"      => m!("explore", true,  false, true,  "local",   Some("Stats"),        &[],            None),
        "graph"      => m!("explore", true,  false, true,  "local",   Some("Graph"),        &[],            None),
        "ask"        => m!("explore", true,  false, true,  PROVIDER,  Some("AskAnswer"),    AI,             None),
        "serve"      => m!("explore", true,  false, false, "long-running", None,            &["web"],       None),
        // Maintain the repo
        "check"      => m!("maintain", true,  false, true,  "local",  Some("CheckReport"), &[],            Some("non-zero on an Error-severity problem (CI gate)")),
        "relink"     => m!("maintain", false, true,  true,  "local",  None,                &[],            None),
        "renumber"   => m!("maintain", false, true,  false, "local",  None,                &[],            None),
        "migrate"    => m!("maintain", false, true,  true,  "local",  None,                &[],            None),
        "index"      => m!("maintain", false, true,  true,  "local",  None,                &[],            Some("`--check`: non-zero if SUMMARY.md is stale")),
        "publish"    => m!("maintain", true,  false, true,  "local",  None,                &[],            None),
        // Forge integration (compiled with the `forge` feature)
        "init"       => m!("forge",   false, true,  false, NET,       None,                 &["forge"],          None),
        "auth"       => m!("forge",   false, true,  false, NET,       None,                 &["forge"],          None),
        "sync"       => m!("forge",   true,  false, true,  NET,       None,                 &["forge config"],   None),
        "reconcile"  => m!("forge",   true,  true,  false, NET,       None,                 &["forge config"],   None),
        "notify"     => m!("forge",   true,  false, false, NET,       None,                 &["forge config"],   None),
        // Configuration & meta
        "config"     => m!("config",  true,  true,  true,  "local",   None,                 &[],            None),
        "completions"=> m!("config",  true,  false, true,  "local",   None,                 &[],            None),
        "manifest"   => m!("config",  true,  false, true,  "local",   Some("this document"),&[],            None),
        _ => return None,
    })
}

fn meta(name: &str) -> Meta {
    classified(name).unwrap_or(Meta {
        stage: "other",
        reads: false,
        writes: false,
        idempotent: false,
        cost: "local",
        json_output: None,
        requires: &[],
        exit: None,
    })
}

fn arg_info(a: &clap::Arg) -> OptionInfo {
    // A boolean switch / counter takes no value — don't advertise a placeholder
    // or true/false "possible values" for it.
    let is_switch = matches!(
        a.get_action(),
        clap::ArgAction::SetTrue
            | clap::ArgAction::SetFalse
            | clap::ArgAction::Count
            | clap::ArgAction::Help
            | clap::ArgAction::HelpShort
            | clap::ArgAction::HelpLong
            | clap::ArgAction::Version
    );
    OptionInfo {
        name: a.get_id().as_str().to_string(),
        long: a.get_long().map(|s| format!("--{s}")),
        short: a.get_short().map(|c| format!("-{c}")),
        positional: a.is_positional(),
        required: a.is_required_set(),
        flag: is_switch && !a.is_positional(),
        value: if is_switch {
            None
        } else {
            a.get_value_names()
                .and_then(|v| v.first())
                .map(|s| s.to_string())
        },
        values: if is_switch {
            Vec::new()
        } else {
            a.get_possible_values()
                .iter()
                .map(|p| p.get_name().to_string())
                .collect()
        },
        default: a
            .get_default_values()
            .first()
            .map(|s| s.to_string_lossy().into_owned()),
        env: a.get_env().map(|s| s.to_string_lossy().into_owned()),
        help: a.get_help().map(|s| s.to_string()),
    }
}

fn type_schemas() -> serde_json::Map<String, Value> {
    fn to_val(s: schemars::schema::RootSchema) -> Value {
        serde_json::to_value(s).unwrap_or(Value::Null)
    }
    let mut m = serde_json::Map::new();
    m.insert(
        "AdrSummary".into(),
        to_val(schemars::schema_for!(crate::view::AdrSummary)),
    );
    m.insert(
        "AdrDetail".into(),
        to_val(schemars::schema_for!(crate::view::AdrDetail)),
    );
    m.insert(
        "Stats".into(),
        to_val(schemars::schema_for!(crate::view::Stats)),
    );
    m.insert(
        "Graph".into(),
        to_val(schemars::schema_for!(crate::view::Graph)),
    );
    m.insert(
        "CheckReport".into(),
        to_val(schemars::schema_for!(crate::view::CheckReport)),
    );
    // The ad-hoc read shapes, registered so every `json_output` name resolves:
    // `lint` (LintFinding[]), `dedupe` / `related` (Match[]), `ask` (AskAnswer).
    m.insert(
        "LintFinding".into(),
        to_val(schemars::schema_for!(crate::lint::LintFinding)),
    );
    m.insert(
        "Match".into(),
        to_val(schemars::schema_for!(crate::similar::Match)),
    );
    m.insert(
        "AskAnswer".into(),
        to_val(schemars::schema_for!(crate::view::AskAnswer)),
    );
    m
}

/// Build the manifest from the live clap tree + the semantics table + the type
/// schemas. Commands not compiled in (feature-gated off) never appear.
pub fn build() -> Manifest {
    let root = crate::cli::Cli::command();
    let global_options = root
        .get_arguments()
        .filter(|a| a.is_global_set())
        .map(arg_info)
        .collect();
    let mut commands = Vec::new();
    for sub in root.get_subcommands() {
        let name = sub.get_name();
        if name == "help" {
            continue; // clap's built-in help subcommand
        }
        let info = meta(name);
        commands.push(CommandInfo {
            name: name.to_string(),
            summary: sub.get_about().map(|s| s.to_string()),
            stage: info.stage,
            reads: info.reads,
            writes: info.writes,
            idempotent: info.idempotent,
            cost: info.cost,
            json_output: info.json_output,
            requires: info.requires.to_vec(),
            exit: info.exit,
            args: sub
                .get_arguments()
                .filter(|a| !a.is_global_set() && a.get_id() != "help" && a.get_id() != "help_all")
                .map(arg_info)
                .collect(),
        });
    }
    Manifest {
        tool: "adroit",
        version: env!("CARGO_PKG_VERSION"),
        manifest_schema: 1,
        description: root.get_about().map(|s| s.to_string()),
        global_options,
        commands,
        types: type_schemas(),
    }
}

/// The manifest as pretty JSON (what `adroit manifest` prints).
pub fn json() -> String {
    serde_json::to_string_pretty(&build()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_classifies_every_command() {
        for sub in crate::cli::Cli::command().get_subcommands() {
            let name = sub.get_name();
            if name == "help" {
                continue;
            }
            assert!(
                classified(name).is_some(),
                "command `{name}` has no manifest semantics entry — add it to `classified()` in src/manifest.rs"
            );
        }
    }

    #[test]
    fn manifest_is_valid_json_with_commands_and_type_schemas() {
        let v: Value = serde_json::from_str(&json()).expect("manifest is valid JSON");
        assert_eq!(v["tool"], "adroit");
        assert_eq!(v["manifest_schema"], 1);
        // Globals carry `--output` (the -o json selector itself).
        assert!(
            v["global_options"]
                .as_array()
                .unwrap()
                .iter()
                .any(|o| o["name"] == "output")
        );
        // `list` is present, reads, and its -o json shape is the AdrSummary view…
        let cmds = v["commands"].as_array().unwrap();
        let list = cmds
            .iter()
            .find(|c| c["name"] == "list")
            .expect("list present");
        assert_eq!(list["reads"], true);
        assert_eq!(list["json_output"], "AdrSummary[]");
        // …whose schema is published in `types`.
        assert!(v["types"]["AdrSummary"].is_object());
        assert!(v["types"]["CheckReport"].is_object());
    }

    #[test]
    fn every_json_output_shape_is_registered() {
        // Each command's `json_output` (a `view`-type name, minus any `[]` array
        // suffix) must resolve to a schema in `types` — so an agent can always
        // validate an output it's told to expect. The one non-type value is the
        // manifest's self-description.
        let v: Value = serde_json::from_str(&json()).unwrap();
        let types = v["types"].as_object().unwrap();
        for c in v["commands"].as_array().unwrap() {
            let Some(shape) = c["json_output"].as_str() else {
                continue;
            };
            if shape == "this document" {
                continue;
            }
            let ty = shape.strip_suffix("[]").unwrap_or(shape);
            assert!(
                types.contains_key(ty),
                "command `{}` advertises json_output `{shape}` but `{ty}` is not in `types`",
                c["name"]
            );
        }
    }

    #[test]
    fn every_command_has_a_known_cost() {
        const KNOWN: &[&str] = &["local", "provider-call", "network", "long-running"];
        let v: Value = serde_json::from_str(&json()).unwrap();
        for c in v["commands"].as_array().unwrap() {
            let cost = c["cost"].as_str().expect("cost is a string");
            assert!(
                KNOWN.contains(&cost),
                "command `{}` has unknown cost `{cost}`",
                c["name"]
            );
        }
        // The AI verbs are the expensive ones — `ask` makes a provider call.
        let ask = v["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "ask");
        if let Some(ask) = ask {
            assert_eq!(ask["cost"], "provider-call");
        }
    }

    #[test]
    fn ai_commands_advertise_their_runtime_requirement() {
        let v: Value = serde_json::from_str(&json()).unwrap();
        // `summarize` is compiled here (ai is a default feature) but must advertise
        // that it still needs `ai.enabled` at runtime.
        if let Some(s) = v["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "summarize")
        {
            let req = s["requires"].as_array().unwrap();
            assert!(
                req.iter().any(|x| x == "ai.enabled"),
                "summarize requires: {req:?}"
            );
        }
    }
}
