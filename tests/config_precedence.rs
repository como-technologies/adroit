//! Config resolution precedence (hardening blitz #1).
//!
//! adroit resolves a setting from up to five sources. This pins the documented
//! precedence — highest wins:
//!
//! `--flag` > process env (`ADROIT_*`) > `.env` file > `config.yaml` (XDG) > default
//!
//! Each source is isolated: `config.yaml` via `XDG_CONFIG_HOME`, `.env` via the
//! child's working directory (dotenvy reads CWD), the env var via the child's env,
//! and the flag via args. We read the resolved value back with `adroit config get`.

use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

/// Which sources to populate for one resolution.
#[derive(Default)]
struct Sources<'a> {
    config: Option<&'a str>,
    dotenv: Option<&'a str>,
    env: Option<&'a str>,
    flag: Option<&'a str>,
}

/// Resolve `key` (with env var `env_var`) under the given sources via
/// `adroit config get <key>`, returning the printed value.
fn resolved(key: &str, env_var: &str, src: Sources) -> String {
    let tmp = TempDir::new().unwrap();
    let cfg_home = tmp.path().join("config");
    let work = tmp.path().join("work");
    fs::create_dir_all(cfg_home.join("adroit")).unwrap();
    fs::create_dir_all(&work).unwrap();

    if let Some(c) = src.config {
        fs::write(cfg_home.join("adroit/config.yaml"), format!("{key}: {c}\n")).unwrap();
    }
    if let Some(d) = src.dotenv {
        fs::write(work.join(".env"), format!("{env_var}={d}\n")).unwrap();
    }

    let mut cmd = Command::cargo_bin("adroit").unwrap();
    cmd.current_dir(&work) // dotenvy reads .env from here
        .env("XDG_CONFIG_HOME", &cfg_home) // config.yaml lives under here
        .env_remove(env_var); // start from a clean process env for this key
    if let Some(e) = src.env {
        cmd.env(env_var, e);
    }
    if let Some(f) = src.flag {
        cmd.arg(format!("--{}", key.replace('_', "-"))).arg(f);
    }
    cmd.args(["config", "get", key]);

    let out = cmd.output().expect("spawn adroit");
    assert!(
        out.status.success(),
        "`config get {key}` failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn precedence_ladder_for_format() {
    let k = "format";
    let v = "ADROIT_FORMAT";

    // 1. Nothing set → built-in default.
    assert_eq!(resolved(k, v, Sources::default()), "markdown");

    // 2. config.yaml alone.
    assert_eq!(
        resolved(
            k,
            v,
            Sources {
                config: Some("frontmatter"),
                ..Default::default()
            }
        ),
        "frontmatter"
    );

    // 3. .env overrides config.yaml.
    assert_eq!(
        resolved(
            k,
            v,
            Sources {
                config: Some("markdown"),
                dotenv: Some("frontmatter"),
                ..Default::default()
            }
        ),
        "frontmatter"
    );

    // 4. process env overrides .env (and config.yaml).
    assert_eq!(
        resolved(
            k,
            v,
            Sources {
                config: Some("markdown"),
                dotenv: Some("markdown"),
                env: Some("frontmatter"),
                flag: None,
            }
        ),
        "frontmatter"
    );

    // 5. flag overrides everything.
    assert_eq!(
        resolved(
            k,
            v,
            Sources {
                config: Some("markdown"),
                dotenv: Some("markdown"),
                env: Some("markdown"),
                flag: Some("frontmatter"),
            }
        ),
        "frontmatter"
    );
}

/// Every other settable profile key participates in the chain: a flag beats
/// `config.yaml`, and an unset key resolves to its documented default.
#[test]
fn precedence_participates_for_every_profile_key() {
    // (key, env var, value-a, value-b, default)
    let keys = [
        (
            "layout",
            "ADROIT_LAYOUT",
            "flat",
            "by_category",
            "by_status",
        ),
        ("naming", "ADROIT_NAMING", "uuid", "date", "sequential"),
        ("relink_scope", "ADROIT_RELINK_SCOPE", "self", "none", "all"),
        (
            "date_source",
            "ADROIT_DATE_SOURCE",
            "git",
            "filesystem",
            "auto",
        ),
    ];
    for (key, env_var, a, b, default) in keys {
        // flag (a) beats config.yaml (b).
        assert_eq!(
            resolved(
                key,
                env_var,
                Sources {
                    config: Some(b),
                    flag: Some(a),
                    ..Default::default()
                }
            ),
            a,
            "flag should beat config.yaml for `{key}`"
        );
        // env beats .env for `{key}`.
        assert_eq!(
            resolved(
                key,
                env_var,
                Sources {
                    dotenv: Some(b),
                    env: Some(a),
                    ..Default::default()
                }
            ),
            a,
            "process env should beat .env for `{key}`"
        );
        // nothing set → default.
        assert_eq!(
            resolved(key, env_var, Sources::default()),
            default,
            "unset `{key}` should be its default"
        );
    }
}
