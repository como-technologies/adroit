//! Write-side git helpers: create a branch, stage, commit, and push.
//!
//! The counterpart to [`crate::history`] (which only *reads* via `git log`).
//! Used by the forge integration to base a draft PR on an `adr/NNNN-…` branch,
//! and (later) by status changes that want to commit the move. Like
//! `history`, this shells `git` via [`std::process::Command`] and degrades
//! gracefully — every call returns a [`GitError`] the caller can warn on and
//! continue (the ADR file is already written; git/forge are scaffolding).
//!
//! This module is **std-only and always compiled** (not behind the `forge`
//! feature), so non-forge code paths can use it too.

use std::path::Path;
use std::process::Command;

/// A git command failed (git missing, not a work tree, non-zero exit, …).
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("failed to run git (is it installed and on PATH?): {0}")]
    Spawn(#[from] std::io::Error),
    #[error("git {op} failed: {stderr}")]
    Failed { op: String, stderr: String },
}

/// Run `git -C <dir> <args…>` and return trimmed stdout, or a [`GitError`]
/// carrying git's stderr on a non-zero exit.
fn run(dir: &Path, args: &[&str]) -> Result<String, GitError> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output()?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(GitError::Failed {
            op: args.first().copied().unwrap_or("").to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        })
    }
}

/// The name of the currently checked-out branch (e.g. `main`).
pub fn current_branch(dir: &Path) -> Result<String, GitError> {
    run(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
}

/// The default branch of `remote` (e.g. `main`), read from `remote`'s `HEAD`.
/// `None` when there's no such remote or HEAD ref (e.g. a repo with no remote).
pub fn default_remote_branch(dir: &Path, remote: &str) -> Option<String> {
    let head = format!("refs/remotes/{remote}/HEAD");
    let full = run(dir, &["symbolic-ref", "--short", &head]).ok()?;
    // `origin/main` -> `main`
    full.strip_prefix(&format!("{remote}/"))
        .map(str::to_string)
        .or(Some(full))
}

/// Create and switch to a new branch from the current HEAD.
pub fn create_branch(dir: &Path, name: &str) -> Result<(), GitError> {
    run(dir, &["switch", "-c", name]).map(drop)
}

/// Switch to an existing branch.
pub fn switch(dir: &Path, name: &str) -> Result<(), GitError> {
    run(dir, &["switch", name]).map(drop)
}

/// Stage a single path.
pub fn add(dir: &Path, path: &Path) -> Result<(), GitError> {
    let path = path.to_string_lossy();
    run(dir, &["add", "--", &path]).map(drop)
}

/// Commit the staged changes with `message` (no hooks, to stay scriptable).
pub fn commit(dir: &Path, message: &str) -> Result<(), GitError> {
    run(dir, &["commit", "--no-verify", "-m", message]).map(drop)
}

/// Push `branch` to `remote`, setting upstream.
pub fn push(dir: &Path, remote: &str, branch: &str) -> Result<(), GitError> {
    run(dir, &["push", "--set-upstream", remote, branch]).map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Configure identity in an isolated temp repo so `commit` works in CI.
    fn init_repo(dir: &Path) {
        run(dir, &["init", "-q"]).unwrap();
        run(dir, &["config", "user.email", "t@example.com"]).unwrap();
        run(dir, &["config", "user.name", "Tester"]).unwrap();
    }

    #[test]
    fn branch_add_commit_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        init_repo(dir);

        // Seed an initial commit so HEAD exists and branching is allowed.
        std::fs::write(dir.join("seed.txt"), "seed\n").unwrap();
        add(dir, &dir.join("seed.txt")).unwrap();
        commit(dir, "seed").unwrap();

        // Create a feature branch, add a file, commit on it.
        create_branch(dir, "adr/0001-use-postgres").unwrap();
        assert_eq!(current_branch(dir).unwrap(), "adr/0001-use-postgres");
        std::fs::write(dir.join("0001.md"), "# ADR-0001\n").unwrap();
        add(dir, &dir.join("0001.md")).unwrap();
        commit(dir, "ADR-0001").unwrap();

        // The commit landed on the new branch, not on the default branch.
        let log = run(dir, &["log", "--oneline"]).unwrap();
        assert!(log.contains("ADR-0001"));
    }

    #[test]
    fn errors_carry_stderr_not_panic() {
        let tmp = tempfile::tempdir().unwrap();
        // Not a git repo → a graceful error, never a panic.
        let err = current_branch(tmp.path()).unwrap_err();
        assert!(matches!(err, GitError::Failed { .. }));
    }
}
