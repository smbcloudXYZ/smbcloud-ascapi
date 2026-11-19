use std::collections::HashMap;
use std::path::Path;

use crate::error::{Error, Result};

pub const KEY_APP_ID: &str = "XIAOMI_APP_ID";
pub const KEY_APP_SECRET: &str = "XIAOMI_APP_SECRET";
pub const KEY_PACKAGE: &str = "XIAOMI_PACKAGE";
pub const KEY_EMAIL: &str = "XIAOMI_EMAIL";
pub const KEY_DEVELOPER_ID: &str = "XIAOMI_DEVELOPER_ID";
pub const KEY_USER_ID: &str = "XIAOMI_USER_ID";

/// The App ID and App Secret are per app (the app's page in the console);
/// the email, developer id and Xiaomi user id belong to the developer
/// account. GetApps checks that all of them belong together.
#[derive(Clone)]
pub struct Credentials {
    pub app_id: String,
    app_secret: String,
    pub package: String,
    pub email: String,
    pub developer_id: String,
    pub user_id: String,
}

// Hand-written so `{:?}` can never print the App Secret.
impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("app_id", &self.app_id)
            .field("app_secret", &"<redacted>")
            .field("package", &self.package)
            .field("email", &self.email)
            .field("developer_id", &self.developer_id)
            .field("user_id", &self.user_id)
            .finish()
    }
}

/// Parse a dotenv file: `KEY=VALUE` lines, `#` comments, an optional
/// `export ` prefix, and one pair of matching quotes around a value.
pub fn parse_dotenv(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        let value = ['"', '\'']
            .iter()
            .find_map(|q| value.strip_prefix(*q).and_then(|v| v.strip_suffix(*q)))
            .unwrap_or(value);
        map.insert(key.trim().to_string(), value.to_string());
    }
    map
}

impl Credentials {
    pub fn new(
        app_id: impl Into<String>,
        app_secret: impl Into<String>,
        package: impl Into<String>,
        email: impl Into<String>,
        developer_id: impl Into<String>,
        user_id: impl Into<String>,
    ) -> Self {
        Self {
            app_id: app_id.into(),
            app_secret: app_secret.into(),
            package: package.into(),
            email: email.into(),
            developer_id: developer_id.into(),
            user_id: user_id.into(),
        }
    }

    /// Build from a key lookup. A key that is absent or empty is an error
    /// naming the key; so is a developer or user id that is not a number.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let need = |key: &'static str| {
            get(key)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .ok_or(Error::MissingCredential(key))
        };
        let numeric = |key: &'static str| {
            let value = need(key)?;
            if value.bytes().all(|b| b.is_ascii_digit()) {
                Ok(value)
            } else {
                Err(Error::NotNumeric(key))
            }
        };
        Ok(Self {
            app_id: need(KEY_APP_ID)?,
            app_secret: need(KEY_APP_SECRET)?,
            package: need(KEY_PACKAGE)?,
            email: need(KEY_EMAIL)?,
            developer_id: numeric(KEY_DEVELOPER_ID)?,
            user_id: numeric(KEY_USER_ID)?,
        })
    }

    pub fn from_env_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|source| Error::EnvRead {
            path: path.display().to_string(),
            source,
        })?;
        let map = parse_dotenv(&text);
        Self::from_lookup(|key| map.get(key).cloned())
    }

    pub fn from_process_env() -> Result<Self> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    pub(crate) fn app_secret(&self) -> &str {
        &self.app_secret
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full() -> HashMap<&'static str, &'static str> {
        HashMap::from([
            (KEY_APP_ID, "2882303761521427725"),
            (KEY_APP_SECRET, "s3cret"),
            (KEY_PACKAGE, "com.example.app"),
            (KEY_EMAIL, "dev@example.com"),
            (KEY_DEVELOPER_ID, "1080405"),
            (KEY_USER_ID, "2410896325"),
        ])
    }

    #[test]
    fn reads_every_key() {
        let map = full();
        let c = Credentials::from_lookup(|k| map.get(k).map(|v| v.to_string())).unwrap();
        assert_eq!(c.app_id, "2882303761521427725");
        assert_eq!(c.package, "com.example.app");
        assert_eq!(c.app_secret(), "s3cret");
    }

    #[test]
    fn names_the_missing_key() {
        let mut map = full();
        map.insert(KEY_EMAIL, "  ");
        let e = Credentials::from_lookup(|k| map.get(k).map(|v| v.to_string())).unwrap_err();
        assert!(matches!(e, Error::MissingCredential(KEY_EMAIL)));
    }

    #[test]
    fn rejects_a_non_numeric_user_id() {
        let mut map = full();
        map.insert(KEY_USER_ID, "2410896325xx");
        let e = Credentials::from_lookup(|k| map.get(k).map(|v| v.to_string())).unwrap_err();
        assert!(matches!(e, Error::NotNumeric(KEY_USER_ID)));
    }

    #[test]
    fn debug_hides_the_secret() {
        let map = full();
        let c = Credentials::from_lookup(|k| map.get(k).map(|v| v.to_string())).unwrap();
        let shown = format!("{c:?}");
        assert!(!shown.contains("s3cret"));
        assert!(shown.contains("<redacted>"));
    }

    #[test]
    fn dotenv_strips_quotes_and_export() {
        let map = parse_dotenv("# c\nexport XIAOMI_APP_ID=\"1\"\nXIAOMI_EMAIL='a@b'\n");
        assert_eq!(map["XIAOMI_APP_ID"], "1");
        assert_eq!(map["XIAOMI_EMAIL"], "a@b");
    }
}
