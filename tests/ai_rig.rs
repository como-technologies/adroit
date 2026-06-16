//! Wire-shape tests for the rig-backed AI provider (`src/ai/rig_provider.rs`).
//!
//! The iteration-2 root cause behind run-1's structure retries: **ollama
//! silently truncates at its default context window** (`num_ctx`, 2048 in the
//! suite's ollama) — a context-bearing prompt left ~50 tokens of generation
//! room and clipped output mid-fence, and nothing errored. The fix is pinning
//! `num_ctx` explicitly on every ollama call in the suite (learnings ledger;
//! assessments pinned 8192 first). These tests capture the literal JSON adroit
//! puts on the wire — a fake ollama server on a loopback socket, no network,
//! no model — so the pin can never silently regress with a rig upgrade.

#![cfg(feature = "ai")]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use adroit::ai::rig_provider::{OLLAMA_NUM_CTX, RigProvider};
use adroit::ai::{AiProvider, CompletionRequest};
use adroit::config::{AiConfig, AiProviderKind};

/// Accept exactly one HTTP request, return its body as parsed JSON, and answer
/// with a minimal valid ollama `/api/chat` (non-streaming) response.
fn fake_ollama_once(listener: TcpListener) -> thread::JoinHandle<serde_json::Value> {
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        // Read headers.
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        while !buf.ends_with(b"\r\n\r\n") {
            let n = stream.read(&mut byte).expect("read header byte");
            assert!(n > 0, "connection closed before headers ended");
            buf.push(byte[0]);
        }
        let headers = String::from_utf8_lossy(&buf).to_lowercase();
        let content_length: usize = headers
            .lines()
            .find_map(|l| l.strip_prefix("content-length:"))
            .expect("content-length header")
            .trim()
            .parse()
            .expect("numeric content-length");
        // Read the body.
        let mut body = vec![0u8; content_length];
        stream.read_exact(&mut body).expect("read body");
        let request: serde_json::Value = serde_json::from_slice(&body).expect("JSON body");
        // Answer with a valid non-streaming chat response.
        let reply = serde_json::json!({
            "model": "llama3.2",
            "created_at": "2026-06-12T00:00:00Z",
            "message": { "role": "assistant", "content": "pinned" },
            "done": true
        })
        .to_string();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{}",
            reply.len(),
            reply
        )
        .expect("write response");
        request
    })
}

/// The ollama completion request pins `options.num_ctx` (the iteration-2
/// silent-truncation root cause): without it, ollama clips the prompt at its
/// 2048-token default and never says so. Captured from the literal wire JSON.
#[test]
fn ollama_requests_pin_num_ctx() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    let server = fake_ollama_once(listener);

    let cfg = AiConfig {
        provider: AiProviderKind::Ollama,
        model: "llama3.2".into(),
        enabled: true,
        host: Some(format!("http://{addr}")),
        key: None,
    };
    let provider = RigProvider::from_config(&cfg).expect("provider");
    let out = provider
        .complete(&CompletionRequest {
            system: "You draft ADRs.".into(),
            prompt: "Draft one.".into(),
            max_tokens: 100,
        })
        .expect("completion");
    assert_eq!(out, "pinned");

    let request = server.join().expect("server thread");
    assert_eq!(
        request["options"]["num_ctx"],
        serde_json::json!(OLLAMA_NUM_CTX),
        "ollama request must pin options.num_ctx, got: {request}"
    );
    // The system preamble rides as the first chat message — the pin exists to
    // keep exactly this content from being silently clipped.
    assert_eq!(request["messages"][0]["role"], "system", "{request}");
}
