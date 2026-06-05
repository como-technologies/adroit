//! Model-based ("oracle") testing for the adroit hardening blitz.
//!
//! A proptest generates a random sequence of mutating CLI commands. Each command
//! is run against the **real `adroit` binary** on a throwaway `TempDir` (so the
//! full stack — `main.rs` dispatch, templates, the `Store` write path — is
//! exercised exactly as a user would). In parallel a tiny in-memory **oracle**
//! tracks what the repo *should* contain. After every command we assert a battery
//! of invariants: the on-disk state agrees with the oracle, `adroit check` is
//! clean, the repo is link-canonical (relink is a no-op), and every ADR sits in
//! the directory its status implies.
//!
//! The oracle is a pure *outcome predictor* — it never re-implements adroit's
//! serialization/move logic, only the observable result of each verb — so the
//! oracle itself stays small and is unlikely to carry its own bugs.
//!
//! Spec: docs/superpowers/specs/2026-06-04-adroit-hardening-blitz-design.md

use std::collections::BTreeMap;
use std::process::Command;

use adroit::adr::Status;
use adroit::config::{DateSource, Layout, RelinkScope};
use adroit::format::Format;
use adroit::naming::NamingScheme;
use adroit::store::{Store, StoreOptions};
use adroit::view::Severity;

use proptest::prelude::*;

/// A fixed review date used by the `SetReview` command — set-review only stores
/// the date (the clock-dependent "review due" flagging is asserted elsewhere), so
/// a constant keeps the oracle deterministic.
const REVIEW_DATE: &str = "2026-12-31";

// ---------------------------------------------------------------------------
// Matrix cell
// ---------------------------------------------------------------------------

/// One cell of the format × layout × scheme matrix.
#[derive(Debug, Clone, Copy)]
struct Profile {
    format: Format,
    layout: Layout,
    naming: NamingScheme,
}

impl Profile {
    fn store_options(&self) -> StoreOptions {
        StoreOptions {
            format: self.format,
            layout: self.layout,
            status_dir: BTreeMap::new(),
            review_overdue_days: None,
            date_source: DateSource::Filesystem,
            naming: self.naming,
            relink_scope: RelinkScope::All,
        }
    }

    /// The `--format` / `--layout` / `--naming` CLI values for this cell.
    fn cli_args(&self) -> [&'static str; 8] {
        let format = match self.format {
            Format::Markdown => "markdown",
            Format::Frontmatter => "frontmatter",
        };
        let layout = match self.layout {
            Layout::ByStatus => "by_status",
            Layout::Flat => "flat",
            Layout::ByCategory => "by_category",
        };
        let naming = match self.naming {
            NamingScheme::Sequential => "sequential",
            NamingScheme::Date => "date",
            NamingScheme::Uuid => "uuid",
            NamingScheme::PerCategory => "per_category",
        };
        [
            "--format", format, "--layout", layout, "--naming", naming, "--date-source",
            "filesystem",
        ]
    }
}

// ---------------------------------------------------------------------------
// Commands (abstract — resolved against current state at apply time)
// ---------------------------------------------------------------------------

/// A generated, abstract mutating command. Index fields (`which`/`newer`/`older`)
/// are taken modulo the current ADR count at apply time, so a sequence stays
/// valid no matter how many ADRs exist.
#[derive(Debug, Clone)]
enum Op {
    New { title: String },
    SetStatus { which: usize, status: Status },
    Supersede { newer: usize, older: usize },
    SetReview { which: usize, clear: bool },
    Relink,
}

// ---------------------------------------------------------------------------
// Oracle
// ---------------------------------------------------------------------------

/// The oracle's view of one ADR — only the fields a command can change.
#[derive(Debug, Clone)]
struct ModelAdr {
    number: u32,
    title: String,
    status: Status,
    superseded_by: Option<u32>,
    review_by: Option<String>,
}

struct Harness {
    dir: tempfile::TempDir,
    profile: Profile,
    model: Vec<ModelAdr>,
}

impl Harness {
    fn new(profile: Profile) -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            profile,
            model: Vec::new(),
        }
    }

    /// The sequential number the next `new` will be assigned: max existing + 1.
    fn next_number(&self) -> u32 {
        self.model.iter().map(|a| a.number).max().unwrap_or(0) + 1
    }

    fn find(&self, which: usize) -> Option<usize> {
        if self.model.is_empty() {
            None
        } else {
            Some(which % self.model.len())
        }
    }

    /// Build an `adroit` invocation for this cell, pointed at the temp dir.
    fn cmd(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_adroit"));
        c.arg("--dir")
            .arg(self.dir.path())
            .args(self.profile.cli_args())
            .args(["--relink-scope", "all"])
            // Never block on an editor.
            .env("EDITOR", "true")
            .env("VISUAL", "true");
        c
    }

    /// Run a subcommand and require it to succeed.
    fn run(&self, args: &[&str]) -> Result<(), TestCaseError> {
        let out = self.cmd().args(args).output().expect("spawn adroit");
        prop_assert!(
            out.status.success(),
            "`adroit {}` failed ({})\nstdout: {}\nstderr: {}",
            args.join(" "),
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
        Ok(())
    }

    /// Apply one command to both the real binary and the oracle.
    fn apply(&mut self, op: &Op) -> Result<(), TestCaseError> {
        match op {
            Op::New { title } => {
                let number = self.next_number();
                self.run(&["new", title, "--no-edit"])?;
                self.model.push(ModelAdr {
                    number,
                    title: title.clone(),
                    status: Status::Proposed,
                    superseded_by: None,
                    review_by: None,
                });
            }
            Op::SetStatus { which, status } => {
                let Some(i) = self.find(*which) else {
                    return Ok(());
                };
                let n = self.model[i].number.to_string();
                self.run(&["set-status", &n, &status.to_string().to_lowercase()])?;
                self.model[i].status = *status;
                // A plain set-status rewrites the `## Status` value line, so any
                // existing "Superseded by [..]" note is dropped.
                self.model[i].superseded_by = None;
            }
            Op::Supersede { newer, older } => {
                if self.model.len() < 2 {
                    return Ok(());
                }
                let a = newer % self.model.len();
                let b = older % self.model.len();
                if a == b {
                    return Ok(());
                }
                let new_n = self.model[a].number;
                let old_n = self.model[b].number;
                self.run(&["supersede", &new_n.to_string(), &old_n.to_string()])?;
                self.model[b].status = Status::Superseded;
                self.model[b].superseded_by = Some(new_n);
            }
            Op::SetReview { which, clear } => {
                let Some(i) = self.find(*which) else {
                    return Ok(());
                };
                let n = self.model[i].number.to_string();
                if *clear {
                    self.run(&["set-review", &n, "--clear"])?;
                    self.model[i].review_by = None;
                } else {
                    self.run(&["set-review", &n, REVIEW_DATE])?;
                    self.model[i].review_by = Some(REVIEW_DATE.to_string());
                }
            }
            Op::Relink => {
                self.run(&["relink"])?;
            }
        }
        Ok(())
    }

    /// Assert every invariant against the current on-disk state.
    fn check_invariants(&self) -> Result<(), TestCaseError> {
        let store = Store::open_with(self.dir.path(), self.profile.store_options())
            .map_err(|e| TestCaseError::fail(format!("open store: {e}")))?;
        let entries = store
            .list_with_paths()
            .map_err(|e| TestCaseError::fail(format!("list_with_paths: {e}")))?;

        // (A) The set of ADR numbers on disk equals the oracle's (no missing, no
        //     extra, no duplicates).
        let mut disk: Vec<u32> = Vec::new();
        for (path, adr) in &entries {
            let n = adr
                .number
                .ok_or_else(|| TestCaseError::fail(format!("{} has no number", path.display())))?;
            disk.push(n.get());
        }
        disk.sort_unstable();
        let mut expected: Vec<u32> = self.model.iter().map(|a| a.number).collect();
        expected.sort_unstable();
        prop_assert_eq!(
            &disk,
            &expected,
            "on-disk numbers {:?} != oracle {:?}",
            disk,
            expected
        );

        // (B+C) Per ADR: status, status↔directory, title, supersession, review_by.
        let by_num: BTreeMap<u32, &ModelAdr> =
            self.model.iter().map(|a| (a.number, a)).collect();
        for (path, adr) in &entries {
            let n = adr.number.unwrap().get();
            let m = by_num[&n];

            prop_assert_eq!(
                adr.status,
                m.status,
                "ADR-{} status on disk {:?} != oracle {:?}",
                n,
                adr.status,
                m.status
            );

            // by_status: the file must live in the directory its status implies.
            let parent = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let dir_name = status_dir_name(m.status);
            prop_assert_eq!(
                parent,
                dir_name.as_str(),
                "ADR-{} is in `{}/` but status is {:?}",
                n,
                parent,
                m.status
            );

            prop_assert_eq!(&adr.title, &m.title, "ADR-{} title mismatch", n);

            let disk_sb = adr.superseded_by.as_ref().and_then(|r| r.as_number());
            prop_assert_eq!(
                disk_sb,
                m.superseded_by,
                "ADR-{} superseded_by on disk {:?} != oracle {:?}",
                n,
                disk_sb,
                m.superseded_by
            );

            let disk_rb = adr.review_by.map(|r| r.to_string());
            prop_assert_eq!(
                disk_rb.as_deref(),
                m.review_by.as_deref(),
                "ADR-{} review_by mismatch",
                n
            );
        }

        // (D) `adroit check` reports no errors.
        let report = adroit::query::check(&store)
            .map_err(|e| TestCaseError::fail(format!("query::check: {e}")))?;
        let errors: Vec<&str> = report
            .problems
            .iter()
            .filter(|p| p.severity == Severity::Error)
            .map(|p| p.message.as_str())
            .collect();
        prop_assert!(errors.is_empty(), "adroit check found errors: {:?}", errors);

        // (E) The repo is link-canonical: a relink dry-run rewrites nothing.
        let relink = store
            .relink(false)
            .map_err(|e| TestCaseError::fail(format!("relink dry-run: {e}")))?;
        prop_assert_eq!(
            relink.files_changed,
            0,
            "repo is not link-canonical; relink would rewrite {:?}",
            relink.changed_files
        );

        Ok(())
    }
}

/// The directory name a status maps to under `by_status` (lowercase).
fn status_dir_name(s: Status) -> String {
    s.to_string().to_lowercase()
}

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// A tame title for the first cut (no leading/trailing space, no separators that
/// the heading parser would reinterpret). Adversarial titles come in a later
/// widening pass.
fn arb_title() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[a-z][a-z0-9]{0,15}").unwrap()
}

fn arb_status() -> impl Strategy<Value = Status> {
    prop_oneof![
        Just(Status::Proposed),
        Just(Status::Accepted),
        Just(Status::Rejected),
        Just(Status::Deprecated),
        Just(Status::Superseded),
    ]
}

fn arb_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => arb_title().prop_map(|title| Op::New { title }),
        3 => (any::<usize>(), arb_status())
            .prop_map(|(which, status)| Op::SetStatus { which, status }),
        2 => (any::<usize>(), any::<usize>())
            .prop_map(|(newer, older)| Op::Supersede { newer, older }),
        1 => (any::<usize>(), any::<bool>())
            .prop_map(|(which, clear)| Op::SetReview { which, clear }),
        1 => Just(Op::Relink),
    ]
}

fn arb_ops() -> impl Strategy<Value = Vec<Op>> {
    prop::collection::vec(arb_op(), 1..16)
}

// ---------------------------------------------------------------------------
// The oracle test (smallest matrix cell: markdown / by_status / sequential)
// ---------------------------------------------------------------------------

proptest! {
    // Default 256 cases (the CI budget); override with `PROPTEST_CASES=N` for a
    // wider soak — `ProptestConfig::default()` honors that env var.
    #![proptest_config(ProptestConfig::default())]

    #[test]
    fn oracle_markdown_by_status_sequential(ops in arb_ops()) {
        let mut h = Harness::new(Profile {
            format: Format::Markdown,
            layout: Layout::ByStatus,
            naming: NamingScheme::Sequential,
        });
        for op in &ops {
            h.apply(op)?;
            h.check_invariants()?;
        }
    }
}
