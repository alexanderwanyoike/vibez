//! OAuth 2.0 PKCE flow for Dropbox.
//!
//! Runs a loopback HTTP server on a fixed-port allowlist, opens the
//! user's browser to Dropbox's authorize URL, captures the redirect
//! with a `code` and verifies the `state` to guard against CSRF, then
//! exchanges the code for access + refresh tokens.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use base64::Engine;
use sha2::Digest;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::types::{DropboxError, DropboxResult, Tokens};

/// Ports we pre-register at Dropbox app creation time. Tried in order;
/// if all are busy the flow fails.
pub const LOOPBACK_PORTS: &[u16] = &[53_682, 53_683, 53_684];

/// Maximum time to wait for the user to approve in the browser.
pub const OAUTH_TIMEOUT: Duration = Duration::from_secs(300);

/// Platform-independent browser opener. The real implementation uses
/// the `open` crate; tests can inject a no-op.
pub trait BrowserOpener: Send + Sync {
    fn open(&self, url: &str) -> std::io::Result<()>;
}

pub struct SystemBrowserOpener;

impl BrowserOpener for SystemBrowserOpener {
    fn open(&self, url: &str) -> std::io::Result<()> {
        open::that(url)
    }
}

/// Run the full PKCE flow and return fresh tokens.
pub async fn run_flow(app_key: &str, opener: Arc<dyn BrowserOpener>) -> DropboxResult<Tokens> {
    let verifier = random_url_safe_string(64);
    let challenge = code_challenge(&verifier);
    let state = random_url_safe_string(32);

    let (listener, port) = bind_loopback().await?;
    let redirect_uri = format!("http://127.0.0.1:{port}");

    let auth_url = build_authorize_url(app_key, &challenge, &state, &redirect_uri);
    opener
        .open(&auth_url)
        .map_err(|e| DropboxError::Oauth(format!("failed to open browser: {e}")))?;

    let accept = tokio::time::timeout(OAUTH_TIMEOUT, listener.accept()).await;
    let (stream, _) = match accept {
        Ok(result) => result?,
        Err(_) => {
            return Err(DropboxError::Oauth(
                "timed out waiting for Dropbox approval".into(),
            ))
        }
    };

    let callback = read_callback(stream).await?;
    if callback.state != state {
        return Err(DropboxError::Oauth(
            "state mismatch: possible CSRF, refusing to continue".into(),
        ));
    }
    if let Some(err) = callback.error {
        return Err(DropboxError::Oauth(format!(
            "Dropbox returned authorisation error: {err}"
        )));
    }
    let code = callback
        .code
        .ok_or_else(|| DropboxError::Oauth("no `code` parameter in callback URL".into()))?;

    exchange_code_for_tokens(app_key, &verifier, &code, &redirect_uri).await
}

/// Exchange a long-lived refresh token for a new access token.
/// Dropbox may or may not return a new refresh token; preserve the
/// existing one if absent.
pub async fn refresh_access_token(app_key: &str, existing_refresh: &str) -> DropboxResult<Tokens> {
    let client = reqwest::Client::new();
    let response = client
        .post("https://api.dropboxapi.com/oauth2/token")
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", existing_refresh),
            ("client_id", app_key),
        ])
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(DropboxError::Api {
            status: status.as_u16(),
            body,
        });
    }

    let parsed: TokenResponse = response.json().await?;
    Ok(tokens_from_response(parsed, existing_refresh))
}

async fn exchange_code_for_tokens(
    app_key: &str,
    verifier: &str,
    code: &str,
    redirect_uri: &str,
) -> DropboxResult<Tokens> {
    let client = reqwest::Client::new();
    let response = client
        .post("https://api.dropboxapi.com/oauth2/token")
        .form(&[
            ("code", code),
            ("grant_type", "authorization_code"),
            ("client_id", app_key),
            ("code_verifier", verifier),
            ("redirect_uri", redirect_uri),
        ])
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(DropboxError::Api {
            status: status.as_u16(),
            body,
        });
    }

    let parsed: TokenResponse = response.json().await?;
    let refresh = parsed.refresh_token.clone().ok_or_else(|| {
        DropboxError::Oauth(
            "Dropbox did not return a refresh_token; is `token_access_type=offline` set?".into(),
        )
    })?;
    Ok(tokens_from_response(parsed, &refresh))
}

fn tokens_from_response(resp: TokenResponse, fallback_refresh: &str) -> Tokens {
    let now_secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Tokens {
        access_token: resp.access_token,
        refresh_token: resp
            .refresh_token
            .unwrap_or_else(|| fallback_refresh.to_string()),
        expires_at_secs: now_secs.saturating_add(resp.expires_in),
    }
}

#[derive(serde::Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    expires_in: u64,
}

async fn bind_loopback() -> DropboxResult<(TcpListener, u16)> {
    for &port in LOOPBACK_PORTS {
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)).await {
            return Ok((listener, port));
        }
    }
    Err(DropboxError::Oauth(format!(
        "no free loopback port in {:?}",
        LOOPBACK_PORTS
    )))
}

fn build_authorize_url(app_key: &str, challenge: &str, state: &str, redirect_uri: &str) -> String {
    let mut url = "https://www.dropbox.com/oauth2/authorize?".to_string();
    let mut append = |key: &str, value: &str| {
        url.push_str(key);
        url.push('=');
        url.extend(url::form_urlencoded::byte_serialize(value.as_bytes()));
        url.push('&');
    };
    append("client_id", app_key);
    append("response_type", "code");
    append("code_challenge", challenge);
    append("code_challenge_method", "S256");
    append("token_access_type", "offline");
    append("state", state);
    append("redirect_uri", redirect_uri);
    // Drop trailing '&'.
    url.pop();
    url
}

struct ParsedCallback {
    code: Option<String>,
    state: String,
    error: Option<String>,
}

async fn read_callback(mut stream: tokio::net::TcpStream) -> DropboxResult<ParsedCallback> {
    let mut buf = [0u8; 8_192];
    let n = stream.read(&mut buf).await?;
    let request = std::str::from_utf8(&buf[..n])
        .map_err(|_| DropboxError::Oauth("callback contained invalid UTF-8".into()))?;

    let first_line = request
        .lines()
        .next()
        .ok_or_else(|| DropboxError::Oauth("empty HTTP request".into()))?;
    let path = first_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| DropboxError::Oauth("malformed HTTP request line".into()))?;

    let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
    let mut code: Option<String> = None;
    let mut state: Option<String> = None;
    let mut error: Option<String> = None;
    for (k, v) in url::form_urlencoded::parse(query.as_bytes()) {
        match k.as_ref() {
            "code" => code = Some(v.into_owned()),
            "state" => state = Some(v.into_owned()),
            "error" | "error_description" => error = Some(v.into_owned()),
            _ => {}
        }
    }

    let body = "<!doctype html><html><head><title>vibez</title>\
        <style>body{font-family:system-ui;background:#1a1a1a;color:#eee;\
        display:flex;align-items:center;justify-content:center;height:100vh;margin:0}\
        .card{background:#252525;padding:40px;border-radius:8px;border:1px solid #333}\
        h1{color:#ff8c00;margin-top:0}</style></head>\
        <body><div class=\"card\"><h1>vibez: connected</h1>\
        <p>You can close this tab and return to vibez.</p></div></body></html>";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;

    let state = state.ok_or_else(|| DropboxError::Oauth("no `state` in callback".into()))?;
    Ok(ParsedCallback { code, state, error })
}

fn code_challenge(verifier: &str) -> String {
    let digest = sha2::Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

fn random_url_safe_string(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::getrandom(&mut buf).expect("OS randomness must be available for OAuth");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_is_url_safe_base64() {
        let c = code_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
        assert_eq!(c, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn random_strings_have_expected_length_bucket() {
        let s = random_url_safe_string(32);
        // base64 encodes 32 bytes to 43 characters without padding.
        assert_eq!(s.len(), 43);
        let s2 = random_url_safe_string(32);
        assert_ne!(s, s2, "should produce a different random each call");
    }

    #[test]
    fn authorize_url_includes_required_params() {
        let url = build_authorize_url("abc", "CHALLENGE", "STATE", "http://127.0.0.1:53682");
        assert!(url.contains("client_id=abc"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("code_challenge=CHALLENGE"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("token_access_type=offline"));
        assert!(url.contains("state=STATE"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A53682"));
    }

    #[test]
    fn bind_loopback_returns_port_from_allowlist() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .unwrap();
        let (_listener, port) = rt.block_on(bind_loopback()).unwrap();
        assert!(LOOPBACK_PORTS.contains(&port));
    }
}
