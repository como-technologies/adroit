use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

fn adroit(tmp: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("adroit").unwrap();
    // Isolate every test from the real config to avoid races and filesystem pollution.
    cmd.env("XDG_CONFIG_HOME", tmp.join("config"));
    cmd
}

#[test]
fn no_args_prints_tui_stub() {
    let tmp = tempdir().unwrap();
    adroit(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("TUI"));
}

#[test]
fn new_auto_creates_directory() {
    let tmp = tempdir().unwrap();
    let adr_dir = tmp.path().join("adr");

    adroit(tmp.path())
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

    adroit(tmp.path())
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

    adroit(tmp.path())
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

// ── list ──────────────────────────────────────────────────────────────

#[test]
fn list_shows_table() {
    let tmp = tempdir().unwrap();
    let adr_dir = tmp.path().join("adr");

    adroit(tmp.path())
        .args(["--dir", adr_dir.to_str().unwrap(), "new", "First decision"])
        .assert()
        .success();

    adroit(tmp.path())
        .args(["--dir", adr_dir.to_str().unwrap(), "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("#"))
        .stdout(predicate::str::contains("Status"))
        .stdout(predicate::str::contains("0001"))
        .stdout(predicate::str::contains("Proposed"))
        .stdout(predicate::str::contains("First decision"));
}

#[test]
fn list_empty_store() {
    let tmp = tempdir().unwrap();
    let adr_dir = tmp.path().join("adr");

    adroit(tmp.path())
        .args(["--dir", adr_dir.to_str().unwrap(), "list"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn list_multiple_adrs() {
    let tmp = tempdir().unwrap();
    let adr_dir = tmp.path().join("adr");

    adroit(tmp.path())
        .args(["--dir", adr_dir.to_str().unwrap(), "new", "First"])
        .assert()
        .success();
    adroit(tmp.path())
        .args(["--dir", adr_dir.to_str().unwrap(), "new", "Second"])
        .assert()
        .success();

    adroit(tmp.path())
        .args(["--dir", adr_dir.to_str().unwrap(), "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("0001"))
        .stdout(predicate::str::contains("First"))
        .stdout(predicate::str::contains("0002"))
        .stdout(predicate::str::contains("Second"));
}

// ── show ──────────────────────────────────────────────────────────────

#[test]
fn show_displays_adr() {
    let tmp = tempdir().unwrap();
    let adr_dir = tmp.path().join("adr");

    adroit(tmp.path())
        .args(["--dir", adr_dir.to_str().unwrap(), "new", "Test decision"])
        .assert()
        .success();

    adroit(tmp.path())
        .args(["--dir", adr_dir.to_str().unwrap(), "show", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ADR 0001: Test decision"))
        .stdout(predicate::str::contains("Status:  Proposed"))
        .stdout(predicate::str::contains("Created:"))
        .stdout(predicate::str::contains("ID:"));
}

#[test]
fn show_not_found() {
    let tmp = tempdir().unwrap();
    let adr_dir = tmp.path().join("adr");

    adroit(tmp.path())
        .args(["--dir", adr_dir.to_str().unwrap(), "show", "99"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("0099"));
}

// ── status ────────────────────────────────────────────────────────────

#[test]
fn status_updates_adr() {
    let tmp = tempdir().unwrap();
    let adr_dir = tmp.path().join("adr");

    adroit(tmp.path())
        .args(["--dir", adr_dir.to_str().unwrap(), "new", "Test"])
        .assert()
        .success();

    adroit(tmp.path())
        .args([
            "--dir",
            adr_dir.to_str().unwrap(),
            "status",
            "1",
            "accepted",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Updated ADR 0001 status to Accepted",
        ));

    // Verify the status was persisted
    let content = std::fs::read_to_string(adr_dir.join("0001-test.md")).unwrap();
    assert!(content.contains("status: Accepted"));
}

#[test]
fn status_case_insensitive() {
    let tmp = tempdir().unwrap();
    let adr_dir = tmp.path().join("adr");

    adroit(tmp.path())
        .args(["--dir", adr_dir.to_str().unwrap(), "new", "Test"])
        .assert()
        .success();

    adroit(tmp.path())
        .args([
            "--dir",
            adr_dir.to_str().unwrap(),
            "status",
            "1",
            "DEPRECATED",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Deprecated"));
}

#[test]
fn status_invalid() {
    let tmp = tempdir().unwrap();
    let adr_dir = tmp.path().join("adr");

    adroit(tmp.path())
        .args(["--dir", adr_dir.to_str().unwrap(), "new", "Test"])
        .assert()
        .success();

    adroit(tmp.path())
        .args(["--dir", adr_dir.to_str().unwrap(), "status", "1", "bogus"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid status"));
}

#[test]
fn status_not_found() {
    let tmp = tempdir().unwrap();
    let adr_dir = tmp.path().join("adr");

    adroit(tmp.path())
        .args([
            "--dir",
            adr_dir.to_str().unwrap(),
            "status",
            "99",
            "accepted",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("0099"));
}

// ── edit ──────────────────────────────────────────────────────────────

#[test]
fn edit_opens_editor() {
    let tmp = tempdir().unwrap();
    let adr_dir = tmp.path().join("adr");

    adroit(tmp.path())
        .args(["--dir", adr_dir.to_str().unwrap(), "new", "Test"])
        .assert()
        .success();

    adroit(tmp.path())
        .args(["--dir", adr_dir.to_str().unwrap(), "edit", "1"])
        .env("EDITOR", "true")
        .assert()
        .success();
}

#[test]
fn edit_not_found() {
    let tmp = tempdir().unwrap();
    let adr_dir = tmp.path().join("adr");

    adroit(tmp.path())
        .args(["--dir", adr_dir.to_str().unwrap(), "edit", "99"])
        .env("EDITOR", "true")
        .assert()
        .failure()
        .stderr(predicate::str::contains("0099"));
}

#[test]
fn edit_editor_failure_reports_error() {
    let tmp = tempdir().unwrap();
    let adr_dir = tmp.path().join("adr");

    adroit(tmp.path())
        .args(["--dir", adr_dir.to_str().unwrap(), "new", "Test"])
        .assert()
        .success();

    // Editor that exits non-zero should propagate as an error.
    adroit(tmp.path())
        .args(["--dir", adr_dir.to_str().unwrap(), "edit", "1"])
        .env("EDITOR", "false")
        .assert()
        .failure()
        .stderr(predicate::str::contains("editor"));
}

// ── bootstrap ─────────────────────────────────────────────────────────

#[test]
fn first_run_creates_config() {
    let tmp = tempdir().unwrap();
    let adr_dir = tmp.path().join("adr");
    let config_home = tmp.path().join("config");

    adroit(tmp.path())
        .args(["--dir", adr_dir.to_str().unwrap(), "new", "Test"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Created"));

    let config_file = config_home.join("adroit/config.yaml");
    assert!(config_file.exists());
    let content = std::fs::read_to_string(config_file).unwrap();
    assert!(content.contains("editor"));
}

#[test]
fn second_run_does_not_recreate_config() {
    let tmp = tempdir().unwrap();
    let adr_dir = tmp.path().join("adr");

    // First run — creates config
    adroit(tmp.path())
        .args(["--dir", adr_dir.to_str().unwrap(), "new", "First"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Created"));

    // Second run — no "Created" message
    adroit(tmp.path())
        .args(["--dir", adr_dir.to_str().unwrap(), "new", "Second"])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
}

// ── flags ─────────────────────────────────────────────────────────────

#[test]
fn version_flag() {
    let tmp = tempdir().unwrap();
    adroit(tmp.path()).arg("--version").assert().success();
}

#[test]
fn help_flag() {
    let tmp = tempdir().unwrap();
    adroit(tmp.path())
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Architecture Decision Records"));
}
