//! `date_source = git` harness (hardening blitz #4).
//!
//! The oracle (`tests/model.rs`) runs `date_source = filesystem` to stay git-free.
//! This suite exercises the *other* path — the git-history + status-timeline
//! reconstruction in `src/history.rs` — on real git-backed repos. It drives the
//! binary to create ADRs and `git commit`s through their status changes, then
//! asserts (via the library `query` layer) that under `date_source = git` the
//! timeline is reconstructed from the commits, dates are populated, and the
//! structural invariants still hold (`check` clean, statuses intact).

use std::path::Path;
use std::process::Command;

use adroit::adr::Status;
use adroit::config::{DateSource, Layout, RelinkScope};
use adroit::format::Format;
use adroit::naming::NamingScheme;
use adroit::store::{Store, StoreOptions};
use adroit::{query, view::Severity};

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

fn init_repo(dir: &Path) {
    git(dir, &["init", "-q"]);
    git(dir, &["config", "user.email", "t@t.co"]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
}

fn commit(dir: &Path, msg: &str) {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", msg]);
}

/// Run an `adroit` subcommand (markdown / by_status / sequential) and require
/// success, then commit the result.
fn adroit_commit(dir: &Path, args: &[&str], msg: &str) {
    let out = Command::new(env!("CARGO_BIN_EXE_adroit"))
        .arg("--dir")
        .arg(dir)
        .env("EDITOR", "true")
        .env("VISUAL", "true")
        .args(args)
        .output()
        .expect("spawn adroit");
    assert!(
        out.status.success(),
        "adroit {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    commit(dir, msg);
}

/// A read-only store over `dir` reading dates/timeline from **git**.
fn git_store(dir: &Path) -> Store {
    Store::open_with(
        dir,
        StoreOptions {
            format: Format::Markdown,
            layout: Layout::ByStatus,
            status_dir: Default::default(),
            review_overdue_days: None,
            date_source: DateSource::Git,
            naming: NamingScheme::Sequential,
            relink_scope: RelinkScope::All,
        },
    )
    .unwrap()
}

fn check_clean(store: &Store) {
    let report = query::check(store).unwrap();
    let errors: Vec<&str> = report
        .problems
        .iter()
        .filter(|p| p.severity == Severity::Error)
        .map(|p| p.message.as_str())
        .collect();
    assert!(errors.is_empty(), "check errors under git: {errors:?}");
}

#[test]
fn timeline_reconstructs_status_changes_from_git() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    init_repo(root);

    adroit_commit(root, &["new", "Alpha", "--no-edit"], "add 0001"); // proposed
    adroit_commit(root, &["set-status", "1", "accepted"], "accept 0001"); // moved to accepted/

    let store = git_store(root);
    let detail = query::detail(&store, 1).unwrap();

    // The git history yields a created date and a status timeline.
    assert!(
        detail.summary.created.is_some(),
        "git created date populated"
    );
    assert!(
        detail.history.len() >= 2,
        "expected a Proposed→Accepted timeline, got {:?}",
        detail.history
    );
    assert_eq!(detail.history.first().unwrap().status, Status::Proposed);
    assert_eq!(detail.history.last().unwrap().status, Status::Accepted);
    // Timeline dates are non-decreasing (commit order).
    assert!(
        detail.history.windows(2).all(|w| w[0].date <= w[1].date),
        "timeline dates not monotonic: {:?}",
        detail.history
    );
    // Status still resolves correctly, and the repo is clean under git.
    assert_eq!(detail.summary.status, Status::Accepted);
    check_clean(&store);
}

#[test]
fn supersede_timeline_and_invariants_hold_under_git() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    init_repo(root);

    adroit_commit(root, &["new", "Old way", "--no-edit"], "add 0001"); // proposed
    adroit_commit(root, &["new", "New way", "--no-edit"], "add 0002");
    adroit_commit(root, &["set-status", "2", "accepted"], "accept 0002");
    adroit_commit(root, &["supersede", "2", "1"], "supersede 0001 by 0002");

    let store = git_store(root);
    // ADR-1's timeline ends Superseded; ADR-2 is Accepted. Both resolved from git.
    let d1 = query::detail(&store, 1).unwrap();
    assert_eq!(d1.summary.status, Status::Superseded);
    assert_eq!(d1.history.last().unwrap().status, Status::Superseded);
    let d2 = query::detail(&store, 2).unwrap();
    assert_eq!(d2.summary.status, Status::Accepted);

    // No corruption under git: the full set is intact and check is clean.
    let summaries = query::summaries(&store, &query::Filter::default()).unwrap();
    assert_eq!(summaries.len(), 2);
    check_clean(&store);
}

#[test]
fn date_source_git_on_a_non_git_dir_degrades_gracefully() {
    // `date_source = git` against a directory that isn't a git repo must not
    // panic — it falls back to the filesystem and still works.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // No `git init` here.
    let out = Command::new(env!("CARGO_BIN_EXE_adroit"))
        .arg("--dir")
        .arg(root)
        .env("EDITOR", "true")
        .args(["new", "Alpha", "--no-edit"])
        .output()
        .unwrap();
    assert!(out.status.success());

    let store = git_store(root);
    let detail = query::detail(&store, 1).unwrap(); // must not panic
    assert_eq!(detail.summary.status, Status::Proposed);
    check_clean(&store);
}
