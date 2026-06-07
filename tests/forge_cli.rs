//! Forge CLI orchestration — graceful degradation (hardening blitz #5).
//!
//! Drives the real `--forge` flows end-to-end and asserts the documented contract:
//! **the ADR is the durable record** — a forge that is inactive (no token) or
//! unreachable (network down) must *warn and keep the local write*, never fail the
//! command or lose the ADR. (The adapters' response parsing is fuzzed separately in
//! `tests/forge_faults.rs`; the orchestration cores have unit tests with mock
//! adapters in `src/forge/mod.rs`. The happy-path live wiring — issue+PR created
//! against a mock HTTP server with a git remote — is the remaining heavier piece.)
//!
//! Runs under `--features forge` (`just test-forge`, folded into `just ci`).

#![cfg(feature = "forge")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

struct Setup {
    tmp: TempDir,
    cfg_home: PathBuf,
    adr_dir: PathBuf,
}

/// A throwaway repo + an isolated `config.yaml` containing `forge_yaml`.
fn setup(forge_yaml: &str) -> Setup {
    let tmp = TempDir::new().unwrap();
    let cfg_home = tmp.path().join("config");
    let adr_dir = tmp.path().join("adrs");
    fs::create_dir_all(cfg_home.join("adroit")).unwrap();
    fs::create_dir_all(&adr_dir).unwrap();
    fs::write(cfg_home.join("adroit/config.yaml"), forge_yaml).unwrap();
    Setup {
        tmp,
        cfg_home,
        adr_dir,
    }
}

/// Run `adroit` against the isolated config (XDG) and a clean working dir (so no
/// stray `.env` leaks in). `token` sets `ADROIT_GITHUB_TOKEN`, else it's removed.
fn run(s: &Setup, token: Option<&str>, args: &[&str]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_adroit"));
    c.current_dir(s.tmp.path())
        .env("XDG_CONFIG_HOME", &s.cfg_home)
        // Pin the file store so credential ops stay in the isolated XDG home and
        // never touch the developer's real OS keychain.
        .env("ADROIT_CREDENTIAL_STORE", "file")
        .env("EDITOR", "true")
        .env("VISUAL", "true")
        .env_remove("ADROIT_GITHUB_TOKEN");
    if let Some(t) = token {
        c.env("ADROIT_GITHUB_TOKEN", t);
    }
    c.arg("--dir").arg(&s.adr_dir).args(args);
    c.output().expect("spawn adroit")
}

fn adr_count(root: &Path) -> usize {
    fn walk(dir: &Path, n: &mut usize) {
        for e in fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, n);
            } else if p.extension().is_some_and(|x| x == "md") {
                let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if !name.eq_ignore_ascii_case("README.md") {
                    *n += 1;
                }
            }
        }
    }
    let mut n = 0;
    walk(root, &mut n);
    n
}

const GITHUB: &str = "forge:\n  provider: github\n  repo: owner/repo\n";
const GITHUB_OFFLINE: &str =
    "forge:\n  provider: github\n  repo: owner/repo\n  host: 127.0.0.1:9\n";

/// `--forge` with no token: the integration is inactive, so the ADR is written
/// locally with a clear "inactive" notice — the command still succeeds.
#[test]
fn new_forge_inactive_without_token_keeps_local_write() {
    let s = setup(GITHUB);
    let out = run(&s, None, &["new", "Alpha", "--no-edit", "--forge"]);
    assert!(
        out.status.success(),
        "should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("inactive"),
        "should warn that the integration is inactive"
    );
    assert_eq!(adr_count(&s.adr_dir), 1, "ADR written locally");
}

/// `--forge` with a token but an unreachable host: graceful-offline — warn and
/// keep the local write, exit 0.
#[test]
fn new_forge_offline_keeps_local_write() {
    let s = setup(GITHUB_OFFLINE);
    let out = run(&s, Some("token"), &["new", "Beta", "--no-edit", "--forge"]);
    assert!(
        out.status.success(),
        "graceful-offline should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("unreachable"),
        "should warn the forge is unreachable"
    );
    assert_eq!(
        adr_count(&s.adr_dir),
        1,
        "ADR written locally despite forge down"
    );
}

/// A status change with `--forge` while offline still applies the **local** move
/// (the ADR is the durable record); only the forge side is skipped with a warning.
#[test]
fn set_status_forge_offline_keeps_local_move() {
    let s = setup(GITHUB_OFFLINE);
    // Create locally (no forge), then accept with --forge while offline.
    let out = run(&s, Some("token"), &["new", "Gamma", "--no-edit"]);
    assert!(out.status.success());
    let out = run(
        &s,
        Some("token"),
        &["set-status", "1", "accepted", "--forge", "--yes"],
    );
    assert!(
        out.status.success(),
        "graceful-offline status change should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // The local move happened regardless of the forge being down.
    assert!(
        s.adr_dir.join("accepted/0001-gamma.md").exists(),
        "the ADR must have moved to accepted/ locally"
    );
    assert_eq!(adr_count(&s.adr_dir), 1);
}

// ---------------------------------------------------------------------------
// `adroit auth` — credential storage + secret hygiene (#9, Part A)
// ---------------------------------------------------------------------------

#[test]
fn auth_stores_to_the_file_store_and_never_echoes_the_token() {
    let s = setup("forge:\n  provider: github\n  repo: owner/repo\n");
    let out = run(&s, None, &["auth", "github", "--token", "SUPER-SECRET-XYZ"]);
    assert!(
        out.status.success(),
        "auth failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    // Reports where it landed (file store, since ADROIT_CREDENTIAL_STORE=file)…
    assert!(stdout.contains("file store"), "stdout: {stdout}");
    // …and NEVER echoes the token value (secret hygiene).
    assert!(
        !stdout.contains("SUPER-SECRET-XYZ"),
        "token leaked to stdout"
    );
    assert!(
        !stderr.contains("SUPER-SECRET-XYZ"),
        "token leaked to stderr"
    );
    // It persisted to the isolated 0600 credentials.yaml.
    let creds = fs::read_to_string(s.cfg_home.join("adroit/credentials.yaml")).unwrap();
    assert!(creds.contains("github"));
    assert!(creds.contains("SUPER-SECRET-XYZ"));
}
