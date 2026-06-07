//! OAuth 2.0 **Device Authorization Grant** for `adroit auth` — log in without a
//! manual token paste.
//!
//! A pure orchestration core over the [`HttpTransport`](super::HttpTransport) seam
//! (so it is unit-tested with a fake, no network), parameterized by per-provider
//! [`Endpoints`]. Device flow uses a **public** `client_id` (no secret); register
//! an OAuth app with device flow enabled and set `forge.oauth_client_id`.

use std::time::Duration;

use serde_json::Value;

use super::{ForgeError, HttpTransport, want_str};
use crate::config::Provider;

/// Per-provider device-flow endpoints + the OAuth scope to request.
pub struct Endpoints {
    pub device_url: String,
    pub token_url: String,
    pub scope: &'static str,
}

/// Resolve the device-flow endpoints for `provider`, honoring a self-hosted
/// `host` (GitHub Enterprise / self-managed GitLab). Device flow lives on the
/// **web** host, not the API host. `None` for [`Provider::None`].
pub fn endpoints(provider: Provider, host: Option<&str>) -> Option<Endpoints> {
    match provider {
        Provider::Github => {
            let base = match host {
                None => "https://github.com".to_string(),
                Some(h) if h.contains("api.github.com") => "https://github.com".to_string(),
                // GHE: host is like `ghe.example.com/api/v3` → web root is the bare host.
                Some(h) => format!("https://{}", h.split('/').next().unwrap_or(h)),
            };
            Some(Endpoints {
                device_url: format!("{base}/login/device/code"),
                token_url: format!("{base}/login/oauth/access_token"),
                scope: "repo",
            })
        }
        Provider::Gitlab => {
            let base = match host {
                None => "https://gitlab.com".to_string(),
                Some(h) => format!("https://{}", h.trim_end_matches('/')),
            };
            Some(Endpoints {
                device_url: format!("{base}/oauth/authorize_device"),
                token_url: format!("{base}/oauth/token"),
                scope: "api",
            })
        }
        Provider::None => None,
    }
}

/// The device-code response: the user-facing code + verification URL, and the
/// poll parameters.
#[derive(Debug, Clone)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
    pub expires_in: u64,
}

/// One poll outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Poll {
    Token(String),
    Pending,
    SlowDown,
    Denied,
    Expired,
    Other(String),
}

const FORM_HEADERS: &[(&str, &str)] = &[
    ("Accept", "application/json"),
    ("Content-Type", "application/x-www-form-urlencoded"),
];

/// Minimal `application/x-www-form-urlencoded` encoding (percent-encode values),
/// enough for the device-flow params (the `grant_type` URN's colons must encode).
fn form_encode(pairs: &[(&str, &str)]) -> String {
    fn enc(s: &str) -> String {
        s.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    (b as char).to_string()
                }
                b' ' => "+".to_string(),
                _ => format!("%{b:02X}"),
            })
            .collect()
    }
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={}", enc(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Step 1: request a device + user code.
pub fn request_device_code(
    transport: &dyn HttpTransport,
    device_url: &str,
    client_id: &str,
    scope: &str,
) -> Result<DeviceCode, ForgeError> {
    let body = form_encode(&[("client_id", client_id), ("scope", scope)]);
    let resp = transport.request("POST", device_url, FORM_HEADERS, Some(body.as_bytes()))?;
    let v: Value = serde_json::from_slice(&resp.body).map_err(|e| ForgeError::Api {
        status: resp.status,
        message: format!("invalid device-code response: {e}"),
    })?;
    Ok(DeviceCode {
        device_code: want_str(&v, "device_code", "OAuth")?,
        user_code: want_str(&v, "user_code", "OAuth")?,
        verification_uri: v["verification_uri"]
            .as_str()
            .or_else(|| v["verification_uri_complete"].as_str())
            .unwrap_or("")
            .to_string(),
        interval: v["interval"].as_u64().unwrap_or(5),
        expires_in: v["expires_in"].as_u64().unwrap_or(900),
    })
}

/// Step 2 (one poll): exchange the device code for a token, or report status.
pub fn poll_token(
    transport: &dyn HttpTransport,
    token_url: &str,
    client_id: &str,
    device_code: &str,
) -> Result<Poll, ForgeError> {
    let body = form_encode(&[
        ("client_id", client_id),
        ("device_code", device_code),
        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
    ]);
    let resp = transport.request("POST", token_url, FORM_HEADERS, Some(body.as_bytes()))?;
    // The provider returns 200 with an `error` body while pending, so parse the
    // body regardless of status.
    let v: Value = serde_json::from_slice(&resp.body).unwrap_or(Value::Null);
    if let Some(tok) = v["access_token"].as_str() {
        return Ok(Poll::Token(tok.to_string()));
    }
    Ok(match v["error"].as_str() {
        Some("authorization_pending") => Poll::Pending,
        Some("slow_down") => Poll::SlowDown,
        Some("expired_token") => Poll::Expired,
        Some("access_denied") => Poll::Denied,
        Some(other) => Poll::Other(other.to_string()),
        None => {
            return Err(ForgeError::Api {
                status: resp.status,
                message: "device token response had neither access_token nor error".to_string(),
            });
        }
    })
}

/// Step 2 (loop): poll until the token is granted, denied, or expired. `sleep` is
/// injected so tests run instantly; production passes `std::thread::sleep`.
pub fn poll_until(
    transport: &dyn HttpTransport,
    token_url: &str,
    client_id: &str,
    dc: &DeviceCode,
    sleep: impl Fn(Duration),
) -> Result<String, ForgeError> {
    let mut interval = dc.interval.max(1);
    let mut elapsed = 0u64;
    loop {
        sleep(Duration::from_secs(interval));
        elapsed += interval;
        match poll_token(transport, token_url, client_id, &dc.device_code)? {
            Poll::Token(t) => return Ok(t),
            Poll::Pending => {}
            Poll::SlowDown => interval += 5, // back off per the spec
            Poll::Denied => return Err(ForgeError::Auth("device login was denied".to_string())),
            Poll::Expired => {
                return Err(ForgeError::Auth(
                    "the device code expired — re-run `adroit auth`".to_string(),
                ));
            }
            Poll::Other(e) => {
                return Err(ForgeError::Api {
                    status: 0,
                    message: format!("device login error: {e}"),
                });
            }
        }
        if elapsed >= dc.expires_in {
            return Err(ForgeError::Auth("device login timed out".to_string()));
        }
    }
}

/// The full device-flow login: request a code, show the user the verification URL
/// + code via `announce`, then poll until a token is granted.
pub fn device_login(
    transport: &dyn HttpTransport,
    endpoints: &Endpoints,
    client_id: &str,
    announce: impl Fn(&DeviceCode),
    sleep: impl Fn(Duration),
) -> Result<String, ForgeError> {
    let dc = request_device_code(transport, &endpoints.device_url, client_id, endpoints.scope)?;
    announce(&dc);
    poll_until(transport, &endpoints.token_url, client_id, &dc, sleep)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forge::HttpResponse;
    use std::sync::Mutex;

    /// Returns the scripted responses in order (one per `request`). `Mutex` (not
    /// `RefCell`) because [`HttpTransport`] is `Send + Sync`.
    struct Fake {
        responses: Mutex<Vec<(u16, String)>>,
    }
    impl Fake {
        fn new(responses: Vec<(u16, &str)>) -> Self {
            Self {
                responses: Mutex::new(
                    responses
                        .into_iter()
                        .map(|(s, b)| (s, b.to_string()))
                        .collect(),
                ),
            }
        }
    }
    impl HttpTransport for Fake {
        fn request(
            &self,
            _m: &str,
            _u: &str,
            _h: &[(&str, &str)],
            _b: Option<&[u8]>,
        ) -> Result<HttpResponse, ForgeError> {
            let (status, body) = self.responses.lock().unwrap().remove(0);
            Ok(HttpResponse {
                status,
                body: body.into_bytes(),
            })
        }
    }

    fn no_sleep(_: Duration) {}

    #[test]
    fn endpoints_default_and_self_hosted() {
        let gh = endpoints(Provider::Github, None).unwrap();
        assert_eq!(gh.device_url, "https://github.com/login/device/code");
        assert_eq!(gh.scope, "repo");
        let ghe = endpoints(Provider::Github, Some("ghe.example.com/api/v3")).unwrap();
        assert_eq!(ghe.device_url, "https://ghe.example.com/login/device/code");
        let gl = endpoints(Provider::Gitlab, None).unwrap();
        assert_eq!(gl.token_url, "https://gitlab.com/oauth/token");
        assert!(endpoints(Provider::None, None).is_none());
    }

    #[test]
    fn request_device_code_parses_the_response() {
        let fake = Fake::new(vec![(
            200,
            r#"{"device_code":"DC","user_code":"WXYZ-1234","verification_uri":"https://github.com/login/device","interval":5,"expires_in":900}"#,
        )]);
        let dc = request_device_code(&fake, "u", "cid", "repo").unwrap();
        assert_eq!(dc.device_code, "DC");
        assert_eq!(dc.user_code, "WXYZ-1234");
        assert_eq!(dc.verification_uri, "https://github.com/login/device");
        assert_eq!(dc.interval, 5);
    }

    #[test]
    fn poll_token_maps_every_outcome() {
        let cases = [
            (
                r#"{"access_token":"gho_TOKEN"}"#,
                Poll::Token("gho_TOKEN".into()),
            ),
            (r#"{"error":"authorization_pending"}"#, Poll::Pending),
            (r#"{"error":"slow_down"}"#, Poll::SlowDown),
            (r#"{"error":"access_denied"}"#, Poll::Denied),
            (r#"{"error":"expired_token"}"#, Poll::Expired),
        ];
        for (body, expected) in cases {
            let fake = Fake::new(vec![(200, body)]);
            assert_eq!(poll_token(&fake, "u", "cid", "DC").unwrap(), expected);
        }
    }

    #[test]
    fn poll_until_returns_the_token_after_pending() {
        // device_login: device-code → pending → token.
        let fake = Fake::new(vec![
            (
                200,
                r#"{"device_code":"DC","user_code":"AB-CD","verification_uri":"https://x/dev","interval":1,"expires_in":900}"#,
            ),
            (200, r#"{"error":"authorization_pending"}"#),
            (200, r#"{"access_token":"gho_OK"}"#),
        ]);
        let ep = endpoints(Provider::Github, None).unwrap();
        let announced = std::cell::RefCell::new(String::new());
        let token = device_login(
            &fake,
            &ep,
            "cid",
            |dc| announced.borrow_mut().push_str(&dc.user_code),
            no_sleep,
        )
        .unwrap();
        assert_eq!(token, "gho_OK");
        assert_eq!(*announced.borrow(), "AB-CD"); // the user code was shown
    }

    #[test]
    fn poll_until_denied_and_expiry_are_errors() {
        let dc = DeviceCode {
            device_code: "DC".into(),
            user_code: "U".into(),
            verification_uri: "v".into(),
            interval: 1,
            expires_in: 3,
        };
        // Denied → Auth error.
        let denied = Fake::new(vec![(200, r#"{"error":"access_denied"}"#)]);
        assert!(poll_until(&denied, "u", "cid", &dc, no_sleep).is_err());
        // Always pending past expiry → timeout error (3 polls of interval 1).
        let pending = Fake::new(vec![
            (200, r#"{"error":"authorization_pending"}"#),
            (200, r#"{"error":"authorization_pending"}"#),
            (200, r#"{"error":"authorization_pending"}"#),
        ]);
        assert!(poll_until(&pending, "u", "cid", &dc, no_sleep).is_err());
    }

    #[test]
    fn hostile_responses_never_panic() {
        // Garbage bodies must yield Err, never panic (parser robustness).
        for body in ["", "not json", "{}", "[1,2,3]", "{\"device_code\":123}"] {
            let fake = Fake::new(vec![(200, body)]);
            let _ = request_device_code(&fake, "u", "cid", "repo");
            let fake = Fake::new(vec![(500, body)]);
            let _ = poll_token(&fake, "u", "cid", "DC");
        }
    }
}
