//! Audit stamps, the audit trail and log masking (BR-SYS-02; 05 "Errors, concurrency, and observability").
//!
//! * Stamps: every entity row carries `created_at`/`created_by`/`modified_at`/`modified_by`. `*_by` is the
//!   username from the session (08 "Session and global state"); rows the app writes itself carry [`SYSTEM`].
//!   Commands write the row with plain SQL and call [`stamp_created`] or [`stamp_modified`] in the same transaction.
//! * Trail: [`log`] writes one `audit_log` row (auth events, deletes, voids, field changes from phase 2).
//! * Masking: the request log carries method, path, status, time, user and request id. It never carries a body, a
//!   query string, a cookie or a header value, so passwords, session tokens and CSRF values cannot reach it.

use axum::extract::Request;
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::Response;
use rusqlite::types::ToSql;
use rusqlite::{params, Connection};

/// `created_by` of rows the app writes without a user (migrations, the seed tool, jobs).
pub const SYSTEM: &str = "system";

/// Who writes a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub user_id: Option<i64>,
    pub username: String,
}

impl Actor {
    pub fn user(user_id: i64, username: impl Into<String>) -> Self {
        Self { user_id: Some(user_id), username: username.into() }
    }
    pub fn system() -> Self {
        Self { user_id: None, username: SYSTEM.into() }
    }
}

/// Sets the four stamp columns of the new row (`key_col` = `key`) of `table`. Table and column names come from code,
/// never from a request.
pub fn stamp_created(
    conn: &Connection,
    table: &str,
    key_col: &str,
    key: &dyn ToSql,
    actor: &Actor,
) -> rusqlite::Result<usize> {
    let now = crate::clock::now_text();
    conn.execute(
        &format!(
            "UPDATE {table} SET created_at = ?1, created_by = ?2, modified_at = ?1, modified_by = ?2 WHERE {key_col} = ?3"
        ),
        params![now, actor.username, key],
    )
}

/// Sets `modified_at`/`modified_by` of one row.
pub fn stamp_modified(
    conn: &Connection,
    table: &str,
    key_col: &str,
    key: &dyn ToSql,
    actor: &Actor,
) -> rusqlite::Result<usize> {
    conn.execute(
        &format!("UPDATE {table} SET modified_at = ?1, modified_by = ?2 WHERE {key_col} = ?3"),
        params![crate::clock::now_text(), actor.username, key],
    )
}

/// One `audit_log` row. `detail` is JSON; callers never put a password, token or CSRF value in it.
pub fn log(
    conn: &Connection,
    user_id: Option<i64>,
    action: &str,
    entity: Option<&str>,
    entity_uuid: Option<&str>,
    detail: Option<&serde_json::Value>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO audit_log (at, user_id, action, entity, entity_uuid, detail) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![crate::clock::now_text(), user_id, action, entity, entity_uuid, detail.map(|d| d.to_string())],
    )?;
    Ok(())
}

/// The request id of a request (`x-request-id`), set by [`request_log`].
#[derive(Clone, Debug)]
pub struct RequestId(pub String);

/// The signed-in username, for the log line; `require_session` puts it on the response.
#[derive(Clone, Debug)]
pub struct LogUser(pub String);

/// One log line per request: id, method, path (no query), status, time, user. Never the body or headers.
pub async fn request_log(mut req: Request, next: Next) -> Response {
    let id = uuid::Uuid::new_v4().simple().to_string()[..12].to_owned();
    req.extensions_mut().insert(RequestId(id.clone()));
    let method = req.method().clone();
    let path = log_path(req.uri().path());
    let started = std::time::Instant::now();
    let mut res = next.run(req).await;
    let user = res.extensions().get::<LogUser>().map(|u| u.0.clone()).unwrap_or_else(|| "-".into());
    tracing::info!(
        request_id = %id,
        user = %user,
        "{} {} {} {}ms",
        method,
        path,
        res.status().as_u16(),
        started.elapsed().as_millis()
    );
    if let Ok(v) = HeaderValue::from_str(&id) {
        res.headers_mut().insert("x-request-id", v);
    }
    res
}

/// The path as logged: printable ASCII only, at most 200 characters. The query string is never passed in.
pub fn log_path(path: &str) -> String {
    path.chars().filter(|c| c.is_ascii_graphic()).take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_and_trail() {
        let conn = crate::db::open_in_memory().unwrap();
        conn.execute("INSERT INTO affiliates (uuid, socio) VALUES ('a1', 'Banco Ejemplo')", []).unwrap();
        let actor = Actor::user(1, "mvega");
        assert_eq!(stamp_created(&conn, "affiliates", "uuid", &"a1", &actor).unwrap(), 1);
        let (cb, mb): (String, String) = conn
            .query_row("SELECT created_by, modified_by FROM affiliates WHERE uuid = 'a1'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((cb.as_str(), mb.as_str()), ("mvega", "mvega"));
        stamp_modified(&conn, "affiliates", "uuid", &"a1", &Actor::system()).unwrap();
        let mb: String = conn.query_row("SELECT modified_by FROM affiliates", [], |r| r.get(0)).unwrap();
        assert_eq!(mb, SYSTEM);
        log(&conn, None, "test.event", Some("affiliates"), Some("a1"), None).unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM audit_log", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn log_path_drops_control_characters() {
        assert_eq!(log_path("/login\r\nX: y"), "/loginX:y");
    }
}
