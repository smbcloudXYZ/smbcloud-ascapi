//! Xiaomi GetApps operations shared by `asc xiaomi` and, later, MCP tools.
//!
//! Only what Xiaomi documents publicly: the order query and the APK upload
//! address. A response can carry a signed upload URL, so URLs in it are cut
//! to scheme, host and path before printing.

use std::path::Path;

use serde_json::{Map, Value};
use smbcloud_ascapi_xiaomi::{Credentials, OrderQuery, Prepared, XiaomiClient};

/// Resolve the client. `path` wins; then `XIAOMI_ENV_PATH`; then the
/// `XIAOMI_*` variables set directly. The App Secret never appears in an
/// error.
pub fn xiaomi_client(path: Option<&Path>) -> Result<XiaomiClient, String> {
    let credentials = if let Some(path) = path {
        Credentials::from_env_file(path)
    } else if let Ok(path) = std::env::var("XIAOMI_ENV_PATH") {
        Credentials::from_env_file(path)
    } else if std::env::var_os("XIAOMI_APP_SECRET").is_some() {
        Credentials::from_process_env()
    } else {
        return Err("no Xiaomi GetApps credentials: pass --xiaomi-env, or set \
                    XIAOMI_ENV_PATH (a dotenv file) or the XIAOMI_* variables"
            .to_string());
    };
    credentials
        .map(XiaomiClient::new)
        .map_err(|e| e.to_string())
}

/// Send `prepared`, or with `dry_run` describe it and send nothing. The
/// response has its URLs redacted.
async fn run(client: &XiaomiClient, prepared: Prepared, dry_run: bool) -> Result<Value, String> {
    if dry_run {
        return Ok(prepared.describe());
    }
    client
        .send(prepared)
        .await
        .map(|data| redact_urls(&data))
        .map_err(|e| e.to_string())
}

/// Orders for the query. Read-only, but `dry_run` still skips the request so
/// a signature can be checked without spending the one-a-second budget.
pub async fn orders(
    client: &XiaomiClient,
    query: &OrderQuery,
    dry_run: bool,
) -> Result<Value, String> {
    query.validate()?;
    let prepared = client.orders_request(query).map_err(|e| e.to_string())?;
    run(client, prepared, dry_run).await
}

/// The APK upload address for the client's package. Fetching it uploads
/// nothing.
pub async fn upload_url(client: &XiaomiClient, dry_run: bool) -> Result<Value, String> {
    let prepared = client.upload_url_request().map_err(|e| e.to_string())?;
    run(client, prepared, dry_run).await
}

/// Copy of `value` with every `http(s)://` string reduced to scheme, host and
/// path, so a signed query string is never printed.
pub fn redact_urls(value: &Value) -> Value {
    match value {
        Value::String(s) if s.starts_with("http://") || s.starts_with("https://") => {
            let base = s.split(['?', '#']).next().unwrap_or(s);
            if base.len() < s.len() {
                Value::String(format!("{base}?<redacted>"))
            } else {
                value.clone()
            }
        }
        Value::Array(items) => Value::Array(items.iter().map(redact_urls).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), redact_urls(v)))
                .collect::<Map<_, _>>(),
        ),
        _ => value.clone(),
    }
}

/// Parse a time argument: Unix seconds, or a `YYYY-MM-DD` date taken as
/// 00:00 UTC.
pub fn parse_time(arg: &str) -> Result<u64, String> {
    let arg = arg.trim();
    if !arg.is_empty() && arg.bytes().all(|b| b.is_ascii_digit()) {
        return arg.parse().map_err(|e| format!("{arg}: {e}"));
    }
    let parts: Vec<&str> = arg.split('-').collect();
    let [y, m, d] = parts.as_slice() else {
        return Err(format!("{arg}: expected Unix seconds or YYYY-MM-DD"));
    };
    let (y, m, d): (i64, i64, i64) = match (y.parse(), m.parse(), d.parse()) {
        (Ok(y), Ok(m), Ok(d)) if (1..=12).contains(&m) && (1..=31).contains(&d) => (y, m, d),
        _ => return Err(format!("{arg}: expected Unix seconds or YYYY-MM-DD")),
    };
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400).map_err(|_| format!("{arg}: before 1970"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn signed_query_strings_are_cut() {
        let v = json!({"data": {"uploadUrl": "https://bucket.example/k?Signature=abc&Expires=1",
                                "plain": "https://example.com/a", "n": 1}});
        assert_eq!(
            redact_urls(&v),
            json!({"data": {"uploadUrl": "https://bucket.example/k?<redacted>",
                            "plain": "https://example.com/a", "n": 1}})
        );
    }

    #[test]
    fn times_parse_as_seconds_or_dates() {
        assert_eq!(parse_time("1703134938").unwrap(), 1_703_134_938);
        assert_eq!(parse_time("1970-01-01").unwrap(), 0);
        assert_eq!(parse_time("2026-11-19").unwrap(), 1_795_046_400);
        assert!(parse_time("2026-13-01").is_err());
        assert!(parse_time("yesterday").is_err());
    }
}
