//! Git-derived ADR history: the real creation date, last-modified date, and the
//! status lifecycle (proposed → accepted / rejected / superseded), reconstructed
//! from the repository's commit log.
//!
//! **Why git.** The markdown / by-status profile persists no creation date, and
//! a fresh clone resets every file's mtime to checkout time — so neither the
//! file body nor the filesystem can tell you when an ADR was proposed or
//! decided. Git can: in the by-status layout a status change *is* a directory
//! move (a rename git records), and the first commit that added the file is its
//! creation.
//!
//! This module only **reads** git (via `git log`) and degrades gracefully:
//! outside a git repo, or for an untracked file, the lookups return `None` and
//! callers fall back to other date sources.

use std::path::{Path, PathBuf};
use std::process::Command;

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::adr::Status;

/// A handle to the git repository containing an ADR tree. Created once via
/// [`open`] so per-file lookups don't each re-probe for a repository.
#[derive(Debug, Clone)]
pub struct GitRepo {
    /// Directory to run `git -C` in (the resolved ADR dir; git walks up to the
    /// enclosing work tree on its own).
    dir: PathBuf,
}

/// The git-derived history of a single ADR file.
#[derive(Debug, Clone)]
pub struct AdrHistory {
    /// When the file was first added to the repo (oldest commit touching it).
    pub created: OffsetDateTime,
    /// The most recent commit that touched the file.
    pub last_modified: OffsetDateTime,
    /// Lifecycle milestones in chronological order: the initial proposal and
    /// each status change (a directory move). Plain content edits are excluded;
    /// empty in flat layout (where status isn't encoded by directory).
    pub events: Vec<HistoryEvent>,
}

/// One lifecycle milestone for an ADR (it reached `status` at `date`).
#[derive(Debug, Clone)]
pub struct HistoryEvent {
    /// Author date of the commit that produced this milestone.
    pub date: OffsetDateTime,
    /// The status the ADR reached.
    pub status: Status,
    /// Abbreviated commit hash.
    pub commit: String,
    /// Commit subject line.
    pub subject: String,
}

/// Probe for a git repository at (or above) `dir`. Returns `None` when git is
/// unavailable or `dir` is not inside a work tree — callers then fall back to
/// non-git date sources. Run once; reuse the handle for many files.
pub fn open(dir: &Path) -> Option<GitRepo> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .ok()?;
    if out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "true" {
        Some(GitRepo {
            dir: dir.to_path_buf(),
        })
    } else {
        None
    }
}

impl GitRepo {
    /// Whether this is a shallow clone (`git clone --depth=…`). On a shallow
    /// clone `git log --follow` can't see a file's true first commit, so
    /// creation dates are unreliable — callers in strict `git` mode warn.
    pub fn is_shallow(&self) -> bool {
        Command::new("git")
            .arg("-C")
            .arg(&self.dir)
            .args(["rev-parse", "--is-shallow-repository"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "true")
            .unwrap_or(false)
    }

    /// Git-derived history for `file`, or `None` if it is untracked / has no
    /// commits. `status_of` maps a path's directory to a [`Status`]; inject the
    /// store's config-aware mapping (`Store::dir_status`) so custom directory
    /// names are honored and flat layout yields no milestones.
    ///
    /// Performance: this runs one `git log` per file. ADR repos are small
    /// (dozens of files), so the per-file cost is fine; a single-pass log over
    /// the whole tree could be a future optimization if a repo grows large.
    pub fn history(
        &self,
        file: &Path,
        status_of: impl Fn(&Path) -> Option<Status>,
    ) -> Option<AdrHistory> {
        // `--follow` links a proposed→accepted move to the same logical file.
        // `%x1f` (unit separator) prefixes each commit header line so we can
        // tell headers apart from name-status lines unambiguously.
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.dir)
            .args([
                "log",
                "--follow",
                "--name-status",
                "--format=%x1f%h%x1f%aI%x1f%s",
            ])
            .arg("--")
            .arg(file)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        parse_log(&String::from_utf8_lossy(&out.stdout), status_of)
    }
}

/// Parse `git log --follow --name-status --format=%x1f%h%x1f%aI%x1f%s` output
/// (newest commit first) into an [`AdrHistory`]. Split out from the git call so
/// it can be unit-tested without a repository. Returns `None` if no commits are
/// present (untracked file).
fn parse_log(text: &str, status_of: impl Fn(&Path) -> Option<Status>) -> Option<AdrHistory> {
    let mut newest: Option<OffsetDateTime> = None;
    let mut oldest: Option<OffsetDateTime> = None;
    // Milestones collected newest-first (git's order), reversed below.
    let mut milestones: Vec<HistoryEvent> = Vec::new();
    // The header of the commit whose name-status lines we're currently reading.
    let mut cur: Option<(OffsetDateTime, String, String)> = None;

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix('\u{1f}') {
            // Commit header: <hash> \x1f <author-date> \x1f <subject>
            let mut p = rest.split('\u{1f}');
            let hash = p.next().unwrap_or("").to_string();
            let date = OffsetDateTime::parse(p.next().unwrap_or(""), &Rfc3339).ok()?;
            let subject = p.next().unwrap_or("").to_string();
            newest.get_or_insert(date);
            oldest = Some(date);
            cur = Some((date, hash, subject));
        } else if !line.trim().is_empty() {
            // Name-status line: "A\tpath", "M\tpath", or "R099\told\tnew".
            let Some((date, hash, subject)) = cur.as_ref() else {
                continue;
            };
            let mut cols = line.split('\t').filter(|s| !s.is_empty());
            let code = cols.next().unwrap_or("");
            let kind = code.as_bytes().first().copied().unwrap_or(b' ');
            // The path reflecting the file's location at/after this commit: for a
            // rename take the new (last) path, otherwise the single path.
            let path = if kind == b'R' || kind == b'C' {
                cols.next_back()
            } else {
                cols.next()
            };
            let Some(path) = path else { continue };
            // An add (the proposal) or a rename (a move) is a candidate
            // milestone — but only when the directory maps to a status. In flat
            // layout `status_of` is always `None`, so no milestones are emitted.
            if (kind == b'A' || kind == b'R')
                && let Some(status) = status_of(Path::new(path))
            {
                milestones.push(HistoryEvent {
                    date: *date,
                    status,
                    commit: hash.clone(),
                    subject: subject.clone(),
                });
            }
        }
    }

    let created = oldest?;
    let last_modified = newest?;

    // Chronological order, then collapse consecutive same-status milestones so a
    // within-status rename (e.g. a title change) isn't reported as a transition.
    milestones.reverse();
    let mut events: Vec<HistoryEvent> = Vec::new();
    for ev in milestones {
        if events.last().map(|e| e.status) != Some(ev.status) {
            events.push(ev);
        }
    }

    Some(AdrHistory {
        created,
        last_modified,
        events,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test resolver mirroring the by-status mapping: the parent directory name
    /// parsed as a `Status` (case-insensitive, via strum).
    fn by_status(p: &Path) -> Option<Status> {
        p.parent()?.file_name()?.to_str()?.parse::<Status>().ok()
    }

    /// Flat layout: directory never implies a status.
    fn flat(_p: &Path) -> Option<Status> {
        None
    }

    const US: char = '\u{1f}';

    #[test]
    fn parses_proposed_then_accepted_lifecycle() {
        // Newest first: a later edit, the accept rename, then the original add.
        let log = format!(
            "{US}cccc{US}2026-05-07T09:00:00-04:00{US}fix links\n\
             M\tsrc/adrs/accepted/0003-x.md\n\
             {US}bbbb{US}2026-04-20T15:31:58-04:00{US}accept it\n\
             R099\tsrc/adrs/proposed/0003-x.md\tsrc/adrs/accepted/0003-x.md\n\
             {US}aaaa{US}2026-04-10T14:10:45-04:00{US}propose it\n\
             A\tsrc/adrs/proposed/0003-x.md\n"
        );
        let h = parse_log(&log, by_status).unwrap();
        assert_eq!(
            h.created,
            OffsetDateTime::parse("2026-04-10T14:10:45-04:00", &Rfc3339).unwrap()
        );
        assert_eq!(
            h.last_modified,
            OffsetDateTime::parse("2026-05-07T09:00:00-04:00", &Rfc3339).unwrap()
        );
        assert_eq!(h.events.len(), 2);
        assert_eq!(h.events[0].status, Status::Proposed);
        assert_eq!(h.events[0].commit, "aaaa");
        assert_eq!(h.events[1].status, Status::Accepted);
        assert_eq!(h.events[1].subject, "accept it");
        // Chronological: proposal before acceptance.
        assert!(h.events[0].date < h.events[1].date);
    }

    #[test]
    fn collapses_within_status_rename() {
        // A title-change rename inside accepted/ must not register as a transition.
        let log = format!(
            "{US}cccc{US}2026-05-01T09:00:00Z{US}rename for clarity\n\
             R100\tsrc/adrs/accepted/0003-old.md\tsrc/adrs/accepted/0003-new.md\n\
             {US}bbbb{US}2026-04-20T09:00:00Z{US}accept\n\
             R099\tsrc/adrs/proposed/0003-old.md\tsrc/adrs/accepted/0003-old.md\n\
             {US}aaaa{US}2026-04-10T09:00:00Z{US}propose\n\
             A\tsrc/adrs/proposed/0003-old.md\n"
        );
        let h = parse_log(&log, by_status).unwrap();
        // Proposed, then Accepted — the second accepted→accepted rename collapses.
        assert_eq!(h.events.len(), 2);
        assert_eq!(h.events[0].status, Status::Proposed);
        assert_eq!(h.events[1].status, Status::Accepted);
    }

    #[test]
    fn flat_layout_has_dates_but_no_milestones() {
        let log = format!(
            "{US}bbbb{US}2026-04-20T09:00:00Z{US}edit\n\
             M\tdocs/0003-x.md\n\
             {US}aaaa{US}2026-04-10T09:00:00Z{US}add\n\
             A\tdocs/0003-x.md\n"
        );
        let h = parse_log(&log, flat).unwrap();
        assert!(h.events.is_empty());
        assert_eq!(
            h.created,
            OffsetDateTime::parse("2026-04-10T09:00:00Z", &Rfc3339).unwrap()
        );
        assert_eq!(
            h.last_modified,
            OffsetDateTime::parse("2026-04-20T09:00:00Z", &Rfc3339).unwrap()
        );
    }

    #[test]
    fn empty_log_is_none() {
        assert!(parse_log("", by_status).is_none());
    }

    #[test]
    fn end_to_end_against_a_real_repo() {
        // Skip gracefully if git isn't on PATH (keeps the suite green anywhere).
        if Command::new("git").arg("--version").output().is_err() {
            eprintln!("git not available; skipping end_to_end_against_a_real_repo");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // Commit dates are pinned (deterministic-rerun lesson): on a
        // clock-stepping host (e.g. NTP under WSL2) two wall-clock commits can
        // land out of order, flaking `created <= last_modified` — the
        // iteration-2 integration gate hit exactly that.
        let git = |args: &[&str], date: &str| {
            let ok = Command::new("git")
                .arg("-C")
                .arg(root)
                // Deterministic identity so commits succeed in any environment.
                .args(["-c", "user.email=t@t", "-c", "user.name=t"])
                .env("GIT_AUTHOR_DATE", date)
                .env("GIT_COMMITTER_DATE", date)
                .args(args)
                .output()
                .unwrap()
                .status
                .success();
            assert!(ok, "git {args:?} failed");
        };
        const T1: &str = "2026-01-01T00:00:00Z";
        const T2: &str = "2026-01-02T00:00:00Z";
        git(&["init", "-q"], T1);
        std::fs::create_dir_all(root.join("proposed")).unwrap();
        std::fs::write(root.join("proposed/0001-x.md"), "# ADR-0001: X\n").unwrap();
        git(&["add", "."], T1);
        git(&["commit", "-q", "-m", "propose"], T1);
        std::fs::create_dir_all(root.join("accepted")).unwrap();
        git(&["mv", "proposed/0001-x.md", "accepted/0001-x.md"], T2);
        git(&["commit", "-q", "-m", "accept"], T2);

        let repo = open(root).expect("temp dir is a git work tree");
        let h = repo
            .history(&root.join("accepted/0001-x.md"), |p| {
                p.parent()?.file_name()?.to_str()?.parse::<Status>().ok()
            })
            .expect("file is tracked");
        assert!(h.created <= h.last_modified);
        let statuses: Vec<Status> = h.events.iter().map(|e| e.status).collect();
        assert_eq!(statuses, vec![Status::Proposed, Status::Accepted]);
    }
}
