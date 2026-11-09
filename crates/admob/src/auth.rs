//! Installed-app OAuth for AdMob: PKCE, a loopback redirect, and a cached
//! refresh token. Nothing here prints a token or a secret.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::distributions::Alphanumeric;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::error::{Error, Result};

/// `admob.readonly` covers the lists; `admob.monetization` is what
/// `adUnits.create` needs.
pub const SCOPES: &str = "https://www.googleapis.com/auth/admob.readonly \
                          https://www.googleapis.com/auth/admob.monetization";
const DEFAULT_AUTH_URI: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const DEFAULT_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";
const REFRESH_MARGIN: Duration = Duration::from_secs(60);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

/// A Desktop-app OAuth client, as downloaded from Google Cloud Console.
#[derive(Clone)]
pub struct OAuthClient {
    pub client_id: String,
    client_secret: String,
    auth_uri: String,
    token_uri: String,
}

// Hand-written so `{:?}` can never print the secret.
impl std::fmt::Debug for OAuthClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthClient")
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .finish()
    }
}

#[derive(Deserialize)]
struct ClientFile {
    installed: Option<RawClient>,
    web: Option<RawClient>,
}

#[derive(Deserialize)]
struct RawClient {
    client_id: String,
    client_secret: String,
    #[serde(default)]
    auth_uri: Option<String>,
    #[serde(default)]
    token_uri: Option<String>,
}

impl OAuthClient {
    pub fn from_json(json: &str) -> Result<Self> {
        let file: ClientFile = serde_json::from_str(json).map_err(|_| Error::ClientInvalid)?;
        let raw = file.installed.or(file.web).ok_or(Error::ClientInvalid)?;
        if raw.client_id.is_empty() || raw.client_secret.is_empty() {
            return Err(Error::ClientInvalid);
        }
        Ok(Self {
            client_id: raw.client_id,
            client_secret: raw.client_secret,
            auth_uri: raw.auth_uri.unwrap_or_else(|| DEFAULT_AUTH_URI.to_string()),
            token_uri: raw
                .token_uri
                .unwrap_or_else(|| DEFAULT_TOKEN_URI.to_string()),
        })
    }

    pub fn from_json_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let json = std::fs::read_to_string(path).map_err(|source| Error::ClientRead {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_json(&json)
    }
}

/// A PKCE verifier and its S256 challenge (RFC 7636).
#[derive(Clone)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl std::fmt::Debug for Pkce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pkce")
            .field("verifier", &"<redacted>")
            .field("challenge", &self.challenge)
            .finish()
    }
}

impl Pkce {
    pub fn generate() -> Self {
        Self::from_verifier(random_string(64))
    }

    pub fn from_verifier(verifier: String) -> Self {
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        Self {
            verifier,
            challenge,
        }
    }
}

fn random_string(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

/// What is cached on disk: the refresh token, nothing else.
#[derive(Clone, Serialize, Deserialize)]
pub struct StoredToken {
    pub refresh_token: String,
}

impl std::fmt::Debug for StoredToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredToken")
            .field("refresh_token", &"<redacted>")
            .finish()
    }
}

impl StoredToken {
    /// `~/.config/asc/admob-token.json`.
    pub fn default_path() -> PathBuf {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        home.join(".config").join("asc").join("admob-token.json")
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let io = |source| Error::TokenCache {
            path: path.display().to_string(),
            source,
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let json = serde_json::to_vec_pretty(self)?;
        write_private(path, &json).map_err(io)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let json = std::fs::read_to_string(path).map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                Error::NotSignedIn {
                    path: path.display().to_string(),
                }
            } else {
                Error::TokenCache {
                    path: path.display().to_string(),
                    source,
                }
            }
        })?;
        serde_json::from_str(&json).map_err(|_| Error::TokenCacheInvalid {
            path: path.display().to_string(),
        })
    }
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    // `mode` only applies on creation; tighten a pre-existing file too.
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.write_all(bytes)
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    refresh_token: Option<String>,
}

/// The consent URL for one login attempt.
pub fn authorization_url(
    client: &OAuthClient,
    redirect_uri: &str,
    pkce: &Pkce,
    state: &str,
) -> String {
    reqwest::Url::parse_with_params(
        &client.auth_uri,
        &[
            ("client_id", client.client_id.as_str()),
            ("redirect_uri", redirect_uri),
            ("response_type", "code"),
            ("scope", SCOPES),
            ("code_challenge", pkce.challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("state", state),
            // Without these Google may omit the refresh token.
            ("access_type", "offline"),
            ("prompt", "consent"),
        ],
    )
    .map(|url| url.to_string())
    .unwrap_or_default()
}

/// Pull the authorization code out of the loopback request line
/// (`GET /?code=…&state=… HTTP/1.1`), checking `state`.
pub fn parse_callback(request_line: &str, expected_state: &str) -> Result<String> {
    let target = request_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| Error::Authorization("malformed redirect request".to_string()))?;
    let url = reqwest::Url::parse(&format!("http://127.0.0.1{target}"))
        .map_err(|_| Error::Authorization("malformed redirect request".to_string()))?;
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            _ => {}
        }
    }
    if let Some(error) = error {
        return Err(Error::Authorization(format!("Google returned {error}")));
    }
    if state.as_deref() != Some(expected_state) {
        return Err(Error::Authorization(
            "the redirect's state did not match; start the login again".to_string(),
        ));
    }
    code.ok_or_else(|| Error::Authorization("the redirect carried no code".to_string()))
}

fn open_browser(url: &str) {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(program).arg(url).spawn();
}

/// Run the browser sign-in once and cache the refresh token at `token_path`.
pub async fn login(client: &OAuthClient, token_path: &Path) -> Result<()> {
    let http = reqwest::Client::new();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| Error::Authorization(format!("cannot open a loopback port: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| Error::Authorization(e.to_string()))?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}");
    let pkce = Pkce::generate();
    let state = random_string(32);
    let url = authorization_url(client, &redirect_uri, &pkce, &state);

    eprintln!("Opening the browser to sign in to AdMob. If it does not open, visit:\n{url}");
    open_browser(&url);

    let code = tokio::time::timeout(LOGIN_TIMEOUT, async {
        let (mut socket, _) = listener
            .accept()
            .await
            .map_err(|e| Error::Authorization(e.to_string()))?;
        let mut buf = vec![0u8; 8192];
        let n = socket
            .read(&mut buf)
            .await
            .map_err(|e| Error::Authorization(e.to_string()))?;
        let request = String::from_utf8_lossy(&buf[..n]).into_owned();
        let line = request.lines().next().unwrap_or_default();
        let parsed = parse_callback(line, &state);
        let body = if parsed.is_ok() {
            "Signed in. You can close this tab and return to the terminal."
        } else {
            "Sign-in failed. Return to the terminal for details."
        };
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = socket.write_all(reply.as_bytes()).await;
        parsed
    })
    .await
    .map_err(|_| Error::Authorization("timed out waiting for the browser sign-in".to_string()))??;

    let response = http
        .post(&client.token_uri)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("client_id", client.client_id.as_str()),
            ("client_secret", client.client_secret.as_str()),
            ("code_verifier", pkce.verifier.as_str()),
        ])
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        return Err(Error::Token {
            status: status.as_u16(),
            detail: response.text().await.unwrap_or_default(),
        });
    }
    let body: TokenResponse = response.json().await?;
    let refresh_token = body.refresh_token.ok_or_else(|| {
        Error::Authorization(
            "Google returned no refresh token; revoke the app's access in your Google account and log in again"
                .to_string(),
        )
    })?;
    StoredToken { refresh_token }.save(token_path)
}

/// Turns the cached refresh token into short-lived access tokens.
pub(crate) struct Authenticator {
    client: OAuthClient,
    token_path: PathBuf,
    cached: Mutex<Option<(String, Instant)>>,
}

impl Authenticator {
    pub(crate) fn new(client: OAuthClient, token_path: PathBuf) -> Self {
        Self {
            client,
            token_path,
            cached: Mutex::new(None),
        }
    }

    pub(crate) async fn access_token(&self, http: &reqwest::Client) -> Result<String> {
        let mut cached = self.cached.lock().await;
        if let Some((token, expires)) = cached.as_ref() {
            if Instant::now() < *expires {
                return Ok(token.clone());
            }
        }
        let stored = StoredToken::load(&self.token_path)?;
        let response = http
            .post(&self.client.token_uri)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", stored.refresh_token.as_str()),
                ("client_id", self.client.client_id.as_str()),
                ("client_secret", self.client.client_secret.as_str()),
            ])
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(Error::Token {
                status: status.as_u16(),
                detail: response.text().await.unwrap_or_default(),
            });
        }
        let body: TokenResponse = response.json().await?;
        let lifetime = Duration::from_secs(body.expires_in.unwrap_or(3600));
        let expires = Instant::now() + lifetime.saturating_sub(REFRESH_MARGIN);
        *cached = Some((body.access_token.clone(), expires));
        Ok(body.access_token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIENT_JSON: &str =
        r#"{"installed":{"client_id":"cid.apps.googleusercontent.com","client_secret":"SECRET"}}"#;

    #[test]
    fn pkce_challenge_matches_the_rfc_7636_vector() {
        let pkce = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".to_string());
        assert_eq!(
            pkce.challenge,
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn generated_pkce_pairs_are_unique_and_in_range() {
        let a = Pkce::generate();
        let b = Pkce::generate();
        assert_ne!(a.verifier, b.verifier);
        assert!((43..=128).contains(&a.verifier.len()));
        assert_eq!(
            a.challenge,
            Pkce::from_verifier(a.verifier.clone()).challenge
        );
    }

    #[test]
    fn token_cache_round_trips_with_owner_only_permissions() {
        let dir = std::env::temp_dir().join(format!("asc-admob-test-{}", random_string(8)));
        let path = dir.join("nested").join("admob-token.json");
        StoredToken {
            refresh_token: "r-1".to_string(),
        }
        .save(&path)
        .unwrap();
        assert_eq!(StoredToken::load(&path).unwrap().refresh_token, "r-1");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_cache_says_to_log_in() {
        let err = StoredToken::load(Path::new("/nonexistent/admob-token.json")).unwrap_err();
        assert!(err.to_string().contains("asc admob login"));
    }

    #[test]
    fn debug_output_redacts_secrets() {
        let client = OAuthClient::from_json(CLIENT_JSON).unwrap();
        assert!(!format!("{client:?}").contains("SECRET"));
        let token = StoredToken {
            refresh_token: "REFRESH".to_string(),
        };
        assert!(!format!("{token:?}").contains("REFRESH"));
        let pkce = Pkce::from_verifier("VERIFIER".to_string());
        assert!(!format!("{pkce:?}").contains("VERIFIER"));
    }

    #[test]
    fn an_invalid_client_file_is_rejected_without_echoing_it() {
        let err = OAuthClient::from_json(r#"{"installed":{"client_id":"a","client_secret":""}}"#)
            .unwrap_err()
            .to_string();
        assert!(!err.contains("\"a\""));
        assert!(OAuthClient::from_json("nope").is_err());
        assert!(OAuthClient::from_json("{}").is_err());
    }

    #[test]
    fn authorization_url_carries_pkce_scopes_and_offline_access() {
        let client = OAuthClient::from_json(CLIENT_JSON).unwrap();
        let pkce = Pkce::from_verifier("v".repeat(43));
        let url = authorization_url(&client, "http://127.0.0.1:5000", &pkce, "st");
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("access_type=offline"));
        assert!(url.contains("admob.monetization"));
        assert!(url.contains("state=st"));
        assert!(!url.contains("SECRET"));
    }

    #[test]
    fn callback_yields_the_code_and_rejects_a_wrong_state() {
        assert_eq!(
            parse_callback("GET /?code=abc%2F1&state=st HTTP/1.1", "st").unwrap(),
            "abc/1"
        );
        assert!(parse_callback("GET /?code=abc&state=other HTTP/1.1", "st").is_err());
        assert!(parse_callback("GET /?error=access_denied&state=st HTTP/1.1", "st").is_err());
        assert!(parse_callback("garbage", "st").is_err());
    }
}
