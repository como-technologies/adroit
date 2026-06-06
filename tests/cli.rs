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

/// Snapshot every file under `root` (relative path → bytes), including
/// `SUMMARY.md`. Used by the idempotency guards for byte-identical before/after
/// comparisons of the whole tree.
fn snapshot(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    fn walk(dir: &Path, root: &Path, out: &mut std::collections::BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk(&p, root, out);
            } else {
                let rel = p.strip_prefix(root).unwrap().to_path_buf();
                out.insert(rel, fs::read(&p).unwrap());
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    walk(root, root, &mut out);
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
        .args(["set-status", "2", "accepted"])
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
        .args(["set-status", "1", "accepted"])
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
fn status_getter_prints_lowercase_and_round_trips_into_set_status() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "A decision", "--no-edit"])
        .assert()
        .success();

    // `status <ID>` is a getter: just the status word, lowercase (scriptable).
    adroit(&dir)
        .args(["status", "1"])
        .assert()
        .success()
        .stdout("proposed\n");

    // ...and it feeds straight back into `set-status`.
    adroit(&dir)
        .args(["set-status", "1", "accepted"])
        .assert()
        .success();
    adroit(&dir)
        .args(["status", "1"])
        .assert()
        .success()
        .stdout("accepted\n");
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
        .args(["set-status", "2", "accepted"])
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

/// Regression (hardening blitz, model-based oracle): when the *newer* ADR is
/// itself already in `superseded/`, `supersede` adds the reciprocal
/// "Supersedes [..]" note with a **same-directory** link. That link must be in
/// the canonical `./` form `relink` produces, so the repo stays link-canonical
/// and a follow-up `relink` is a no-op (the documented invariant). Previously the
/// note was written as a bare `0002-beta.md` (no `./`), so `relink` would then
/// rewrite it — leaving `supersede` output non-canonical.
#[test]
fn supersede_when_new_is_already_superseded_leaves_links_canonical() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Alpha", "--no-edit"])
        .assert()
        .success(); // ADR-0001
    adroit(&dir)
        .args(["new", "Beta", "--no-edit"])
        .assert()
        .success(); // ADR-0002
    // Move ADR-0001 into superseded/ via a plain status change.
    adroit(&dir)
        .args(["set-status", "1", "superseded"])
        .assert()
        .success();
    // Supersede ADR-0002 by ADR-0001 — both now live in superseded/.
    adroit(&dir)
        .args(["supersede", "1", "2"])
        .assert()
        .success();

    // The reciprocal note's same-dir link is canonical (`./`).
    let new = fs::read_to_string(dir.path().join("superseded/0001-alpha.md")).unwrap();
    assert!(
        new.contains("Supersedes [ADR-0002](./0002-beta.md)"),
        "reciprocal note must use the canonical ./ link form, got:\n{new}"
    );

    // And the repo is link-canonical: a relink dry-run rewrites nothing.
    adroit(&dir)
        .args(["relink", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("canonical"));
}

/// Regression (hardening blitz, model-based oracle): superseding an ADR that is
/// **already** in `superseded/` (the old ADR doesn't move) must still write a
/// canonical `## Status` "Superseded by [..]" link. The old side's link is
/// produced by `Store::supersede`, which previously only canonicalized links when
/// the file moved dirs — so re-superseding in place left a bare `0002-x.md` link
/// (no `./`), and the repo was no longer link-canonical.
#[test]
fn supersede_in_place_writes_canonical_status_link() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Alpha", "--no-edit"])
        .assert()
        .success(); // ADR-0001
    adroit(&dir)
        .args(["new", "Beta", "--no-edit"])
        .assert()
        .success(); // ADR-0002
    // Put BOTH ADRs in superseded/ so the supersede target doesn't move.
    adroit(&dir)
        .args(["set-status", "1", "superseded"])
        .assert()
        .success();
    adroit(&dir)
        .args(["set-status", "2", "superseded"])
        .assert()
        .success();
    // Supersede ADR-0001 by ADR-0002 — old is already in superseded/ (no move).
    adroit(&dir)
        .args(["supersede", "2", "1"])
        .assert()
        .success();

    let old = fs::read_to_string(dir.path().join("superseded/0001-alpha.md")).unwrap();
    assert!(
        old.contains("Superseded by [ADR-0002](./0002-beta.md)"),
        "in-place supersede must write the canonical ./ status link, got:\n{old}"
    );
    adroit(&dir)
        .args(["relink", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("canonical"));
}

/// Regression (hardening blitz, full-matrix oracle): under the `uuid` naming
/// scheme, a supersession link (`…/{uuid}-{slug}.md`) must resolve back to the
/// ADR whose identity is the bare `{uuid}`. `ref_in_link` previously returned the
/// whole filename stem (`{uuid}-{slug}`), so `adroit check` reported the
/// supersession as "no such ADR exists" and exited non-zero — uuid supersede
/// produced a repo that failed its own validation.
#[test]
fn uuid_scheme_supersede_passes_check() {
    let dir = TempDir::new().unwrap();
    let scheme = ["--naming", "uuid"];
    adroit(&dir)
        .args(scheme)
        .args(["new", "Alpha", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(scheme)
        .args(["new", "Beta", "--no-edit"])
        .assert()
        .success();

    // Recover the two uuids from the filenames (`{uuid}-{slug}.md`).
    let ids: Vec<String> = adr_files(dir.path())
        .iter()
        .map(|p| {
            let name = p.file_name().unwrap().to_str().unwrap();
            name.split('-').next().unwrap().to_string()
        })
        .collect();
    assert_eq!(ids.len(), 2, "expected two ADRs, got {ids:?}");

    adroit(&dir)
        .args(scheme)
        .args(["supersede", &ids[0], &ids[1]])
        .assert()
        .success();

    // The superseded ADR's link must resolve, so `check` passes and `relink` is
    // a no-op (the repo is consistent).
    adroit(&dir).args(scheme).args(["check"]).assert().success();
    adroit(&dir)
        .args(scheme)
        .args(["relink", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("canonical"));
}

/// Regression (hardening blitz, full-matrix oracle): the `frontmatter` format is
/// numeric-only, so combining it with a slug-based naming scheme (date / uuid /
/// per_category) must be refused **up front** with a clear message — previously it
/// failed deep in the write path with a cryptic "ADR number must be assigned
/// before serializing" error.
#[test]
fn frontmatter_rejects_slug_naming_with_clear_error() {
    for scheme in ["date", "uuid"] {
        let dir = TempDir::new().unwrap();
        adroit(&dir)
            .args(["--format", "frontmatter", "--naming", scheme])
            .args(["new", "Hello", "--no-edit"])
            .assert()
            .failure()
            .stderr(
                predicate::str::contains("frontmatter").and(predicate::str::contains("sequential")),
            );
    }
}

/// Regression (hardening blitz, full-matrix oracle): under `by_category`,
/// `supersede` must write the supersession link relative to where the **old** ADR
/// ends up — and in `by_category` it stays in its category dir (it does not move
/// to `superseded/`). The link was computed relative to the superseded dir
/// unconditionally, producing a `./<category>/<file>` path (a spurious category
/// segment) pointing at a nonexistent file. Cross-category links carry the
/// category, so they now resolve and `check` passes.
///
/// (The *same-category* case — `ref_in_link` recovering the per_category identity
/// from a category-less `./<file>` link — is now also fixed; see
/// `per_category_same_category_supersede_passes_check`.)
#[test]
fn per_category_cross_category_supersede_passes_check() {
    let dir = TempDir::new().unwrap();
    let prof = ["--layout", "by_category", "--naming", "per_category"];
    adroit(&dir)
        .args(prof)
        .args(["new", "Alpha", "--category", "data", "--no-edit"])
        .assert()
        .success(); // data/0001
    adroit(&dir)
        .args(prof)
        .args(["new", "Beta", "--category", "infra", "--no-edit"])
        .assert()
        .success(); // infra/0001
    adroit(&dir)
        .args(prof)
        .args(["supersede", "infra/0001", "data/0001"])
        .assert()
        .success();

    // The superseded ADR's link resolves: `check` passes and `relink` is a no-op.
    adroit(&dir).args(prof).args(["check"]).assert().success();
    adroit(&dir)
        .args(prof)
        .args(["relink", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("canonical"));
}

/// Regression (hardening blitz, #9): a *same-category* per_category supersede must
/// also pass `check`. Its link (`./0002-x.md`) carries no category segment, so
/// resolution falls back to the source file's category (`ref_in_link_from`).
/// Previously `check` falsely reported the supersession as broken.
#[test]
fn per_category_same_category_supersede_passes_check() {
    let dir = TempDir::new().unwrap();
    let prof = ["--layout", "by_category", "--naming", "per_category"];
    for title in ["Alpha", "Beta"] {
        adroit(&dir)
            .args(prof)
            .args(["new", title, "--category", "data", "--no-edit"])
            .assert()
            .success();
    }
    adroit(&dir)
        .args(prof)
        .args(["supersede", "data/0002", "data/0001"])
        .assert()
        .success();
    adroit(&dir).args(prof).args(["check"]).assert().success();
    adroit(&dir)
        .args(prof)
        .args(["relink", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("canonical"));
}

/// Regression (hardening blitz, #8 check-half): `check` validates frontmatter
/// supersession refs (the YAML `superseded_by:` field), not just markdown
/// `## Status` links. This is the backstop that keeps a dangling pointer from
/// being silent — e.g. when the target ADR is removed out-of-band. (`renumber`
/// itself no longer strands these; it remaps the YAML ref — see
/// `renumber_rewrites_frontmatter_supersession_ref`.)
#[test]
fn frontmatter_check_flags_stranded_supersession() {
    let dir = TempDir::new().unwrap();
    let fm = ["--format", "frontmatter"];
    adroit(&dir)
        .args(fm)
        .args(["new", "Alpha", "--no-edit"])
        .assert()
        .success(); // ADR-1
    adroit(&dir)
        .args(fm)
        .args(["new", "Beta", "--no-edit"])
        .assert()
        .success(); // ADR-2
    adroit(&dir)
        .args(fm)
        .args(["supersede", "2", "1"])
        .assert()
        .success(); // ADR-1 superseded_by: 2
    adroit(&dir).args(fm).args(["check"]).assert().success();

    // Remove ADR-2 out-of-band, stranding ADR-1's `superseded_by: 2`. The check
    // rule resolves the bare-number YAML ref against the identity set and must
    // flag it as a broken supersession.
    fs::remove_file(dir.path().join("proposed/0002-beta.md")).unwrap();
    adroit(&dir)
        .args(fm)
        .args(["check"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no such ADR exists"));
}

/// Regression (hardening blitz #2/#12): under a slug scheme, a *stale* cross-ADR
/// link (the ADR merely moved) must be a `check` **warning**, not a broken-link
/// **error** — so a `relink_scope=self` deferred branch still passes `check`.
/// Check #5 was numeric-only; for date/uuid/per_category it couldn't tell "ADR
/// moved" (stale) from "no such ADR" (broken) and wrongly failed the repo.
#[test]
fn date_scheme_stale_link_passes_check_under_relink_scope_self() {
    let dir = TempDir::new().unwrap();
    let prof = ["--naming", "date", "--relink-scope", "self"];
    let run = |args: &[&str]| {
        adroit(&dir)
            .env("ADROIT_TODAY", "2026-06-04")
            .args(prof)
            .args(args)
            .assert()
    };
    run(&["new", "Alpha", "--no-edit"]).success(); // 20260604-alpha
    run(&["new", "Beta", "--no-edit"]).success(); // 20260604-beta
    // Alpha superseded by Beta → Alpha moves to superseded/ with a link to Beta
    // (still in proposed/ at this point).
    run(&["supersede", "20260604-beta", "20260604-alpha"]).success();
    // Move Beta to accepted/. With relink_scope=self, Alpha's inbound link to Beta
    // is intentionally left stale (deferred to a later full `relink`).
    run(&["set-status", "20260604-beta", "accepted"]).success();
    // The stale link must be a warning, so `check` still passes (exit 0).
    run(&["check"]).success();
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
        .args(["set-status", "2", "accepted"])
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
        .args(["set-status", "1", "accepted"])
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
        .args(["set-status", "2", "accepted"])
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
        .args(["set-status", "1", "bogus"])
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
        .arg("--out")
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
        .args(["set-status", "2", "accepted"])
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
fn check_warns_on_stale_link_and_relink_repairs_it() {
    let dir = TempDir::new().unwrap();
    let proposed = dir.path().join("proposed");
    let accepted = dir.path().join("accepted");
    fs::create_dir_all(&proposed).unwrap();
    fs::create_dir_all(&accepted).unwrap();
    // 0002 lives in accepted/, but 0001 still links to a stale proposed/ path
    // (as if 0002 had been moved outside adroit, or by a deferred-relink PR).
    // The literal target file is gone, but ADR-0002 still exists — so this is a
    // STALE link a `relink` heals, NOT a hard error.
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

    // check SUCCEEDS — the stale link is a warning, not an error — but reports it.
    adroit(&dir)
        .arg("check")
        .assert()
        .success()
        .stderr(predicate::str::contains("stale link"));

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

    // check is now fully clean.
    adroit(&dir)
        .arg("check")
        .assert()
        .success()
        .stdout(predicate::str::contains("no problems"));
}

#[test]
fn check_fails_on_dangling_link_to_unknown_adr() {
    let dir = TempDir::new().unwrap();
    let proposed = dir.path().join("proposed");
    fs::create_dir_all(&proposed).unwrap();
    // ADR-0001 links to ADR-0099, which exists nowhere in the repo — a truly
    // dangling link that points at no ADR. This stays a hard error (so genuine
    // breakage still fails CI even though stale links are now warnings).
    fs::write(
        proposed.join("0001-a.md"),
        "# ADR-0001: A\n\n## Status\n\nProposed\n\n## Context\n\nSee [ADR-0099](./0099-ghost.md).\n",
    )
    .unwrap();

    adroit(&dir)
        .arg("check")
        .assert()
        .failure()
        .stderr(predicate::str::contains("broken link"));
}

#[test]
fn self_scope_status_change_defers_inbound_relink_to_explicit_relink() {
    let dir = TempDir::new().unwrap();
    let proposed = dir.path().join("proposed");
    fs::create_dir_all(&proposed).unwrap();
    // Two cross-linked ADRs in proposed/.
    let one_seed =
        "# ADR-0001: A\n\n## Status\n\nProposed\n\n## Context\n\nSee [ADR-0002](./0002-b.md).\n";
    fs::write(proposed.join("0001-a.md"), one_seed).unwrap();
    fs::write(
        proposed.join("0002-b.md"),
        "# ADR-0002: B\n\n## Status\n\nProposed\n\n## Context\n\nRelated to [ADR-0001](./0001-a.md).\n",
    )
    .unwrap();

    // Accept 0002 with self-scope: it moves to accepted/ and fixes ITS OWN link,
    // but must NOT rewrite the inbound link in its neighbor 0001.
    adroit(&dir)
        .args(["--relink-scope", "self", "set-status", "2", "accepted"])
        .assert()
        .success();

    assert!(dir.path().join("accepted/0002-b.md").exists());
    assert!(!proposed.join("0002-b.md").exists());

    // Neighbor 0001 is byte-identical to its seed — a status-change PR under
    // self-scope touches only the ADR it is about, so two decision PRs never
    // collide on a shared neighbor.
    let one = fs::read_to_string(proposed.join("0001-a.md")).unwrap();
    assert_eq!(one, one_seed, "neighbor must be untouched, got:\n{one}");

    // The moved file's OWN outbound link was fixed (it stays internally valid).
    let two = fs::read_to_string(dir.path().join("accepted/0002-b.md")).unwrap();
    assert!(
        two.contains("[ADR-0001](../proposed/0001-a.md)"),
        "moved file's own link should be fixed, got:\n{two}"
    );

    // check still passes: 0001's now-stale inbound link is a warning, not an error.
    adroit(&dir)
        .arg("check")
        .assert()
        .success()
        .stderr(predicate::str::contains("stale link"));

    // The post-merge `adroit relink` (always full-scope) heals the deferred
    // inbound link — the "heal-on-main" step.
    adroit(&dir)
        .arg("relink")
        .assert()
        .success()
        .stdout(predicate::str::contains("Relinked"));
    let one = fs::read_to_string(proposed.join("0001-a.md")).unwrap();
    assert!(
        one.contains("[ADR-0002](../accepted/0002-b.md)"),
        "explicit relink should heal the inbound link, got:\n{one}"
    );
    adroit(&dir)
        .arg("check")
        .assert()
        .success()
        .stdout(predicate::str::contains("no problems"));
}

#[test]
fn duplicate_number_fails_check() {
    let dir = TempDir::new().unwrap();
    let proposed = dir.path().join("proposed");
    let accepted = dir.path().join("accepted");
    fs::create_dir_all(&proposed).unwrap();
    fs::create_dir_all(&accepted).unwrap();
    // Two ADRs share number 0009 — the collision two branches produce on merge.
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

    // The merge-queue gate: a duplicate number is an ERROR (not a warning), so
    // `adroit check` fails — ejecting the second colliding PR from the queue.
    adroit(&dir)
        .arg("check")
        .assert()
        .failure()
        .stderr(predicate::str::contains("duplicate number"));
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

#[test]
fn renumber_rewrites_frontmatter_supersession_ref() {
    // In the frontmatter profile, supersession is a bare-number YAML field
    // (`superseded_by: N`), not a markdown link. Renumbering the *superseding*
    // ADR must retarget that inbound ref so it isn't stranded (the markdown
    // profile heals the equivalent `## Status` link via relabeling).
    let dir = TempDir::new().unwrap();
    adroit_flat(&dir)
        .args(["new", "First", "--no-edit"])
        .assert()
        .success();
    adroit_flat(&dir)
        .args(["new", "Second", "--no-edit"])
        .assert()
        .success();
    // ADR 2 supersedes ADR 1 -> ADR 1's YAML gains `superseded_by: 2`.
    adroit_flat(&dir)
        .args(["supersede", "2", "1"])
        .assert()
        .success();
    let one = dir.path().join("0001-first.md");
    assert!(
        fs::read_to_string(&one)
            .unwrap()
            .contains("superseded_by: 2"),
        "precondition: ADR 1 records the supersession"
    );

    // Renumber the superseding ADR 2 -> 9.
    adroit_flat(&dir)
        .args(["renumber", "2", "9"])
        .assert()
        .success();

    let one_after = fs::read_to_string(&one).unwrap();
    assert!(
        one_after.contains("superseded_by: 9"),
        "the inbound frontmatter ref must follow the renumber:\n{one_after}"
    );
    assert!(
        !one_after.contains("superseded_by: 2"),
        "the stranded ref must be gone:\n{one_after}"
    );
    // No stranded supersession -> `check` is clean.
    adroit_flat(&dir).arg("check").assert().success();
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
        .args(["set-status", "2", "accepted"])
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

// ---------------------------------------------------------------------------
// Statelessness / idempotency invariant
//
// Guards the design principle (CLAUDE.md "Design principles", book dev/design.md):
// the only state is the filesystem, and every converge-style verb is idempotent —
// re-running it on an unchanged tree is a byte-for-byte no-op.
// ---------------------------------------------------------------------------

#[test]
fn commands_are_idempotent() {
    let dir = TempDir::new().unwrap();
    let run = |args: &[&str]| adroit(&dir).args(args).assert().success();

    // A small repo with a status change, a supersession, a review deadline, and a
    // regenerated index — so relink/index/etc. all have real work to (not) redo.
    run(&["new", "First", "--no-edit"]);
    run(&["new", "Second", "--no-edit"]);
    run(&["new", "Third", "--no-edit"]);
    run(&["new", "Fourth", "--no-edit"]);
    run(&["new", "Fifth", "--no-edit"]);

    // The converge-style verbs: each asserts a desired state. Distinct ADRs per
    // verb so the *first* loop pass below is already a no-op (1 is accepted, 4
    // supersedes 5, 2 has a deadline — none conflict).
    let converge: &[&[&str]] = &[
        &["set-status", "1", "accepted"],
        &["supersede", "4", "5"],
        &["set-review", "2", "2030-01-01"],
        &["index"],
        &["relink"],
    ];
    for argv in converge {
        run(argv);
    }

    // Re-running every converge verb on the now-canonical tree must change
    // nothing — same files, byte-identical contents (incl. SUMMARY.md).
    let before = snapshot(dir.path());
    for argv in converge {
        run(argv);
    }
    let after = snapshot(dir.path());
    assert_eq!(
        before, after,
        "re-running converge-style verbs must be a byte-for-byte no-op"
    );
}

#[test]
fn migrate_is_idempotent_at_fixpoint() {
    let dir = TempDir::new().unwrap();
    let run = |args: &[&str]| adroit(&dir).args(args).assert().success();
    run(&["new", "One", "--no-edit"]);
    run(&["new", "Two", "--no-edit"]);
    run(&["set-status", "2", "accepted"]);

    // Converge to the flat layout.
    run(&["--layout", "flat", "migrate", "--yes"]);
    let before = snapshot(dir.path());

    // A second migrate to the same target is a no-op: it reports nothing to do
    // and leaves every file byte-identical.
    adroit(&dir)
        .args(["--layout", "flat", "migrate", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("nothing to migrate"));
    assert_eq!(
        before,
        snapshot(dir.path()),
        "re-running migrate at its fixpoint must change nothing"
    );
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
        .args(["set-status", "1", "accepted"])
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
// per_category naming + by_category layout (MADR-style category folders)
// ---------------------------------------------------------------------------

/// A command in the by_category layout + per_category naming scheme.
fn adroit_category(dir: &TempDir) -> Command {
    let mut cmd = adroit(dir);
    cmd.args(["--layout", "by_category", "--naming", "per_category"]);
    cmd
}

#[test]
fn per_category_numbers_locally_per_directory() {
    let dir = TempDir::new().unwrap();
    adroit_category(&dir)
        .args(["new", "Use Postgres", "--category", "data", "--no-edit"])
        .assert()
        .success();
    adroit_category(&dir)
        .args(["new", "Use Kafka", "--category", "data", "--no-edit"])
        .assert()
        .success();
    adroit_category(&dir)
        .args(["new", "Use Terraform", "--category", "infra", "--no-edit"])
        .assert()
        .success();

    // Numbering is local to each category: data has 0001/0002, infra has 0001.
    assert!(dir.path().join("data/0001-use-postgres.md").exists());
    assert!(dir.path().join("data/0002-use-kafka.md").exists());
    assert!(dir.path().join("infra/0001-use-terraform.md").exists());
    // The heading carries the local number.
    let infra = fs::read_to_string(dir.path().join("infra/0001-use-terraform.md")).unwrap();
    assert!(
        infra.starts_with("# ADR-0001: Use Terraform\n"),
        "got: {infra}"
    );
}

#[test]
fn per_category_requires_category_flag() {
    let dir = TempDir::new().unwrap();
    adroit_category(&dir)
        .args(["new", "No category", "--no-edit"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("requires `--category"));
}

#[test]
fn per_category_show_and_status_by_composite_id() {
    let dir = TempDir::new().unwrap();
    adroit_category(&dir)
        .args(["new", "Use Postgres", "--category", "data", "--no-edit"])
        .assert()
        .success();
    adroit_category(&dir)
        .args(["new", "Use Terraform", "--category", "infra", "--no-edit"])
        .assert()
        .success();

    // Address by `category/NNNN` (accepts an unpadded number too).
    adroit_category(&dir)
        .args(["show", "data/1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("data/0001: Use Postgres"));

    // A status change stays in the category directory (no move).
    adroit_category(&dir)
        .args(["set-status", "data/0001", "accepted"])
        .assert()
        .success();
    assert!(
        dir.path().join("data/0001-use-postgres.md").exists(),
        "file stays in its category dir on a status change"
    );
    let body = fs::read_to_string(dir.path().join("data/0001-use-postgres.md")).unwrap();
    assert!(body.contains("Accepted"));

    adroit_category(&dir).arg("check").assert().success();
}

#[test]
fn per_category_list_shows_composite_ids() {
    let dir = TempDir::new().unwrap();
    adroit_category(&dir)
        .args(["new", "Use Postgres", "--category", "data", "--no-edit"])
        .assert()
        .success();
    adroit_category(&dir)
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("data/0001"))
        .stdout(predicate::str::contains("Use Postgres"));
}

// ---------------------------------------------------------------------------
// Typed relational links (`adroit link`, frontmatter profile)
// ---------------------------------------------------------------------------

#[test]
fn link_adds_typed_relation_in_frontmatter() {
    let dir = TempDir::new().unwrap();
    adroit_flat(&dir)
        .args(["new", "Base", "--no-edit"])
        .assert()
        .success();
    adroit_flat(&dir)
        .args(["new", "Dependent", "--no-edit"])
        .assert()
        .success();

    adroit_flat(&dir)
        .args(["link", "2", "--depends-on", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("depends on"));

    let body = fs::read_to_string(dir.path().join("0002-dependent.md")).unwrap();
    assert!(
        body.contains("depends_on:"),
        "frontmatter records the link: {body}"
    );

    adroit_flat(&dir)
        .args(["show", "2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Depends on: ADR-0001"));

    // --remove takes it back out.
    adroit_flat(&dir)
        .args(["link", "2", "--depends-on", "1", "--remove"])
        .assert()
        .success();
    let after = fs::read_to_string(dir.path().join("0002-dependent.md")).unwrap();
    assert!(!after.contains("depends_on:"));
}

#[test]
fn link_to_missing_adr_errors() {
    let dir = TempDir::new().unwrap();
    adroit_flat(&dir)
        .args(["new", "Base", "--no-edit"])
        .assert()
        .success();
    adroit_flat(&dir)
        .args(["link", "1", "--relates-to", "99"])
        .assert()
        .failure();
}

#[test]
fn link_rejected_under_markdown_profile() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "A", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["new", "B", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["link", "2", "--depends-on", "1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("frontmatter format"));
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

#[test]
fn read_command_warns_when_adr_dir_freshly_created() {
    let tmp = TempDir::new().unwrap();

    // A read command pointed at a non-existent dir creates it empty AND warns,
    // so a typo'd --dir / ADROIT_DIR doesn't masquerade as an empty repo.
    let missing = tmp.path().join("typo-adrs");
    Command::cargo_bin("adroit")
        .unwrap()
        .arg("--dir")
        .arg(&missing)
        .env("EDITOR", "true")
        .env("VISUAL", "true")
        .arg("list")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "did not exist and was created empty",
        ));

    // `new`, by contrast, is the expected first-run scaffold — a neutral note,
    // not a warning.
    let fresh = tmp.path().join("fresh-adrs");
    Command::cargo_bin("adroit")
        .unwrap()
        .arg("--dir")
        .arg(&fresh)
        .env("EDITOR", "true")
        .env("VISUAL", "true")
        .args(["new", "First decision", "--no-edit"])
        .assert()
        .success()
        .stderr(
            predicate::str::contains("Created new ADR directory")
                .and(predicate::str::contains("did not exist").not()),
        );
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
        .args(["set-status", &slug, "accepted"])
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
fn date_scheme_relinks_cross_adr_links_on_status_move() {
    let dir = TempDir::new().unwrap();
    adroit_date(&dir)
        .args(["new", "Adopt PostgreSQL", "--no-edit"])
        .assert()
        .success();
    adroit_date(&dir)
        .args(["new", "Use Redis cache", "--no-edit"])
        .assert()
        .success();

    // Find the two date-slug files and make A link to B (same dir).
    let a = adr_files(dir.path())
        .into_iter()
        .find(|p| p.to_str().unwrap().contains("adopt-postgresql"))
        .unwrap();
    let b_name = adr_files(dir.path())
        .into_iter()
        .find(|p| p.to_str().unwrap().contains("use-redis-cache"))
        .unwrap()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let mut body = fs::read_to_string(&a).unwrap();
    body.push_str(&format!("\n## Context\n\nSee [{b_name}](./{b_name}).\n"));
    fs::write(&a, body).unwrap();

    // Accepting B moves it to accepted/ and must rewrite A's link via the seam.
    let b_slug = b_name.strip_suffix(".md").unwrap();
    adroit_date(&dir)
        .args(["set-status", b_slug, "accepted"])
        .assert()
        .success();

    let a_after = fs::read_to_string(&a).unwrap();
    assert!(
        a_after.contains(&format!("(../accepted/{b_name})")),
        "date-slug cross-link should be relinked to the new dir, got:\n{a_after}"
    );
    adroit_date(&dir).arg("check").assert().success();
}

#[test]
fn date_scheme_rejects_numeric_only_commands() {
    let dir = TempDir::new().unwrap();
    adroit_date(&dir)
        .args(["new", "Adopt PostgreSQL", "--no-edit"])
        .assert()
        .success();

    // renumber / review are number-shaped and don't apply to a non-numeric
    // scheme — they bail with a clear message, not a confusing "not found".
    adroit_date(&dir)
        .args(["renumber", "1", "2"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("requires a numeric naming scheme"));
    adroit_date(&dir)
        .args(["review", "1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("requires a numeric naming scheme"));
}

#[test]
fn date_scheme_supersede_by_slug() {
    let dir = TempDir::new().unwrap();
    adroit_date(&dir)
        .args(["new", "Old approach", "--no-edit"])
        .assert()
        .success();
    adroit_date(&dir)
        .args(["new", "New approach", "--no-edit"])
        .assert()
        .success();
    let slug_of = |needle: &str| {
        adr_files(dir.path())
            .into_iter()
            .find(|p| p.to_str().unwrap().contains(needle))
            .map(|p| p.file_stem().unwrap().to_str().unwrap().to_string())
            .unwrap()
    };
    let old_slug = slug_of("old-approach");
    let new_slug = slug_of("new-approach");

    // Supersede by slug: the old ADR moves to superseded/ with a slug link;
    // the new ADR gets a reciprocal "Supersedes [<old-slug>]" note.
    adroit_date(&dir)
        .args(["supersede", &new_slug, &old_slug])
        .assert()
        .success();

    let old = dir.path().join(format!("superseded/{old_slug}.md"));
    assert!(old.exists(), "old ADR moved to superseded/");
    let old_body = fs::read_to_string(&old).unwrap();
    assert!(old_body.contains(&format!("Superseded by [{new_slug}]")));
    let new = dir.path().join(format!("proposed/{new_slug}.md"));
    let new_body = fs::read_to_string(&new).unwrap();
    assert!(new_body.contains(&format!("Supersedes [{old_slug}]")));

    // The repo stays consistent (links resolve, no broken supersession refs).
    adroit_date(&dir).arg("check").assert().success();
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

// ---------------------------------------------------------------------------
// Forge integration (issue #4)
// ---------------------------------------------------------------------------

#[test]
fn new_without_forge_has_no_references_section() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Plain ADR", "--no-edit"])
        .assert()
        .success();
    let body = fs::read_to_string(dir.path().join("proposed/0001-plain-adr.md")).unwrap();
    assert!(
        !body.contains("## References"),
        "bare `new` must not touch forge"
    );
}

#[cfg(not(feature = "forge"))]
#[test]
fn forge_flag_is_absent_without_the_feature() {
    // A no-forge build doesn't expose `--forge` at all (it's `#[cfg]`-gated), so
    // passing it is a hard error, not a silent no-op.
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "X", "--no-edit", "--forge"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unexpected argument"));
}

#[cfg(not(feature = "forge"))]
#[test]
fn forge_only_commands_are_absent_without_the_feature() {
    // init/auth/sync/notify are `#[cfg(feature = "forge")]` — a no-forge build
    // doesn't have them at all (publish stays — it's offline).
    let dir = TempDir::new().unwrap();
    for sub in ["auth", "init", "sync", "notify", "reconcile"] {
        adroit(&dir)
            .arg(sub)
            .assert()
            .failure()
            .stderr(predicate::str::contains("unrecognized subcommand"));
    }
}

#[cfg(feature = "forge")]
#[test]
fn new_with_forge_dry_run_previews_plan_without_network() {
    // Point config at a temp XDG dir with a github forge block + a fake token,
    // so the adapter constructs but --dry-run returns before any HTTP/git.
    let home = TempDir::new().unwrap();
    let cfgdir = home.path().join("adroit");
    fs::create_dir_all(&cfgdir).unwrap();
    fs::write(
        cfgdir.join("config.yaml"),
        "forge:\n  provider: github\n  repo: owner/repo\n",
    )
    .unwrap();

    let dir = TempDir::new().unwrap();
    let mut cmd = Command::cargo_bin("adroit").unwrap();
    cmd.env("EDITOR", "true")
        .env("VISUAL", "true")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .env("ADROIT_GITHUB_TOKEN", "fake-token")
        .arg("--dir")
        .arg(dir.path())
        .args(["new", "Adopt Postgres", "--no-edit", "--forge", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Forge plan"))
        .stdout(predicate::str::contains("create issue"));

    // Dry run touched nothing beyond the ADR file itself.
    let body = fs::read_to_string(dir.path().join("proposed/0001-adopt-postgres.md")).unwrap();
    assert!(!body.contains("## References"));
}

#[cfg(feature = "forge")]
#[test]
fn init_yes_writes_config_env_template_and_hook() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path();
    let adrs = repo.join("adrs");
    fs::create_dir_all(&adrs).unwrap();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .current_dir(repo)
            .args(args)
            .output()
            .unwrap();
    };
    git(&["init", "-q"]);
    git(&["remote", "add", "origin", "git@github.com:acme/widgets.git"]);
    let cfg = repo.join("cfg");

    // `--yes` = full non-interactive setup from the detected remote.
    Command::cargo_bin("adroit")
        .unwrap()
        .current_dir(repo)
        .env("XDG_CONFIG_HOME", &cfg)
        .env("XDG_DATA_HOME", repo.join("data"))
        .env("ADROIT_DIR", &adrs)
        .env("EDITOR", "true")
        .env("VISUAL", "true")
        .args(["init", "--yes"])
        .assert()
        .success();

    let conf = fs::read_to_string(cfg.join("adroit").join("config.yaml")).unwrap();
    assert!(conf.contains("provider: github"), "config:\n{conf}");
    assert!(conf.contains("repo: acme/widgets"), "config:\n{conf}");
    assert!(
        fs::read_to_string(repo.join(".env"))
            .unwrap()
            .contains("ADROIT_DIR=")
    );
    assert!(adrs.join("adr-template.md").exists());
    let hook = repo.join(".git").join("hooks").join("pre-commit");
    assert!(hook.exists(), "pre-commit hook not installed");
    assert!(fs::read_to_string(&hook).unwrap().contains("adroit check"));
}

// ---------------------------------------------------------------------------
// `-o json` output for the read verbs (agent-consumable CLI)
// ---------------------------------------------------------------------------

/// Run `args` against `dir`, assert success, and parse stdout as JSON.
fn json_ok(dir: &TempDir, args: &[&str]) -> serde_json::Value {
    let out = adroit(dir).args(args).assert().success();
    let text = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("`{args:?}` did not emit valid JSON: {e}\n{text}"))
}

#[test]
fn list_json_emits_array_of_summaries() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "First decision", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["new", "Second decision", "--no-edit"])
        .assert()
        .success();

    let v = json_ok(&dir, &["list", "-o", "json"]);
    assert_eq!(v.as_array().map(|a| a.len()), Some(2));
    assert_eq!(v[0]["reference"], "ADR-0001");
    assert_eq!(v[0]["title"], "First decision");
    assert_eq!(v[0]["status"], "Proposed");
}

#[test]
fn list_json_empty_repo_is_empty_array() {
    let dir = TempDir::new().unwrap();
    let v = json_ok(&dir, &["list", "-o", "json"]);
    assert_eq!(v.as_array().map(|a| a.len()), Some(0));
}

#[test]
fn show_json_emits_detail_object() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Only decision", "--no-edit"])
        .assert()
        .success();
    let v = json_ok(&dir, &["show", "1", "-o", "json"]);
    // AdrDetail flattens the summary to the top level alongside `body`.
    assert_eq!(v["reference"], "ADR-0001");
    assert_eq!(v["title"], "Only decision");
    assert!(v["body"].is_string(), "detail JSON carries the raw body");
}

#[test]
fn search_json_emits_matching_array() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Adopt Postgres", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["new", "Adopt Redis", "--no-edit"])
        .assert()
        .success();
    let v = json_ok(&dir, &["search", "Postgres", "-o", "json"]);
    assert_eq!(v.as_array().map(|a| a.len()), Some(1));
    assert_eq!(v[0]["title"], "Adopt Postgres");
}

#[test]
fn stats_json_has_totals_and_status_breakdown() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "A decision", "--no-edit"])
        .assert()
        .success();
    let v = json_ok(&dir, &["stats", "-o", "json"]);
    assert_eq!(v["total"], 1);
    assert!(v["by_status"].is_array());
}

#[test]
fn graph_json_has_nodes_and_edges() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "A decision", "--no-edit"])
        .assert()
        .success();
    let v = json_ok(&dir, &["graph", "-o", "json"]);
    assert_eq!(v["nodes"].as_array().map(|a| a.len()), Some(1));
    assert!(v["edges"].is_array());
}

#[test]
fn check_json_clean_repo_exits_zero() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "A decision", "--no-edit"])
        .assert()
        .success();
    let v = json_ok(&dir, &["check", "-o", "json"]);
    assert_eq!(v["checked"], 1);
    assert_eq!(v["problems"].as_array().map(|a| a.len()), Some(0));
}

#[test]
fn check_json_broken_link_emits_json_and_exits_nonzero() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "A decision", "--no-edit"])
        .assert()
        .success();
    // Inject a broken cross-ADR link (ADR-0099 doesn't exist) → Error severity.
    let file = adr_files(dir.path()).into_iter().next().unwrap();
    let mut body = fs::read_to_string(&file).unwrap();
    body.push_str("\nSee [ADR-0099](./0099-ghost.md) for context.\n");
    fs::write(&file, body).unwrap();

    // The CI gate still holds (non-zero exit), but stdout is still valid JSON.
    let out = adroit(&dir)
        .args(["check", "-o", "json"])
        .assert()
        .failure();
    let text = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("check -o json must emit JSON even on failure: {e}\n{text}"));
    assert!(
        !v["problems"].as_array().unwrap().is_empty(),
        "expected the broken link to be reported as a problem"
    );
}

#[test]
fn read_verbs_default_to_human_output() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "A decision", "--no-edit"])
        .assert()
        .success();
    // No -o flag → human table (header line), not JSON.
    adroit(&dir)
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("Status"))
        .stdout(predicate::str::starts_with("[").not());
}

// ---------------------------------------------------------------------------
// `new --interview` (AI-assisted authoring; FakeProvider via ADROIT_AI_FAKE)
// ---------------------------------------------------------------------------

#[test]
fn new_interview_drafts_body_but_keeps_mechanical_heading_and_status() {
    let dir = TempDir::new().unwrap();
    let canned = "## Context and Problem Statement\n\nDrafted by the fake provider.\n\n\
                  ## Decision Outcome\n\nChosen option: **A**.";
    adroit(&dir)
        .args(["new", "Adopt feature flags", "--interview", "--no-edit"])
        .env("ADROIT_AI_FAKE", canned)
        .write_stdin("ctx\ndrivers\noptions\nrisks\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Created"));

    let file = adr_files(dir.path()).into_iter().next().unwrap();
    let body = fs::read_to_string(&file).unwrap();
    // Identity + status stay mechanical; the AI prose lands under the marker.
    assert!(
        body.contains("# ADR-0001: Adopt feature flags"),
        "heading preserved"
    );
    assert!(body.contains("## Status"), "status section preserved");
    assert!(
        body.contains("<!-- adroit:ai-suggested -->"),
        "AI marker present"
    );
    assert!(
        body.contains("Drafted by the fake provider."),
        "AI prose present"
    );

    // The result is a valid repo and the status getter still works.
    adroit(&dir).arg("check").assert().success();
    adroit(&dir)
        .args(["status", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("proposed"));
}

#[test]
fn new_interview_without_a_provider_keeps_the_plain_template() {
    let dir = TempDir::new().unwrap();
    // No ADROIT_AI_FAKE and no `ai` feature → no provider → degrade gracefully.
    adroit(&dir)
        .args(["new", "Some decision", "--interview", "--no-edit"])
        .assert()
        .success()
        .stderr(predicate::str::contains("needs an AI provider"));
    // The ADR still exists and is valid (the plain template).
    adroit(&dir).arg("check").assert().success();
}

// ---------------------------------------------------------------------------
// `adroit plan` (AI implementation plan; read-only; FakeProvider seam)
// ---------------------------------------------------------------------------

#[test]
fn plan_generates_an_implementation_plan_via_fake_provider() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Adopt feature flags", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["plan", "1"])
        .env("ADROIT_AI_FAKE", "## Implementation Plan\n\n- [ ] Step one")
        .assert()
        .success()
        .stdout(predicate::str::contains("Implementation Plan"))
        .stdout(predicate::str::contains("Step one"));
    // Read-only: the ADR is untouched and the repo stays valid.
    adroit(&dir).arg("check").assert().success();
}

#[test]
fn plan_without_a_provider_errors() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "X", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["plan", "1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("needs an AI provider"));
}

// ---------------------------------------------------------------------------
// `adroit lint` (authoring-quality checks; read-only)
// ---------------------------------------------------------------------------

#[test]
fn lint_flags_a_fresh_template_and_exits_nonzero() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Adopt feature flags", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["lint", "1"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("placeholder"));
}

#[test]
fn lint_json_emits_findings_on_stdout() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "X", "--no-edit"])
        .assert()
        .success();
    let out = adroit(&dir)
        .args(["lint", "1", "-o", "json"])
        .assert()
        .failure();
    let text = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(!v.as_array().unwrap().is_empty());
    assert_eq!(v[0]["source"], "mechanical");
}

#[test]
fn lint_passes_a_complete_adr() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Adopt feature flags", "--no-edit"])
        .assert()
        .success();
    // Overwrite with a fully-filled ADR (no placeholders, 2 options, real downside).
    let file = adr_files(dir.path()).into_iter().next().unwrap();
    fs::write(
        &file,
        "# ADR-0001: Adopt feature flags\n\n> State: Proposed\n\n## Status\n\nProposed\n\n\
         ## Stakeholders\n\n- Platform team\n\n## Context and Problem Statement\n\n\
         We ship risky changes and want to decouple deploy from release.\n\n\
         ## Considered Options\n\n1. Feature flags\n2. Long-lived branches\n\n\
         ## Decision Outcome\n\nChosen: feature flags, to decouple deploy from release.\n\n\
         ### Negative Consequences\n\n- Flag debt accumulates and needs periodic cleanup.\n",
    )
    .unwrap();
    adroit(&dir)
        .args(["lint", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("no lint findings"));
}

#[test]
fn lint_ai_without_a_provider_errors() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "X", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["lint", "1", "--ai"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("needs an AI provider"));
}

// ---------------------------------------------------------------------------
// `adroit summarize` (one-paragraph AI TL;DR; read-only)
// ---------------------------------------------------------------------------

#[test]
fn summarize_prints_the_tldr_via_fake_provider() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Adopt feature flags", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["summarize", "1"])
        .env(
            "ADROIT_AI_FAKE",
            "A crisp one-paragraph TL;DR of the decision.",
        )
        .assert()
        .success()
        .stdout(predicate::str::contains("A crisp one-paragraph TL;DR"));
    // Read-only.
    adroit(&dir).arg("check").assert().success();
}

#[test]
fn summarize_without_a_provider_errors() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "X", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["summarize", "1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("needs an AI provider"));
}

// ---------------------------------------------------------------------------
// `adroit related` / `dedupe` (mechanical TF-IDF similarity; read-only)
// ---------------------------------------------------------------------------

/// Three ADRs (two about databases, one about the frontend) with topical bodies.
fn three_topical_adrs(dir: &TempDir) {
    for t in [
        "Adopt PostgreSQL datastore",
        "Use Redis cache database",
        "Pick Vue frontend UI",
    ] {
        adroit(dir).args(["new", t, "--no-edit"]).assert().success();
    }
    for f in adr_files(dir.path()) {
        let name = f.file_name().unwrap().to_str().unwrap().to_string();
        let extra = if name.contains("postgresql") {
            "relational postgresql database storage sql persistence datastore"
        } else if name.contains("redis") {
            "redis caching database in-memory storage lookups persistence"
        } else {
            "vue react frontend browser dashboard interface components"
        };
        let mut body = fs::read_to_string(&f).unwrap();
        body.push_str(&format!("\n{extra}\n"));
        fs::write(&f, body).unwrap();
    }
}

#[test]
fn related_ranks_the_topically_similar_adr_first() {
    let dir = TempDir::new().unwrap();
    three_topical_adrs(&dir);
    let out = adroit(&dir)
        .args(["related", "1", "-o", "json"])
        .assert()
        .success();
    let text = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    let arr = v.as_array().unwrap();
    assert!(!arr.is_empty());
    // The other database ADR (ADR-0002) outranks the frontend one (ADR-0003).
    assert_eq!(v[0]["reference"], "ADR-0002");
    assert!(v[0]["score"].as_f64().unwrap() > 0.0);
}

#[test]
fn dedupe_emits_ranked_json() {
    let dir = TempDir::new().unwrap();
    three_topical_adrs(&dir);
    let out = adroit(&dir)
        .args(["dedupe", "1", "-o", "json"])
        .assert()
        .success();
    let text = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(v[0]["title"].is_string() && v[0]["score"].is_number());
}

#[test]
fn related_on_a_single_adr_is_empty() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Lonely decision", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["related", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No unlinked related ADRs"));
}

// ---------------------------------------------------------------------------
// `adroit ask` (mechanical retrieval + AI answer with citations)
// ---------------------------------------------------------------------------

#[test]
fn ask_answers_with_retrieved_sources_via_fake_provider() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "Adopt PostgreSQL datastore", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["new", "Use Vue frontend", "--no-edit"])
        .assert()
        .success();
    for f in adr_files(dir.path()) {
        let name = f.file_name().unwrap().to_str().unwrap().to_string();
        let extra = if name.contains("postgresql") {
            "relational postgresql database storage acid durability"
        } else {
            "vue frontend browser dashboard interface"
        };
        let mut b = fs::read_to_string(&f).unwrap();
        b.push_str(&format!("\n{extra}\n"));
        fs::write(&f, b).unwrap();
    }
    let out = adroit(&dir)
        .args(["ask", "Which database did we choose?", "-o", "json"])
        .env("ADROIT_AI_FAKE", "PostgreSQL, per the datastore ADR.")
        .assert()
        .success();
    let text = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["answer"], "PostgreSQL, per the datastore ADR.");
    // The database ADR is among the retrieved sources.
    let sources = v["sources"].as_array().unwrap();
    assert!(sources.iter().any(|s| s == "ADR-0001"));
}

#[test]
fn ask_without_a_provider_errors() {
    let dir = TempDir::new().unwrap();
    adroit(&dir)
        .args(["new", "X", "--no-edit"])
        .assert()
        .success();
    adroit(&dir)
        .args(["ask", "anything?"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("needs an AI provider"));
}

// ---------------------------------------------------------------------------
// Help model: -h == --help (concise); --help-all is the full reference
// ---------------------------------------------------------------------------

#[test]
fn h_and_help_are_identical_and_concise_help_all_is_full() {
    let bin = || Command::cargo_bin("adroit").unwrap();
    let h = bin().arg("-h").output().unwrap();
    let help = bin().arg("--help").output().unwrap();
    assert!(h.status.success() && help.status.success());
    // -h and --help render the exact same (concise) help.
    assert_eq!(h.stdout, help.stdout, "`-h` and `--help` must be identical");

    let concise = String::from_utf8(h.stdout).unwrap();
    assert!(
        concise.contains("Authoring:"),
        "concise help lists commands"
    );
    assert!(
        !concise.contains("--relink-scope"),
        "concise help must NOT dump the repo-shape options"
    );

    let all = String::from_utf8(bin().arg("--help-all").output().unwrap().stdout).unwrap();
    assert!(
        all.contains("--relink-scope") && all.contains("--layout"),
        "--help-all lists every option"
    );
}

#[test]
fn subcommand_h_and_help_also_match() {
    let bin = || Command::cargo_bin("adroit").unwrap();
    let h = bin().args(["new", "-h"]).output().unwrap();
    let help = bin().args(["new", "--help"]).output().unwrap();
    assert!(h.status.success() && help.status.success());
    assert_eq!(
        h.stdout, help.stdout,
        "`new -h` and `new --help` must match"
    );
}
