use reqwest::Method;

use crate::auth::{Credentials, TokenResponse, TOKEN_URL};
use crate::error::{Error, Result};

const API_ROOT: &str = "https://developer.amazon.com/api/appstore/v1/applications";

/// A signed-in Amazon Appstore client. The access token is fetched on first
/// use and reused (Amazon's lasts an hour, longer than a CLI run).
pub struct AmazonClient {
    http: reqwest::Client,
    credentials: Credentials,
    token_url: String,
    root: String,
    token: tokio::sync::OnceCell<String>,
}

impl AmazonClient {
    pub fn new(credentials: Credentials) -> Self {
        Self {
            http: reqwest::Client::new(),
            credentials,
            token_url: TOKEN_URL.to_string(),
            root: API_ROOT.to_string(),
            token: tokio::sync::OnceCell::new(),
        }
    }

    /// Point at other hosts; used by the tests.
    #[doc(hidden)]
    pub fn with_endpoints(mut self, token_url: &str, root: &str) -> Self {
        self.token_url = token_url.to_string();
        self.root = root.to_string();
        self
    }

    pub fn client_id(&self) -> &str {
        &self.credentials.client_id
    }

    async fn token(&self) -> Result<&str> {
        self.token
            .get_or_try_init(|| async {
                let response = self
                    .http
                    .post(&self.token_url)
                    .form(&self.credentials.token_form())
                    .send()
                    .await?;
                let status = response.status();
                if !status.is_success() {
                    return Err(Error::Token {
                        status: status.as_u16(),
                        detail: response.text().await.unwrap_or_default(),
                    });
                }
                Ok(response.json::<TokenResponse>().await?.access_token)
            })
            .await
            .map(String::as_str)
    }

    pub(crate) fn url(&self, app_id: &str, path: &str) -> String {
        format!("{}/{app_id}{path}", self.root)
    }

    /// An authenticated request builder; the caller adds headers and body.
    pub(crate) async fn request(
        &self,
        method: Method,
        url: &str,
    ) -> Result<reqwest::RequestBuilder> {
        let token = self.token().await?;
        Ok(self.http.request(method, url).bearer_auth(token))
    }

    /// Send, and turn a non-2xx into [`Error::Api`].
    pub(crate) async fn send(&self, request: reqwest::RequestBuilder) -> Result<reqwest::Response> {
        let response = request.send().await?;
        let status = response.status();
        if status.is_success() {
            Ok(response)
        } else {
            Err(Error::Api {
                status: status.as_u16(),
                detail: response.text().await.unwrap_or_default(),
            })
        }
    }
}

/// Test helper: a one-shot HTTP server that records raw requests and answers
/// each with the next canned response.
#[cfg(test)]
pub(crate) mod testserver {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Returns the base URL and a handle yielding the raw requests received.
    pub async fn serve(
        responses: Vec<&'static str>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = tokio::spawn(async move {
            let mut seen = Vec::new();
            for body in responses {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let n = sock.read(&mut chunk).await.unwrap();
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..end]).to_lowercase();
                        let len = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        if buf.len() >= end + 4 + len || n == 0 {
                            break;
                        }
                    } else if n == 0 {
                        break;
                    }
                }
                seen.push(String::from_utf8_lossy(&buf).to_string());
                sock.write_all(body.as_bytes()).await.unwrap();
                sock.shutdown().await.ok();
            }
            seen
        });
        (base, handle)
    }
}
