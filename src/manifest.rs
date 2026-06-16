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

impl Manifest {
    /// The per-command catalog — the `mcp` server projects its read verbs as tools.
    #[cfg(feature = "mcp")]
    pub(crate) fn commands(&self) -> &[CommandInfo] {
        &self.commands
    }
}

#[derive(Serialize, Clone)]
pub(crate) struct CommandInfo {
    pub(crate) name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) summary: Option<String>,
    pub(crate) stage: &'static str,
    pub(crate) reads: bool,
    pub(crate) writes: bool,
    pub(crate) idempotent: bool,
    /// Expense profile, for rate-limiting / confirmation: `local` (filesystem
    /// only), `provider-call` (an AI/token call), `network` (a forge/remote API),
    /// or `long-running` (runs until stopped, e.g. a server).
    pub(crate) cost: &'static str,
    /// The `-o json` output shape — a `view` type name (look it up in `types`) or a
    /// short description for ad-hoc shapes. `null` when the command has no JSON form.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) json_output: Option<&'static str>,
    /// Runtime prerequisites: a compiled command may still need an opt-in
    /// (`ai.enabled` + a provider, a configured forge, …) before it will run.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) requires: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) exit: Option<&'static str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) args: Vec<OptionInfo>,
}

impl CommandInfo {
    /// A read-only, side-effect-free verb safe to expose as an MCP tool: it reads,
    /// never writes (the corpus, an output tree — `publish` is classified a
    /// write), and costs only `local` work or a (read-only) AI call. Excludes
    /// `network` verbs (`sync` / `notify` reach a forge / webhook) and
    /// `long-running` servers (`serve` / `mcp`). Per-verb only — a flag can still
    /// escalate a read verb (see [`escalation`]), so the projection also strips
    /// every arg with an `escalates` classification.
    #[cfg(feature = "mcp")]
    pub(crate) fn is_read_tool(&self) -> bool {
        self.reads && !self.writes && matches!(self.cost, "local" | "provider-call")
    }
}

#[derive(Serialize, Clone)]
pub(crate) struct OptionInfo {
    pub(crate) name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) long: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) short: Option<String>,
    #[serde(skip_serializing_if = "is_false")]
    pub(crate) positional: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub(crate) required: bool,
    /// A boolean switch (`--flag`, takes no value) rather than `--opt <value>`.
    #[serde(skip_serializing_if = "is_false")]
    pub(crate) flag: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) value: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) values: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) default: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) env: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) help: Option<String>,
    /// What passing this flag escalates the verb into (ADR-0006): `forge` (the
    /// flag reaches — or applies / previews reaching — the forge over the
    /// network), `file-output` (writes an arbitrary local file), or `writes`
    /// (mutates the corpus). Absent ⇒ the flag keeps the verb's declared
    /// semantics. Safety filters (the MCP projection, downstream allowlists)
    /// treat the (verb, flag) pair, not the verb name, as the unit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) escalates: Option<&'static str>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// The per-(verb, flag) escalation table (ADR-0006). The per-verb [`classified`]
/// semantics are too coarse on their own: `review --forge` un-drafts the PR and
/// posts comments, `--out` writes an arbitrary file, `list --forge` reaches the
/// network — each from a verb classified read-only. Declaring the escalation
/// here keeps the manifest the single source of truth; the MCP projection strips
/// classified flags and downstream allowlists become mechanical. The
/// `escalating_flags_on_read_verbs_are_classified` test fails CI when a suspect
/// flag on a read verb is missing here.
#[rustfmt::skip]
fn escalation(verb: &str, flag: &str) -> Option<&'static str> {
    Some(match (verb, flag) {
        // `--forge` reaches the forge; on `review`, `--yes` / `--dry-run` apply /
        // preview that side effect — the whole forge control surface escalates.
        ("review", "forge" | "yes" | "dry_run") => "forge",
        ("list" | "check", "forge")             => "forge",
        // `--out` writes an arbitrary local file from an otherwise read-only verb.
        ("review" | "plan" | "summarize", "out") => "file-output",
        // ADR-0008: `plan --save` splices the plan into the ADR document — the
        // whole save control surface (`--force` overwrite, `--dry-run` preview)
        // escalates the read verb into a corpus write, and `--regenerate`
        // forces a fresh, nondeterministic provider call where the stored read
        // is free. Stripping all four keeps the projected MCP `plan` tool
        // read-only and deterministic once a plan is stored.
        ("plan", "save" | "force" | "dry_run" | "regenerate") => "writes",
        _ => return None,
    })
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
        // `plan`'s cost / requires are the conservative *generation* path. With
        // a stored plan in the document (ADR-0008) the bare read is local,
        // deterministic, and provider-free — schema 1 has no conditional cost,
        // so that path is expressed additively: the verb summary documents it
        // and `escalates` marks the flags that re-enter generation/write.
        "plan"       => m!("author",  true,  false, true,  PROVIDER,  Some("Plan"),         AI,             None),
        "edit"       => m!("author",  false, true,  false, "local",   None,                 &[],            None),
        "lint"       => m!("author",  true,  false, true,  "local",   Some("LintFinding[]"), &[],           Some("non-zero on mechanical error findings (warnings + --ai advise)")),
        "dedupe"     => m!("author",  true,  false, true,  "local",   Some("Match[]"),      &[],            None),
        "related"    => m!("author",  true,  false, true,  "local",   Some("Match[]"),      &[],            None),
        "link"       => m!("author",  false, true,  true,  "local",   None,                 &[],            None),
        "import"     => m!("author",  false, true,  false, "local",   Some("ImportSummary"), &[],           None),
        // Review & decide
        "set-review" => m!("review",  false, true,  true,  "local",   None,                 &[],            None),
        "review"     => m!("review",  true,  false, true,  "local",   None,                 &[],            None),
        "summarize"  => m!("review",  true,  false, true,  PROVIDER,  None,                 AI,             None),
        "set-status" => m!("review",  false, true,  true,  "local",   None,                 &[],            None),
        "supersede"  => m!("review",  false, true,  true,  "local",   None,                 &[],            None),
        // Explore the corpus
        "list"       => m!("explore", true,  false, true,  "local",   Some("AdrSummary[]"), &[],            Some("0 (read-only)")),
        "show"       => m!("explore", true,  false, true,  "local",   Some("AdrDetail"),    &[],            None),
        "status"     => m!("explore", true,  false, true,  "local",   Some("Status"),       &[],            None),
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
        // `publish` never touches the corpus, but producing an output tree IS a
    // filesystem write (ADR-0007) — consumers filtering on `writes` (the MCP
    // projection included) must get the safe answer.
    "publish"    => m!("maintain", true,  true,  true,  "local",  None,                &[],            None),
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
        "mcp"        => m!("config",  true,  false, false, "long-running", None,            &[],            None),
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

/// `verb` is the owning subcommand (`None` for a global option) — it keys the
/// per-(verb, flag) [`escalation`] lookup.
fn arg_info(verb: Option<&str>, a: &clap::Arg) -> OptionInfo {
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
        escalates: verb.and_then(|v| escalation(v, a.get_id().as_str())),
    }
}

fn type_schemas() -> serde_json::Map<String, Value> {
    fn to_val(s: schemars::Schema) -> Value {
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
    // `status` (Status), `lint` (LintFinding[]), `dedupe` / `related` (Match[]),
    // `ask` (AskAnswer).
    m.insert(
        "Status".into(),
        to_val(schemars::schema_for!(crate::adr::Status)),
    );
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
    m.insert(
        "Plan".into(),
        to_val(schemars::schema_for!(crate::view::Plan)),
    );
    // The one write verb with a structured report: `import` (ImportSummary).
    m.insert(
        "ImportSummary".into(),
        to_val(schemars::schema_for!(crate::view::ImportSummary)),
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
        .map(|a| arg_info(None, a))
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
                .map(|a| arg_info(Some(name), a))
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
    fn publish_is_classified_a_write() {
        // ADR-0007: producing an output tree IS a filesystem write, even though
        // the corpus itself is untouched. Consumers filtering on `writes` must
        // get the safe answer without out-of-band knowledge.
        let v: Value = serde_json::from_str(&json()).unwrap();
        let publish = v["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "publish")
            .expect("publish present");
        assert_eq!(publish["writes"], true, "publish writes an output tree");
        assert_eq!(publish["reads"], true);
    }

    /// Look up `commands[cmd].args[arg].escalates` in the serialized manifest.
    fn escalates(v: &Value, cmd: &str, arg: &str) -> Value {
        v["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == cmd)
            .unwrap_or_else(|| panic!("command `{cmd}` present"))["args"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["name"] == arg)
            .unwrap_or_else(|| panic!("arg `{arg}` on `{cmd}`"))["escalates"]
            .clone()
    }

    #[test]
    fn known_escalations_are_declared() {
        // ADR-0006: the verified MCP write leak — these (verb, flag) pairs
        // escalate a read verb and must say so in `manifest -o json`.
        let v: Value = serde_json::from_str(&json()).unwrap();
        assert_eq!(escalates(&v, "review", "out"), "file-output");
        assert_eq!(escalates(&v, "plan", "out"), "file-output");
        assert_eq!(escalates(&v, "summarize", "out"), "file-output");
        // ADR-0008: the `plan --save` control surface escalates the read verb —
        // a corpus splice (`save` / `force` / `dry_run`) or a forced fresh
        // provider call (`regenerate`).
        assert_eq!(escalates(&v, "plan", "save"), "writes");
        assert_eq!(escalates(&v, "plan", "force"), "writes");
        assert_eq!(escalates(&v, "plan", "dry_run"), "writes");
        assert_eq!(escalates(&v, "plan", "regenerate"), "writes");
        #[cfg(feature = "forge")]
        {
            assert_eq!(escalates(&v, "review", "forge"), "forge");
            assert_eq!(escalates(&v, "review", "yes"), "forge");
            assert_eq!(escalates(&v, "review", "dry_run"), "forge");
            assert_eq!(escalates(&v, "list", "forge"), "forge");
            assert_eq!(escalates(&v, "check", "forge"), "forge");
        }
        // An ordinary read arg stays unclassified — the field is additive.
        assert_eq!(escalates(&v, "list", "status"), Value::Null);
        assert_eq!(escalates(&v, "show", "id"), Value::Null);
    }

    #[test]
    fn escalating_flags_on_read_verbs_are_classified() {
        // The per-flag mirror of `manifest_classifies_every_command` (ADR-0006):
        // on any verb the MCP projection would expose (reads, !writes, local /
        // provider-call cost), a forge-gated or output-path flag MUST carry an
        // `escalates` classification — otherwise a future flag silently re-opens
        // the read-only leak this table exists to close.
        const SUSPECT: &[&str] = &[
            "forge",
            "yes",
            "dry_run",
            "out",
            "save",
            "force",
            "regenerate",
        ];
        let v: Value = serde_json::from_str(&json()).unwrap();
        for c in v["commands"].as_array().unwrap() {
            let read_tool = c["reads"] == true
                && c["writes"] == false
                && matches!(c["cost"].as_str(), Some("local" | "provider-call"));
            if !read_tool {
                continue;
            }
            let Some(args) = c["args"].as_array() else {
                continue;
            };
            for a in args {
                let name = a["name"].as_str().unwrap();
                if SUSPECT.contains(&name) {
                    assert!(
                        a["escalates"].is_string(),
                        "flag `{name}` on read verb `{}` has no `escalates` classification — add it to `escalation()` in src/manifest.rs",
                        c["name"]
                    );
                }
            }
        }
    }

    #[test]
    fn plan_and_review_summaries_are_their_own() {
        // Regression: `plan`'s doc comment carried a copy-pasted `review`
        // summary line, so the manifest advertised the wrong one-liner and
        // `review` had none at all.
        let v: Value = serde_json::from_str(&json()).unwrap();
        let summary = |name: &str| {
            v["commands"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["name"] == name)
                .unwrap_or_else(|| panic!("command `{name}` present"))["summary"]
                .as_str()
                .unwrap_or_else(|| panic!("command `{name}` has a summary"))
                .to_string()
        };
        let plan = summary("plan");
        assert!(
            plan.starts_with("Generate an AI implementation plan"),
            "plan summary: {plan}"
        );
        assert!(
            !plan.contains("review-kickoff"),
            "plan summary still carries review's copy-pasted sentence: {plan}"
        );
        let review = summary("review");
        assert!(
            review.contains("review-kickoff"),
            "review summary: {review}"
        );
    }

    #[test]
    fn output_long_help_names_every_json_read_verb() {
        // `--help-all`'s description of `-o/--output` enumerates the verbs that
        // honor `-o json`; keep that prose in lockstep with the `json_output`
        // column of `classified()` (every verb advertising a JSON shape honors
        // the flag — `manifest` is the exception: always JSON, `-o` ignored).
        let root = crate::cli::Cli::command();
        let help = root
            .get_arguments()
            .find(|a| a.get_id() == "output")
            .expect("global --output")
            .get_long_help()
            .expect("--output has long help")
            .to_string();
        let v: Value = serde_json::from_str(&json()).unwrap();
        for c in v["commands"].as_array().unwrap() {
            let name = c["name"].as_str().unwrap();
            if name == "manifest" || c["json_output"].is_null() {
                continue;
            }
            assert!(
                help.contains(name),
                "--output long help omits `{name}`, which honors -o json ({}): {help}",
                c["json_output"]
            );
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
