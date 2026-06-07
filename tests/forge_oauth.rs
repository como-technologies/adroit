//! OAuth device-flow **live wiring** — the full login over the real `UreqTransport`
//! against a local mock HTTP server (closes the gap the fake-transport unit tests
//! in `src/forge/oauth.rs` leave: that `cmd_auth`'s actual HTTP path works).
//!
//! Runs under `--features forge` (`just test-forge`, folded into `just ci`).

#![cfg(feature = "forge")]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use adroit::forge::UreqTransport;
use adroit::forge::oauth::{self, Endpoints};

/// A throwaway HTTP/1.1 server: routes `/device` vs `/token`, and on `/token`
/// returns `authorization_pending` once before the access token (exercising the
/// poll loop). Returns the bound port.
fn spawn_mock() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let polls = Arc::new(AtomicUsize::new(0));
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            handle(stream, &polls);
        }
    });
    port
}

fn handle(mut stream: TcpStream, polls: &AtomicUsize) {
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).unwrap_or(0);
    let req = String::from_utf8_lossy(&buf[..n]);
    let body = if req.contains("/device") {
        r#"{"device_code":"DC-XYZ","user_code":"WXYZ-1234","verification_uri":"http://127.0.0.1/dev","interval":1,"expires_in":900}"#.to_string()
    } else if polls.fetch_add(1, Ordering::SeqCst) == 0 {
        // First poll: still pending.
        r#"{"error":"authorization_pending"}"#.to_string()
    } else {
        // Second poll: granted.
        r#"{"access_token":"gho_LIVE_FLOW_TOKEN","token_type":"bearer"}"#.to_string()
    };
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(resp.as_bytes());
    let _ = stream.flush();
}

#[test]
fn device_login_works_end_to_end_over_real_http() {
    let port = spawn_mock();
    // Point Endpoints straight at the mock (http, not https) — exercises the real
    // ureq transport over a real socket, no TLS.
    let ep = Endpoints {
        device_url: format!("http://127.0.0.1:{port}/device"),
        token_url: format!("http://127.0.0.1:{port}/token"),
        scope: "repo",
    };
    let announced = std::sync::Mutex::new(String::new());
    let token = oauth::device_login(
        &UreqTransport,
        &ep,
        "client-id-123",
        |dc| announced.lock().unwrap().push_str(&dc.user_code),
        |_d: Duration| {}, // don't actually sleep between polls
    )
    .expect("device flow should complete");

    assert_eq!(token, "gho_LIVE_FLOW_TOKEN");
    // The user code was surfaced to the announce hook…
    assert_eq!(*announced.lock().unwrap(), "WXYZ-1234");
    // …and the granted token was NEVER passed to it (secret hygiene).
    assert!(!announced.lock().unwrap().contains("gho_LIVE_FLOW_TOKEN"));
}
