//! Server configuration from the environment. Nothing secret has a default, and no SMTP host, SMTP password or
//! licence key exists anywhere (D-05). The only secret is `RESEND_API_KEY` (EXT-01, I4), read once at start and kept
//! out of `Debug` output (AP-13).

use std::fmt;
use std::net::SocketAddr;
use std::path::PathBuf;

/// A value that never prints: `Debug` and `Display` show `***` (BR-SYS-02 log masking, AP-13).
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    /// The value, for the one call that needs it (the provider request header).
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    /// `MBA_BIND`, default `127.0.0.1:8080`.
    pub bind: SocketAddr,
    /// `MBA_DB`, default `data/mba.db`.
    pub db_path: PathBuf,
    /// `MBA_DATA_DIR`, default the folder of the database; uploaded files go under `files/` (OP-SYS-04).
    pub data_dir: PathBuf,
    /// `MBA_SECURE_COOKIE=1` adds `Secure` to the session cookie. Off by default: the demo runs on plain
    /// `http://127.0.0.1`. Turn it on behind HTTPS (AP-02).
    pub secure_cookie: bool,
    /// `RESEND_API_KEY`: set -> Resend provider; unset -> outbox mock (EXT-01, D-05). Used from I4.
    pub resend_api_key: Option<Secret>,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let bind = std::env::var("MBA_BIND").unwrap_or_else(|_| "127.0.0.1:8080".into());
        let bind: SocketAddr = bind.parse().map_err(|e| format!("MBA_BIND `{bind}`: {e}"))?;
        let db_path = PathBuf::from(std::env::var("MBA_DB").unwrap_or_else(|_| "data/mba.db".into()));
        let data_dir = match std::env::var("MBA_DATA_DIR") {
            Ok(d) => PathBuf::from(d),
            Err(_) => db_path.parent().map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")),
        };
        let secure_cookie = std::env::var("MBA_SECURE_COOKIE").is_ok_and(|v| v == "1");
        let resend_api_key = std::env::var("RESEND_API_KEY").ok().filter(|k| !k.is_empty()).map(Secret::new);
        Ok(Self { bind, db_path, data_dir, secure_cookie, resend_api_key })
    }

    /// For tests: a database path, everything else default.
    pub fn for_db(db_path: impl Into<PathBuf>) -> Self {
        let db_path = db_path.into();
        let data_dir = db_path.parent().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        Self {
            bind: "127.0.0.1:0".parse().expect("addr"),
            db_path,
            data_dir,
            secure_cookie: false,
            resend_api_key: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_never_prints() {
        let mut c = Config::for_db("x.db");
        c.resend_api_key = Some(Secret::new("re_live_value"));
        let shown = format!("{c:?} {}", c.resend_api_key.as_ref().unwrap());
        assert!(!shown.contains("re_live_value"), "{shown}");
        assert!(shown.contains("***"));
    }
}
