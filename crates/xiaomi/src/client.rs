use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::Method;
use serde::Serialize;
use serde_json::Value;

use crate::auth::Credentials;
use crate::error::{Error, Result};
use crate::sign::{new_nonce, SignedHeaders};

pub const API_ROOT: &str = "https://global.developer.mi.com";
/// Order query. Needs allowlisting: email overseas-dev@xiaomi.com. At most one
/// request a second and 30 days per query.
pub const ORDER_QUERY_PATH: &str = "/third-api/order/v1/query";
/// The APK upload address. Named in Xiaomi's header page as the GET example;
/// its response shape is not public.
pub const UPLOAD_URL_PATH: &str = "/third-api/apk/v1/getUploadUrl";

/// Body of the order query. Lists are comma-separated, as the API takes them.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderQuery {
    pub pkgs: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regions: Option<String>,
    /// 1 click to pay, 2 paid, 3 closed, 4 refund pending, 5 refund started,
    /// 6 refunded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pay_status: Option<String>,
    /// 0 normal, 1 test, 2 promotion, 3 incentive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pay_type: Option<i64>,
    pub start_timestamp: u64,
    pub end_timestamp: u64,
}

/// Longest window the order query accepts.
pub const ORDER_QUERY_MAX_SECS: u64 = 30 * 24 * 60 * 60;

impl OrderQuery {
    /// The checks GetApps would otherwise answer with a 400.
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.pkgs.trim().is_empty() {
            return Err("at least one package is required".into());
        }
        if self.end_timestamp <= self.start_timestamp {
            return Err("the end time must be after the start time".into());
        }
        if self.end_timestamp - self.start_timestamp > ORDER_QUERY_MAX_SECS {
            return Err("the window is longer than 30 days".into());
        }
        Ok(())
    }
}

/// A prepared request: what would be sent, for `--dry-run`. Holds the
/// signature, never the secret.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub method: Method,
    pub url: String,
    pub headers: SignedHeaders,
    pub body: Option<String>,
}

impl Prepared {
    /// What `--dry-run` prints: the method, URL, headers, canonical string
    /// and body. The `Authorization` value is withheld: it is valid for 30
    /// minutes.
    pub fn describe(&self) -> Value {
        let headers: serde_json::Map<String, Value> = self
            .headers
            .pairs()
            .iter()
            .map(|(name, value)| {
                let shown = if *name == "Authorization" {
                    "<signature withheld>"
                } else {
                    value
                };
                (name.to_string(), Value::String(shown.to_string()))
            })
            .collect();
        serde_json::json!({
            "method": self.method.as_str(),
            "url": self.url,
            "headers": headers,
            "canonical": self.headers.canonical,
            "body": self.body,
        })
    }
}

pub struct XiaomiClient {
    http: reqwest::Client,
    credentials: Credentials,
    root: String,
}

impl XiaomiClient {
    pub fn new(credentials: Credentials) -> Self {
        Self {
            http: reqwest::Client::new(),
            credentials,
            root: API_ROOT.to_string(),
        }
    }

    /// Point at another host, for tests against a local server.
    pub fn with_root(mut self, root: impl Into<String>) -> Self {
        self.root = root.into();
        self
    }

    pub fn package(&self) -> &str {
        &self.credentials.package
    }

    /// Sign a request without sending it. `body` is serialized once here, and
    /// those exact bytes are both hashed and sent.
    pub fn prepare(
        &self,
        method: Method,
        path: &str,
        body: Option<&impl Serialize>,
    ) -> Result<Prepared> {
        let body = body.map(serde_json::to_string).transpose()?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();
        let headers = SignedHeaders::new(
            &self.credentials,
            method.as_str(),
            path,
            body.as_deref().map(str::as_bytes),
            timestamp,
            new_nonce(),
        );
        Ok(Prepared {
            method,
            url: format!("{}{path}", self.root),
            headers,
            body,
        })
    }

    /// Send a prepared request and return the envelope's `data`. An HTTP error
    /// or an envelope `code` other than 200 is an error carrying Xiaomi's
    /// message.
    pub async fn send(&self, prepared: Prepared) -> Result<Value> {
        let mut request = self
            .http
            .request(prepared.method, &prepared.url)
            .header("Content-Type", "application/json");
        for (name, value) in prepared.headers.pairs() {
            request = request.header(name, value);
        }
        if let Some(body) = prepared.body {
            request = request.body(body);
        }
        let response = request.send().await?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(Error::Http2xx {
                status: status.as_u16(),
                detail: text,
            });
        }
        unwrap_envelope(serde_json::from_str(&text)?)
    }

    pub fn orders_request(&self, query: &OrderQuery) -> Result<Prepared> {
        self.prepare(Method::POST, ORDER_QUERY_PATH, Some(query))
    }

    pub fn upload_url_request(&self) -> Result<Prepared> {
        self.prepare(Method::GET, UPLOAD_URL_PATH, None::<&()>)
    }
}

/// `{"code": 200, "msg": …, "data": …}` → `data`. Any other code is an error.
pub(crate) fn unwrap_envelope(envelope: Value) -> Result<Value> {
    let code = envelope.get("code").and_then(Value::as_i64).unwrap_or(-1);
    if code == 200 {
        return Ok(envelope.get("data").cloned().unwrap_or(Value::Null));
    }
    Err(Error::Api {
        code,
        msg: envelope
            .get("msg")
            .and_then(Value::as_str)
            .unwrap_or("no message")
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn query() -> OrderQuery {
        OrderQuery {
            pkgs: "com.example.app".into(),
            regions: None,
            pay_status: Some("2,6".into()),
            pay_type: None,
            start_timestamp: 1_701_084_799,
            end_timestamp: 1_702_684_799,
        }
    }

    #[test]
    fn order_body_is_camel_case_and_skips_unset() {
        let body = serde_json::to_value(query()).unwrap();
        assert_eq!(
            body,
            json!({"pkgs":"com.example.app","payStatus":"2,6",
                   "startTimestamp":1_701_084_799u64,"endTimestamp":1_702_684_799u64})
        );
    }

    #[test]
    fn order_window_is_checked() {
        assert!(query().validate().is_ok());
        let mut q = query();
        q.end_timestamp = q.start_timestamp;
        assert!(q.validate().is_err());
        q.end_timestamp = q.start_timestamp + ORDER_QUERY_MAX_SECS + 1;
        assert!(q.validate().unwrap_err().contains("30 days"));
    }

    #[test]
    fn envelope_200_yields_data() {
        let data = unwrap_envelope(json!({"code":200,"msg":"ok","data":[1]})).unwrap();
        assert_eq!(data, json!([1]));
    }

    #[test]
    fn envelope_error_keeps_code_and_message() {
        let e = unwrap_envelope(json!({"code":1002,"msg":"Signature mismatch!"})).unwrap_err();
        assert!(matches!(e, Error::Api { code: 1002, ref msg } if msg == "Signature mismatch!"));
    }

    #[test]
    fn prepare_hashes_the_bytes_it_sends() {
        let credentials = Credentials::new("1", "secret", "com.example.app", "a@b", "2", "3");
        let client = XiaomiClient::new(credentials);
        let p = client
            .prepare(Method::POST, ORDER_QUERY_PATH, Some(&query()))
            .unwrap();
        let body = p.body.unwrap();
        let md5 = crate::sign::body_md5(body.as_bytes());
        assert!(p
            .headers
            .canonical
            .starts_with(&format!("POST{md5}AppId:1")));
        assert!(p.headers.canonical.ends_with(ORDER_QUERY_PATH));
        assert_eq!(p.url, format!("{API_ROOT}{ORDER_QUERY_PATH}"));
    }
}
