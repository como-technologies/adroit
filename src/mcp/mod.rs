//! `adroit mcp`: a [Model Context Protocol](https://modelcontextprotocol.io)
//! server (feature `mcp`).
//!
//! Projects the **read-only** slice of `adroit manifest` ([`crate::manifest`]) as
//! MCP **tools** over JSON-RPC 2.0 on stdio (newline-delimited messages). An MCP
//! client (Claude, the portfolio's Adopt-stage engine, any agent) lists adroit's
//! read verbs and calls them to read decisions + plans — no `--help` scraping, no
//! bespoke subprocess plumbing.
//!
//! **Read-only by design:** only verbs the manifest marks `reads && !writes` and
//! non-`long-running` (and not the artifact-producing `publish`) become tools, so
//! nothing here can mutate the repo over the wire. A `tools/call` re-runs the verb
//! as `adroit <verb> … -o json` with the resolved on-disk shape forwarded as env,
//! so it stays drift-proof: a new read verb auto-appears as a tool and executes
//! with no code change.
//!
//! The protocol handlers ([`handle_line`]) are pure (`&str -> Option<String>`),
//! unit-tested with no stdio and fuzzed for no-panic on hostile input; [`run`] is
//! the thin stdio driver.

mod tools;

use std::io::{BufRead, Write};
use std::path::Path;

use serde_json::{Value, json};

use crate::config::Config;

pub use tools::Server;

/// The MCP protocol revision this server speaks (latest spec revision).
const PROTOCOL_VERSION: &str = "2025-11-25";

/// Run the MCP server on stdio: read newline-delimited JSON-RPC from stdin, write
/// responses to stdout. `dir` is the resolved ADR directory; the on-disk shape in
/// `cfg` is forwarded to each tool subprocess so it reads the same profile. Blocks
/// until stdin closes (the client disconnects).
pub fn run(cfg: &Config, dir: &Path) -> anyhow::Result<()> {
    let server = Server::new(cfg, dir);
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_line(&server, &line) {
            out.write_all(response.as_bytes())?;
            out.write_all(b"\n")?;
            out.flush()?;
        }
    }
    Ok(())
}

/// Parse one JSON-RPC message line and dispatch it. Returns the serialized
/// response line, or `None` for a notification (no `id` ⇒ no reply). Tolerates any
/// input — a malformed line yields a JSON-RPC parse error, never a panic.
pub fn handle_line(server: &Server, line: &str) -> Option<String> {
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return Some(error(Value::Null, -32700, &format!("parse error: {e}"))),
    };
    let method = msg
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    // No `id` ⇒ a notification: do nothing, reply nothing.
    let id = msg.get("id").cloned()?;
    Some(match method {
        "initialize" => ok(id, initialize()),
        "tools/list" => ok(id, json!({ "tools": server.tool_list() })),
        "tools/call" => server.tools_call(id, &msg),
        "ping" => ok(id, json!({})),
        other => error(id, -32601, &format!("method not found: {other}")),
    })
}

fn initialize() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "adroit", "version": env!("CARGO_PKG_VERSION") },
    })
}

/// A JSON-RPC success response line.
pub(crate) fn ok(id: Value, result: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

/// A JSON-RPC error response line.
pub(crate) fn error(id: Value, code: i64, message: &str) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> Server {
        let tmp = tempfile::tempdir().unwrap();
        Server::new(&Config::default(), tmp.path())
    }

    #[test]
    fn malformed_line_is_a_parse_error() {
        let r = handle_line(&server(), "{not json").unwrap();
        assert!(r.contains("-32700"), "{r}");
    }

    #[test]
    fn notification_without_id_gets_no_response() {
        let r = handle_line(
            &server(),
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        );
        assert!(r.is_none());
    }

    #[test]
    fn initialize_advertises_server_and_protocol() {
        let r = handle_line(
            &server(),
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
        )
        .unwrap();
        assert!(r.contains("protocolVersion"), "{r}");
        assert!(r.contains("\"name\":\"adroit\""), "{r}");
    }

    #[test]
    fn unknown_method_is_method_not_found() {
        let r = handle_line(&server(), r#"{"jsonrpc":"2.0","id":2,"method":"bogus"}"#).unwrap();
        assert!(r.contains("-32601"), "{r}");
    }

    #[test]
    fn unknown_tool_is_an_error() {
        let r = handle_line(
            &server(),
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"nope"}}"#,
        )
        .unwrap();
        assert!(r.contains("unknown tool"), "{r}");
    }
}
