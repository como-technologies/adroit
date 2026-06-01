use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

/// Build a command pointed at an isolated temp ADR directory using the default
/// (markdown / by_status) profile.
fn adroit(dir: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("adroit").unwrap();
    cmd.arg("--dir").arg(dir.path());
    // Never block on an editor in tests.
    cmd.env("EDITOR", "true").env("VISUAL", "true");
    cmd
}

/// Build a command in the legacy flat + frontmatter profile.
fn adroit_flat(dir: &TempDir) -> Command {
    let mut cmd = adroit(dir);
    cmd.args(["--format", "frontmatter", "--layout", "flat"]);
    cmd
}

/// Recursively collect ADR markdown files (excluding README/template).
fn adr_files(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "md") {
                let name = p.file_name().unwrap().to_str().unwrap();
                if !name.eq_ignore_ascii_case("README.md")
                    && !name.eq_ignore_ascii_case("adr-template.md")
                {
                    out.push(p);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}

// ---------------------------------------------------------------------------
// Markdown / by_status (default) profile
// ---------------------------------------------------------------------------

#[test]
fn new_creates_markdown_adr_in_proposed_dir() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Use PostgreSQL", "--no-edit"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created"));

    let files = adr_files(dir.path());
    assert_eq!(files.len(), 1);
    let p = &files[0];
    assert!(p.parent().unwrap().ends_with("proposed"));
    assert!(
        p.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .ends_with("use-postgresql.md")
    );

    let content = fs::read_to_string(p).unwrap();
    assert!(content.starts_with("# ADR-0001: Use PostgreSQL\n"));
    assert!(content.contains("## Status"));
}

#[test]
fn list_shows_created_adrs() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "First decision", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["new", "Second decision", "--no-edit"])
        .assert()
        .success();

    adroit(&dir)
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("First decision"))
        .stdout(predicate::str::contains("Second decision"));
}

#[test]
fn list_filter_by_status() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Keep proposed", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["new", "Make accepted", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["status", "2", "accepted"])
        .assert()
        .success();

    adroit(&dir)
        .args(["list", "--status", "accepted"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Make accepted"))
        .stdout(predicate::str::contains("Keep proposed").not());
}

#[test]
fn status_moves_file_between_dirs() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Use Kafka", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["status", "1", "accepted"])
        .assert()
        .success();

    assert!(!dir.path().join("proposed/0001-use-kafka.md").exists());
    let accepted = dir.path().join("accepted/0001-use-kafka.md");
    assert!(accepted.exists());
    let content = fs::read_to_string(&accepted).unwrap();
    assert!(content.contains("## Status"));
    assert!(content.contains("Accepted"));
}

#[test]
fn supersede_moves_old_and_links_both() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Old way", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["new", "New way", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["status", "2", "accepted"])
        .assert()
        .success();

    adroit(&dir)
        .args(["supersede", "2", "1"])
        .assert()
        .success();

    let old = dir.path().join("superseded/0001-old-way.md");
    assert!(old.exists());
    let old_content = fs::read_to_string(&old).unwrap();
    assert!(old_content.contains("Superseded by [ADR-0002]"));

    let new = dir.path().join("accepted/0002-new-way.md");
    let new_content = fs::read_to_string(&new).unwrap();
    assert!(new_content.contains("Supersedes [ADR-0001]"));
}

#[test]
fn set_review_sets_and_clears_deadline_format_preserving() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Use Redis", "--no-edit"])
        .assert()
        .success();

    let path = dir.path().join("proposed/0001-use-redis.md");
    let before = fs::read_to_string(&path).unwrap();

    // Set a deadline: the `Review by:` line is written into the status region.
    adroit(&dir)
        .args(["set-review", "1", "2026-07-15"])
        .assert()
        .success()
        .stdout(predicate::str::contains("review deadline to 2026-07-15"));
    let after = fs::read_to_string(&path).unwrap();
    assert!(after.contains("Review by: 2026-07-15"));
    assert!(after.contains("## Status"));

    // Clearing removes the line and restores the original bytes.
    adroit(&dir)
        .args(["set-review", "1", "--clear"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Cleared"));
    let cleared = fs::read_to_string(&path).unwrap();
    assert!(!cleared.contains("Review by:"));
    assert_eq!(cleared, before);
}

#[test]
fn set_review_rejects_bad_date() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Use Redis", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["set-review", "1", "07/15/2026"])
        .assert()
        .failure();
}

#[test]
fn search_matches_title_and_body() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Adopt Postgres", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["new", "Adopt Redis", "--no-edit"])
        .assert()
        .success();

    adroit(&dir)
        .args(["search", "redis"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Adopt Redis"))
        .stdout(predicate::str::contains("Adopt Postgres").not());
}

#[test]
fn index_prints_when_no_summary() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "First", "--no-edit"])
        .assert()
        .success();

    adroit(&dir)
        .arg("index")
        .assert()
        .success()
        .stdout(predicate::str::contains("## Proposed"))
        .stdout(predicate::str::contains("ADR-0001: First"));
}

#[test]
fn index_regenerates_summary_preserving_header() {
    let parent = TempDir::new().unwrap();
    let adrs = parent.path().join("adrs");
    fs::create_dir_all(&adrs).unwrap();
    let summary = parent.path().join("SUMMARY.md");
    fs::write(
        &summary,
        "# Summary\n\n[Introduction](./README.md)\n\n# Architecture Decision Records\n\n- [ADR Process](./adrs/README.md)\n",
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("adroit").unwrap();
    cmd.arg("--dir").arg(&adrs);
    cmd.env("EDITOR", "true").env("VISUAL", "true");
    cmd.args(["new", "Repo Strategy", "--no-edit"])
        .assert()
        .success();

    let mut cmd = Command::cargo_bin("adroit").unwrap();
    cmd.arg("--dir").arg(&adrs);
    cmd.arg("index").assert().success();

    let out = fs::read_to_string(&summary).unwrap();
    assert!(out.contains("# Summary"));
    assert!(out.contains("- [ADR Process](./adrs/README.md)"));
    assert!(out.contains("## Proposed"));
    assert!(out.contains("[ADR-0001: Repo Strategy](./adrs/proposed/0001-repo-strategy.md)"));
}

// ---------------------------------------------------------------------------
// `check` — structural CI gate
// ---------------------------------------------------------------------------

#[test]
fn check_passes_on_clean_repo() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "First decision", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["new", "Second decision", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["status", "2", "accepted"])
        .assert()
        .success();

    adroit(&dir)
        .arg("check")
        .assert()
        .success()
        .stdout(predicate::str::contains("OK: 2 ADRs, no problems"));
}

#[test]
fn check_empty_repo_passes() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .arg("check")
        .assert()
        .success()
        .stdout(predicate::str::contains("OK: 0 ADRs"));
}

#[test]
fn check_fails_on_status_dir_mismatch() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Mismatched", "--no-edit"])
        .assert()
        .success();

    // The file lives in proposed/ but its `## Status` section says Accepted: a
    // directory <-> section disagreement that `check` must flag. Rewrite the
    // status value line specifically (the `> State:` banner above it is not part
    // of the `## Status` region the parser reads).
    let path = dir.path().join("proposed/0001-mismatched.md");
    let content = fs::read_to_string(&path).unwrap();
    let tampered = content.replacen("## Status\n\nProposed", "## Status\n\nAccepted", 1);
    assert_ne!(
        content, tampered,
        "test fixture must change the status word"
    );
    fs::write(&path, tampered).unwrap();

    adroit(&dir)
        .arg("check")
        .assert()
        .failure()
        .stderr(predicate::str::contains("directory says Proposed"))
        .stderr(predicate::str::contains("## Status says Accepted"));
}

#[test]
fn check_fails_on_broken_supersession_link() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Standing decision", "--no-edit"])
        .assert()
        .success();

    // Inject a "Superseded by ADR-0099" note pointing at a non-existent ADR.
    let path = dir.path().join("proposed/0001-standing-decision.md");
    let content = fs::read_to_string(&path).unwrap();
    let tampered = content.replacen("## Status", "## Status\n\nSuperseded by ADR-0099", 1);
    fs::write(&path, tampered).unwrap();

    adroit(&dir)
        .arg("check")
        .assert()
        .failure()
        .stderr(predicate::str::contains("ADR-0099"))
        .stderr(predicate::str::contains("no such ADR exists"));
}

#[test]
fn check_flat_frontmatter_skips_dir_checks() {
    // In flat/frontmatter there is no directory-implied status; check should
    // still run and pass on a clean repo.
    let dir = TempDir::new().unwrap();
    adroit_flat(&dir)
        .args(["new", "Flat decision", "--no-edit"])
        .assert()
        .success();
    adroit_flat(&dir)
        .arg("check")
        .assert()
        .success()
        .stdout(predicate::str::contains("OK: 1 ADRs"));
}

// ---------------------------------------------------------------------------
// `index --check` — SUMMARY.md drift gate
// ---------------------------------------------------------------------------

#[test]
fn index_check_passes_when_in_sync() {
    let parent = TempDir::new().unwrap();
    let adrs = parent.path().join("adrs");
    fs::create_dir_all(&adrs).unwrap();
    let summary = parent.path().join("SUMMARY.md");
    fs::write(
        &summary,
        "# Summary\n\n[Introduction](./README.md)\n\n# Architecture Decision Records\n\n- [ADR Process](./adrs/README.md)\n",
    )
    .unwrap();

    let new_cmd = || {
        let mut cmd = Command::cargo_bin("adroit").unwrap();
        cmd.arg("--dir").arg(&adrs);
        cmd.env("EDITOR", "true").env("VISUAL", "true");
        cmd
    };

    new_cmd()
        .args(["new", "Repo Strategy", "--no-edit"])
        .assert()
        .success();
    // Write the SUMMARY so it is in sync.
    new_cmd().arg("index").assert().success();

    new_cmd()
        .args(["index", "--check"])
        .assert()
        .success()
        .stdout(predicate::str::contains("SUMMARY.md is up to date"));
}

#[test]
fn index_check_fails_when_out_of_date() {
    let parent = TempDir::new().unwrap();
    let adrs = parent.path().join("adrs");
    fs::create_dir_all(&adrs).unwrap();
    let summary = parent.path().join("SUMMARY.md");
    fs::write(
        &summary,
        "# Summary\n\n[Introduction](./README.md)\n\n# Architecture Decision Records\n\n- [ADR Process](./adrs/README.md)\n",
    )
    .unwrap();

    let new_cmd = || {
        let mut cmd = Command::cargo_bin("adroit").unwrap();
        cmd.arg("--dir").arg(&adrs);
        cmd.env("EDITOR", "true").env("VISUAL", "true");
        cmd
    };

    new_cmd()
        .args(["new", "Repo Strategy", "--no-edit"])
        .assert()
        .success();
    new_cmd().arg("index").assert().success();

    // Change a status without re-indexing: SUMMARY.md is now stale.
    new_cmd()
        .args(["status", "1", "accepted"])
        .assert()
        .success();

    new_cmd()
        .args(["index", "--check"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("out of date"));

    // Re-indexing brings it back into sync.
    new_cmd().arg("index").assert().success();
    new_cmd().args(["index", "--check"]).assert().success();
}

#[test]
fn index_check_no_summary_exits_zero() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Lonely", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["index", "--check"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No SUMMARY.md found"));
}

#[test]
fn next_number_is_max_across_dirs() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "One", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["new", "Two", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["status", "2", "accepted"])
        .assert()
        .success();
    // Third should be 0003 even though 0002 moved dirs.
    adroit(&dir)
        .args(["new", "Three", "--no-edit"])
        .assert()
        .success();
    assert!(dir.path().join("proposed/0003-three.md").exists());
}

#[test]
fn show_displays_adr_details() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Use Redis", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["show", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Use Redis"));
}

#[test]
fn show_missing_adr_errors() {
    let dir = TempDir::new().unwrap();
    adroit(&dir).args(["show", "99"]).assert().failure();
}

#[test]
fn new_empty_title_errors() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "", "--no-edit"])
        .assert()
        .failure();
}

#[test]
fn status_invalid_errors() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Some ADR", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["status", "1", "bogus"])
        .assert()
        .failure();
}

#[test]
fn list_empty_dir_succeeds() {
    let dir = TempDir::new().unwrap();
    adroit(&dir).arg("list").assert().success();
}

#[test]
fn new_then_edit_with_fake_editor() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Editable decision", "--no-edit"])
        .assert()
        .success();
    adroit(&dir).args(["edit", "1"]).assert().success();
}

#[test]
fn review_generates_kickoff_doc() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Cluster Templates", "--no-edit"])
        .assert()
        .success();

    adroit(&dir)
        .args(["review", "1"])
        .assert()
        .success()
        // H1 with the ADR number.
        .stdout(predicate::str::contains("ADR-0001 Review Kickoff"))
        // The ADR title and number appear in the body.
        .stdout(predicate::str::contains("ADR-0001 (Cluster Templates)"))
        // The quorum line (default 3).
        .stdout(predicate::str::contains("3 team members must approve"))
        // The three Key-docs rows.
        .stdout(predicate::str::contains("[Read the ADR]"))
        .stdout(predicate::str::contains("[Read the README](../README.md)"))
        .stdout(predicate::str::contains(
            "[Read the guide](../../guides/adr-review-process.md)",
        ));
}

#[test]
fn review_writes_output_file() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Repo Strategy", "--no-edit"])
        .assert()
        .success();

    let out = dir.path().join("kickoff.md");
    adroit(&dir)
        .args(["review", "1", "--quorum", "5", "--days", "5"])
        .arg("--output")
        .arg(&out)
        .assert()
        .success()
        .stdout(predicate::str::contains("Created"));

    let content = fs::read_to_string(&out).unwrap();
    assert!(content.contains("ADR-0001 Review Kickoff"));
    assert!(content.contains("5 team members must approve"));
}

#[test]
fn review_missing_adr_errors() {
    let dir = TempDir::new().unwrap();
    adroit(&dir).args(["review", "99"]).assert().failure();
}

// ---------------------------------------------------------------------------
// Cross-ADR link integrity (relink on move, `relink`, `check`)
// ---------------------------------------------------------------------------

#[test]
fn status_change_relinks_inbound_links_and_check_passes() {
    let dir = TempDir::new().unwrap();
    let proposed = dir.path().join("proposed");
    fs::create_dir_all(&proposed).unwrap();
    // ADR-0001 links to ADR-0002 at its current (same-dir, proposed) location.
    fs::write(
        proposed.join("0001-use-postgres.md"),
        "# ADR-0001: Use Postgres\n\n## Status\n\nProposed\n\n## Context\n\nSee [ADR-0002](./0002-use-redis.md).\n",
    )
    .unwrap();
    fs::write(
        proposed.join("0002-use-redis.md"),
        "# ADR-0002: Use Redis\n\n## Status\n\nProposed\n\n## Context\n\nA cache.\n",
    )
    .unwrap();

    // Accepting ADR-0002 moves it to accepted/ AND rewrites ADR-0001's link.
    adroit(&dir)
        .args(["status", "2", "accepted"])
        .assert()
        .success();

    assert!(dir.path().join("accepted/0002-use-redis.md").exists());
    let one = fs::read_to_string(proposed.join("0001-use-postgres.md")).unwrap();
    assert!(
        one.contains("[ADR-0002](../accepted/0002-use-redis.md)"),
        "inbound link should be rewritten to the new dir, got:\n{one}"
    );

    // No broken/stale links remain.
    adroit(&dir).arg("check").assert().success();
}

#[test]
fn check_flags_broken_link_and_relink_repairs_it() {
    let dir = TempDir::new().unwrap();
    let proposed = dir.path().join("proposed");
    let accepted = dir.path().join("accepted");
    fs::create_dir_all(&proposed).unwrap();
    fs::create_dir_all(&accepted).unwrap();
    // 0002 lives in accepted/, but 0001 still links to a stale proposed/ path
    // (as if it had been moved outside adroit) — that target doesn't exist.
    fs::write(
        proposed.join("0001-a.md"),
        "# ADR-0001: A\n\n## Status\n\nProposed\n\n## Context\n\nSee [ADR-0002](../proposed/0002-b.md).\n",
    )
    .unwrap();
    fs::write(
        accepted.join("0002-b.md"),
        "# ADR-0002: B\n\n## Status\n\nAccepted\n",
    )
    .unwrap();

    // check fails: the link target file doesn't exist.
    adroit(&dir)
        .arg("check")
        .assert()
        .failure()
        .stderr(predicate::str::contains("broken link"));

    // relink repairs it to the canonical location.
    adroit(&dir)
        .arg("relink")
        .assert()
        .success()
        .stdout(predicate::str::contains("Relinked"));
    let one = fs::read_to_string(proposed.join("0001-a.md")).unwrap();
    assert!(
        one.contains("[ADR-0002](../accepted/0002-b.md)"),
        "got:\n{one}"
    );

    // check is now clean.
    adroit(&dir).arg("check").assert().success();
}

// ---------------------------------------------------------------------------
// `adroit renumber`
// ---------------------------------------------------------------------------

#[test]
fn renumber_renames_rewrites_heading_and_inbound_refs() {
    let dir = TempDir::new().unwrap();
    let proposed = dir.path().join("proposed");
    let accepted = dir.path().join("accepted");
    fs::create_dir_all(&proposed).unwrap();
    fs::create_dir_all(&accepted).unwrap();
    fs::write(
        proposed.join("0001-a.md"),
        "# ADR-0001: A\n\n## Status\n\nProposed\n\nSee [ADR-0002](../accepted/0002-b.md).\n",
    )
    .unwrap();
    fs::write(
        accepted.join("0002-b.md"),
        "# ADR-0002: B\n\n## Status\n\nAccepted\n",
    )
    .unwrap();

    adroit(&dir)
        .args(["renumber", "2", "5"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ADR-0002 -> ADR-0005"));

    assert!(accepted.join("0005-b.md").exists());
    assert!(!accepted.join("0002-b.md").exists());
    assert!(
        fs::read_to_string(accepted.join("0005-b.md"))
            .unwrap()
            .contains("# ADR-0005: B")
    );
    let a = fs::read_to_string(proposed.join("0001-a.md")).unwrap();
    assert!(a.contains("[ADR-0005](../accepted/0005-b.md)"), "got: {a}");
    adroit(&dir).arg("check").assert().success();
}

#[test]
fn renumber_resolves_duplicate_with_file_flag() {
    let dir = TempDir::new().unwrap();
    let proposed = dir.path().join("proposed");
    let accepted = dir.path().join("accepted");
    fs::create_dir_all(&proposed).unwrap();
    fs::create_dir_all(&accepted).unwrap();
    // Duplicate 0009 (different slugs) — the real-world collision.
    fs::write(
        proposed.join("0009-crossplane.md"),
        "# ADR-0009: Crossplane\n\n## Status\n\nProposed\n",
    )
    .unwrap();
    fs::write(
        accepted.join("0009-dex.md"),
        "# ADR-0009: Dex\n\n## Status\n\nAccepted\n",
    )
    .unwrap();

    // Ambiguous without --file.
    adroit(&dir)
        .args(["renumber", "9", "21"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("ambiguous"));

    adroit(&dir)
        .args([
            "renumber",
            "9",
            "21",
            "--file",
            proposed.join("0009-crossplane.md").to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(proposed.join("0021-crossplane.md").exists());
    assert!(
        accepted.join("0009-dex.md").exists(),
        "the other 0009 is untouched"
    );
    adroit(&dir).arg("check").assert().success();
}

// ---------------------------------------------------------------------------
// Profile mismatch guard + `migrate`
// ---------------------------------------------------------------------------

#[test]
fn mismatched_layout_refuses_to_run() {
    let dir = TempDir::new().unwrap();
    // by_status/markdown repo (the default).
    adroit(&dir)
        .args(["new", "One", "--no-edit"])
        .assert()
        .success();
    // Reading it as flat must refuse loudly (not silently show an empty list).
    adroit(&dir)
        .args(["--layout", "flat", "list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migrate"));
}

#[test]
fn migrate_converts_by_status_to_flat() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "One", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["new", "Two", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["status", "2", "accepted"])
        .assert()
        .success();
    assert!(dir.path().join("proposed/0001-one.md").exists());
    assert!(dir.path().join("accepted/0002-two.md").exists());

    // Preview only — re-run with --yes to apply; nothing changes yet.
    adroit(&dir)
        .args(["--layout", "flat", "migrate"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Preview only"));
    assert!(dir.path().join("proposed/0001-one.md").exists());

    // Apply.
    adroit(&dir)
        .args(["--layout", "flat", "migrate", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Migrated"));
    assert!(dir.path().join("0001-one.md").exists());
    assert!(dir.path().join("0002-two.md").exists());
    assert!(!dir.path().join("proposed/0001-one.md").exists());

    // Flat now reads them; the old default (by_status) config now mismatches.
    adroit(&dir)
        .args(["--layout", "flat", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("0002"));
    adroit(&dir).arg("list").assert().failure();
}

#[test]
fn migrate_markdown_to_frontmatter() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "One", "--no-edit"])
        .assert()
        .success();

    adroit(&dir)
        .args(["--format", "frontmatter", "migrate", "--yes"])
        .assert()
        .success();

    let content = fs::read_to_string(dir.path().join("proposed/0001-one.md")).unwrap();
    assert!(
        content.starts_with("---"),
        "file should now be frontmatter:\n{content}"
    );
    adroit(&dir)
        .args(["--format", "frontmatter", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("One"));
}

#[test]
fn migrate_dry_run_does_not_apply() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "One", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["--layout", "flat", "migrate", "--dry-run", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Dry run"));
    // --dry-run wins over --yes: nothing moved.
    assert!(dir.path().join("proposed/0001-one.md").exists());
    assert!(!dir.path().join("0001-one.md").exists());
}

#[test]
fn relink_dry_run_previews_without_writing() {
    let dir = TempDir::new().unwrap();
    let proposed = dir.path().join("proposed");
    let accepted = dir.path().join("accepted");
    fs::create_dir_all(&proposed).unwrap();
    fs::create_dir_all(&accepted).unwrap();
    let a = proposed.join("0001-a.md");
    fs::write(
        &a,
        "# ADR-0001: A\n\n## Status\n\nProposed\n\nSee [ADR-0002](../proposed/0002-b.md).\n",
    )
    .unwrap();
    fs::write(
        accepted.join("0002-b.md"),
        "# ADR-0002: B\n\n## Status\n\nAccepted\n",
    )
    .unwrap();
    let before = fs::read_to_string(&a).unwrap();

    adroit(&dir)
        .args(["relink", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Would relink"));
    assert_eq!(
        fs::read_to_string(&a).unwrap(),
        before,
        "dry run must not write"
    );

    adroit(&dir).arg("relink").assert().success();
    assert!(
        fs::read_to_string(&a)
            .unwrap()
            .contains("../accepted/0002-b.md")
    );
}

// ---------------------------------------------------------------------------
// `adroit config` (show / get / set)
// ---------------------------------------------------------------------------

#[test]
fn config_show_lists_keys_and_sources() {
    let dir = TempDir::new().unwrap();
    let xdg = TempDir::new().unwrap();
    adroit(&dir)
        .env("XDG_CONFIG_HOME", xdg.path())
        .arg("config")
        .assert()
        .success()
        .stdout(predicate::str::contains("SOURCE"))
        .stdout(predicate::str::contains("layout"))
        .stdout(predicate::str::contains("date_source"));
}

#[test]
fn config_get_reflects_flag_and_env() {
    let dir = TempDir::new().unwrap();
    let xdg = TempDir::new().unwrap();
    // A flag override is reflected.
    adroit(&dir)
        .env("XDG_CONFIG_HOME", xdg.path())
        .args(["--layout", "flat", "config", "get", "layout"])
        .assert()
        .success()
        .stdout(predicate::str::contains("flat"));
    // An env override is reflected.
    adroit(&dir)
        .env("XDG_CONFIG_HOME", xdg.path())
        .env("ADROIT_DATE_SOURCE", "git")
        .args(["config", "get", "date_source"])
        .assert()
        .success()
        .stdout(predicate::str::contains("git"));
}

#[test]
fn config_set_writes_config_yaml_and_round_trips() {
    let dir = TempDir::new().unwrap();
    let xdg = TempDir::new().unwrap();
    adroit(&dir)
        .env("XDG_CONFIG_HOME", xdg.path())
        .args(["config", "set", "review_overdue_days", "45"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Set review_overdue_days = 45"));
    adroit(&dir)
        .env("XDG_CONFIG_HOME", xdg.path())
        .args(["config", "get", "review_overdue_days"])
        .assert()
        .success()
        .stdout(predicate::str::contains("45"));
}

#[test]
fn config_set_local_writes_project_dotenv() {
    let adr_dir = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();
    let xdg = TempDir::new().unwrap();
    adroit(&adr_dir)
        .env("XDG_CONFIG_HOME", xdg.path())
        .current_dir(cwd.path())
        .args(["config", "set", "layout", "flat", "--local"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ADROIT_LAYOUT=flat"));
    let env = fs::read_to_string(cwd.path().join(".env")).unwrap();
    assert!(env.contains("ADROIT_LAYOUT=flat"), "got: {env}");
}

#[test]
fn config_set_rejects_bad_value_and_unknown_key() {
    let dir = TempDir::new().unwrap();
    let xdg = TempDir::new().unwrap();
    adroit(&dir)
        .env("XDG_CONFIG_HOME", xdg.path())
        .args(["config", "set", "layout", "sideways"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid"));
    adroit(&dir)
        .env("XDG_CONFIG_HOME", xdg.path())
        .args(["config", "set", "bogus", "x"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown config key"));
}

// ---------------------------------------------------------------------------
// Legacy flat + frontmatter profile (still supported)
// ---------------------------------------------------------------------------

#[test]
fn flat_new_creates_frontmatter_file() {
    let dir = TempDir::new().unwrap();
    adroit_flat(&dir)
        .args(["new", "Use PostgreSQL", "--no-edit"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created"));

    let files: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    assert_eq!(files.len(), 1);
    let content = fs::read_to_string(&files[0]).unwrap();
    assert!(content.starts_with("---\n"));
    assert!(content.contains("status: Proposed"));
}

#[test]
fn flat_full_workflow() {
    let dir = TempDir::new().unwrap();
    adroit_flat(&dir)
        .args(["new", "Initial decision", "--no-edit"])
        .assert()
        .success();
    adroit_flat(&dir)
        .args(["status", "1", "accepted"])
        .assert()
        .success();
    adroit_flat(&dir)
        .args(["show", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Accepted"));
    adroit_flat(&dir)
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("Initial decision"));
}

#[test]
fn dir_flag_overrides_default() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Scoped decision", "--no-edit"])
        .assert()
        .success();
    let alt = TempDir::new().unwrap();
    adroit(&alt).arg("list").assert().success();
}

// ---------------------------------------------------------------------------
// No subcommand -> interactive TUI (default build)
// ---------------------------------------------------------------------------

/// With no subcommand and a non-interactive stdin (as in CI / piped contexts —
/// `assert_cmd` runs the child with a non-TTY stdin), adroit must NOT try to
/// seize a real terminal: it prints a short hint and exits 0. This exercises
/// exactly that path so the test can never hang waiting on a terminal.
///
/// The hint differs slightly between a `tui`-enabled build ("requires an
/// interactive terminal") and a no-`tui` build ("built without the `tui`
/// feature"), but both steer the user to the CLI subcommands — assert on that
/// shared cue so the test passes under either feature set.
#[test]
fn no_args_without_tty_prints_hint_and_exits_zero() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("CLI subcommands"));
}

// ── global --dir / env var (regression: --dir must work after a subcommand) ──

#[test]
fn dir_flag_works_after_subcommand() {
    let dir = TempDir::new().unwrap();

    // Seed one ADR (the `adroit` helper passes --dir before the subcommand).
    adroit(&dir)
        .args(["new", "First decision", "--no-edit"])
        .assert()
        .success();

    // The global flag must also be accepted AFTER the subcommand. Build a raw
    // command (the helper already injects --dir, so use it directly here).
    let mut cmd = Command::cargo_bin("adroit").unwrap();
    cmd.args(["list", "--dir", dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("First decision"));

    // ...and the short form too.
    let mut cmd = Command::cargo_bin("adroit").unwrap();
    cmd.args(["list", "-d", dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("First decision"));
}

#[test]
fn adroit_dir_env_var_sets_directory() {
    let dir = TempDir::new().unwrap();

    adroit(&dir)
        .args(["new", "Env decision", "--no-edit"])
        .assert()
        .success();

    // No --dir flag: the directory comes from the ADROIT_DIR env var.
    let mut cmd = Command::cargo_bin("adroit").unwrap();
    cmd.env("ADROIT_DIR", dir.path().to_str().unwrap())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("Env decision"));
}

// ---------------------------------------------------------------------------
// Naming schemes (date / uuid) end-to-end through the naming seam
// ---------------------------------------------------------------------------

/// A command in the date naming scheme (markdown / by_status profile).
fn adroit_date(dir: &TempDir) -> Command {
    let mut cmd = adroit(dir);
    cmd.args(["--naming", "date"]);
    cmd
}

/// The filename stem (no `.md`) of the single ADR in the store.
fn sole_stem(root: &Path) -> String {
    let files = adr_files(root);
    assert_eq!(files.len(), 1, "expected exactly one ADR");
    files[0]
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .strip_suffix(".md")
        .unwrap()
        .to_string()
}

#[test]
fn date_scheme_new_uses_date_slug_and_plain_heading() {
    let dir = TempDir::new().unwrap();
    adroit_date(&dir)
        .args(["new", "Adopt PostgreSQL", "--no-edit"])
        .assert()
        .success();

    let files = adr_files(dir.path());
    assert_eq!(files.len(), 1);
    let p = &files[0];
    assert!(p.parent().unwrap().ends_with("proposed"));
    let name = p.file_name().unwrap().to_str().unwrap();
    // `YYYYMMDD-<slug>.md` — 8 leading digits then the title slug.
    assert!(name.ends_with("-adopt-postgresql.md"), "got {name}");
    let digits: String = name.chars().take_while(|c| c.is_ascii_digit()).collect();
    assert_eq!(digits.len(), 8, "expected an 8-digit date prefix in {name}");

    let content = fs::read_to_string(p).unwrap();
    // Slug schemes carry identity in the filename, so the heading is plain.
    assert!(
        content.starts_with("# Adopt PostgreSQL\n"),
        "got: {content}"
    );
    assert!(!content.contains("# ADR-"));
}

#[test]
fn date_scheme_list_show_status_by_slug() {
    let dir = TempDir::new().unwrap();
    adroit_date(&dir)
        .args(["new", "Adopt PostgreSQL", "--no-edit"])
        .assert()
        .success();
    let slug = sole_stem(dir.path());

    // The list row shows the date slug as the identifier.
    adroit_date(&dir)
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains(&slug))
        .stdout(predicate::str::contains("Adopt PostgreSQL"));

    // `show <slug>` resolves through the naming seam.
    adroit_date(&dir)
        .args(["show", &slug])
        .assert()
        .success()
        .stdout(predicate::str::contains("Adopt PostgreSQL"));

    // `status <slug> accepted` moves the file to accepted/ keeping its slug.
    adroit_date(&dir)
        .args(["status", &slug, "accepted"])
        .assert()
        .success();
    let moved = &adr_files(dir.path())[0];
    assert!(moved.parent().unwrap().ends_with("accepted"));
    assert_eq!(sole_stem(dir.path()), slug);
}

#[test]
fn date_scheme_set_review_by_slug() {
    let dir = TempDir::new().unwrap();
    adroit_date(&dir)
        .args(["new", "Adopt PostgreSQL", "--no-edit"])
        .assert()
        .success();
    let slug = sole_stem(dir.path());
    let path = adr_files(dir.path())[0].clone();

    adroit_date(&dir)
        .args(["set-review", &slug, "2026-12-31"])
        .assert()
        .success();
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("Review by: 2026-12-31"), "got: {content}");
}

#[test]
fn date_scheme_check_flags_duplicate_slug() {
    let dir = TempDir::new().unwrap();
    adroit_date(&dir)
        .args(["new", "Adopt PostgreSQL", "--no-edit"])
        .assert()
        .success();
    let p = adr_files(dir.path())[0].clone();
    let slug = sole_stem(dir.path());

    // Plant a colliding copy with the same date slug in another status dir —
    // `check` must flag it even though the date scheme has no number.
    let accepted = dir.path().join("accepted");
    fs::create_dir_all(&accepted).unwrap();
    fs::copy(&p, accepted.join(format!("{slug}.md"))).unwrap();

    adroit_date(&dir)
        .arg("check")
        .assert()
        .failure()
        .stderr(predicate::str::contains("duplicate identifier"))
        .stderr(predicate::str::contains(&slug));
}

#[test]
fn date_scheme_rejects_numeric_only_commands() {
    let dir = TempDir::new().unwrap();
    adroit_date(&dir)
        .args(["new", "Adopt PostgreSQL", "--no-edit"])
        .assert()
        .success();

    // supersede / renumber / review are number-shaped and don't apply to a
    // non-numeric scheme — they bail with a clear message, not a "not found".
    adroit_date(&dir)
        .args(["supersede", "2", "1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("requires a numeric naming scheme"));
    adroit_date(&dir)
        .args(["renumber", "1", "2"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("requires a numeric naming scheme"));
}

#[test]
fn uuid_scheme_new_and_show_by_prefix() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["--naming", "uuid", "new", "Adopt PostgreSQL", "--no-edit"])
        .assert()
        .success();

    let name_stem = sole_stem(dir.path());
    // `<32-hex-uuid>-<slug>` — the uuid is the identity, the slug is for humans.
    let uuid: String = name_stem.chars().take_while(|c| *c != '-').collect();
    assert_eq!(
        uuid.len(),
        32,
        "expected a 32-char uuid prefix in {name_stem}"
    );

    // Addressable by a leading prefix of the uuid (what `list`/display shows).
    let prefix = &uuid[..8];
    adroit(&dir)
        .args(["--naming", "uuid", "show", prefix])
        .assert()
        .success()
        .stdout(predicate::str::contains("Adopt PostgreSQL"));
}
