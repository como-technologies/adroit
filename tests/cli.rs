use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

fn adroit() -> Command {
    Command::cargo_bin("adroit").unwrap()
}

#[test]
fn no_args_prints_tui_stub() {
    adroit()
        .assert()
        .success()
        .stdout(predicate::str::contains("TUI"));
}

#[test]
fn new_auto_creates_directory() {
    let tmp = tempdir().unwrap();
    let adr_dir = tmp.path().join("adr");

    // No init needed — directory is auto-created
    adroit()
        .args(["--dir", adr_dir.to_str().unwrap(), "new", "First decision"])
        .assert()
        .success();

    assert!(adr_dir.is_dir());
    assert!(adr_dir.join("0001-first-decision.md").exists());
}

#[test]
fn new_creates_adr_file() {
    let tmp = tempdir().unwrap();
    let adr_dir = tmp.path().join("adr");

    adroit()
        .args([
            "--dir",
            adr_dir.to_str().unwrap(),
            "new",
            "Use PostgreSQL for primary datastore",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("0001"));

    assert!(
        adr_dir
            .join("0001-use-postgresql-for-primary-datastore.md")
            .exists()
    );
}

#[test]
fn new_creates_frontmatter() {
    let tmp = tempdir().unwrap();
    let adr_dir = tmp.path().join("adr");

    adroit()
        .args(["--dir", adr_dir.to_str().unwrap(), "new", "Test decision"])
        .assert()
        .success();

    let content = std::fs::read_to_string(adr_dir.join("0001-test-decision.md")).unwrap();
    assert!(content.starts_with("---\n"));
    assert!(content.contains("id:"));
    assert!(content.contains("title: Test decision"));
    assert!(content.contains("status: Proposed"));
    assert!(content.contains("created:"));
}

#[test]
fn list_shows_adrs() {
    let tmp = tempdir().unwrap();
    let adr_dir = tmp.path().join("adr");

    adroit()
        .args(["--dir", adr_dir.to_str().unwrap(), "new", "First decision"])
        .assert()
        .success();

    adroit()
        .args(["--dir", adr_dir.to_str().unwrap(), "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("0001-first-decision.md"));
}

#[test]
fn list_empty_store() {
    let tmp = tempdir().unwrap();
    let adr_dir = tmp.path().join("adr");

    adroit()
        .args(["--dir", adr_dir.to_str().unwrap(), "list"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn version_flag() {
    adroit().arg("--version").assert().success();
}

#[test]
fn help_flag() {
    adroit()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Architecture Decision Records"));
}
