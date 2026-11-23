//! Server sessions (D-50, AP-01). The cookie holds a random token; `sessions` holds its SHA-256, the CSRF token,
//! `last_seen_at` and `expires_at`. A session ends after `app_settings.session.idle_minutes` (10) without a request
//! (BR-AUTH-02) or at `expires_at` (12 hours after sign-in), whichever comes first. JOB-02 (phase 2, `sys`) deletes
//! the rows left behind; the check here does not depend on it.

use super::{random_token, CurrentUser, Role};
use crate::clock;
use crate::db::{setting, sha256_hex};
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension};

/// Absolute session length.
pub const MAX_HOURS: i64 = 12;
/// Idle timeout when the setting is missing or unreadable (D-40).
pub const DEFAULT_IDLE_MINUTES: i64 = 10;

pub fn idle_minutes(conn: &Connection) -> i64 {
    setting(conn, "session.idle_minutes")
        .ok()
        .flatten()
        .and_then(|v| v.parse().ok())
        .filter(|m| *m > 0)
        .unwrap_or(DEFAULT_IDLE_MINUTES)
}

/// The user of a session token, or `None` (unknown, expired, idle, or user disabled). An ended session's row is
/// deleted. A live one gets `last_seen_at = now`.
pub fn lookup(state: &AppState, token: &str) -> rusqlite::Result<Option<CurrentUser>> {
    let token_hash = sha256_hex(token.as_bytes());
    let conn = state.db();
    let row = conn
        .query_row(
            "SELECT s.user_id, s.csrf_token, s.last_seen_at, s.expires_at, u.username, u.display_name, u.role, u.active
               FROM sessions s JOIN users u ON u.id = s.user_id
              WHERE s.token_hash = ?1",
            [&token_hash],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, i64>(7)?,
                ))
            },
        )
        .optional()?;
    let Some((user_id, csrf, last_seen, expires, username, display_name, role, active)) = row else {
        return Ok(None);
    };
    let now = clock::now();
    let idle = chrono::Duration::minutes(idle_minutes(&conn));
    let live = active == 1
        && clock::parse(&expires).is_some_and(|e| e > now)
        && clock::parse(&last_seen).is_some_and(|l| now - l <= idle);
    let role = Role::from_db(&role);
    match (live, role) {
        (true, Some(role)) => {
            conn.execute(
                "UPDATE sessions SET last_seen_at = ?1 WHERE token_hash = ?2",
                params![clock::fmt(now), token_hash],
            )?;
            Ok(Some(CurrentUser { id: user_id, username, display_name, role, csrf, token_hash }))
        }
        _ => {
            conn.execute("DELETE FROM sessions WHERE token_hash = ?1", [&token_hash])?;
            Ok(None)
        }
    }
}

/// Creates a session for `user_id`. Returns the cookie token (only its hash is stored).
pub fn create(conn: &Connection, user_id: i64, user_agent: Option<&str>) -> rusqlite::Result<String> {
    let token = random_token();
    let now = clock::now_text();
    conn.execute(
        "INSERT INTO sessions (token_hash, user_id, csrf_token, created_at, last_seen_at, expires_at, ip, user_agent)
         VALUES (?1, ?2, ?3, ?4, ?4, ?5, NULL, ?6)",
        params![
            sha256_hex(token.as_bytes()),
            user_id,
            random_token(),
            now,
            clock::in_minutes(MAX_HOURS * 60),
            user_agent.map(|ua| ua.chars().take(200).collect::<String>()),
        ],
    )?;
    Ok(token)
}

/// Deletes sessions past `expires_at` or idle too long. Returns how many (JOB-02 calls this from phase 2).
pub fn purge_expired(conn: &Connection) -> rusqlite::Result<usize> {
    let idle = idle_minutes(conn);
    conn.execute(
        "DELETE FROM sessions WHERE expires_at <= ?1 OR last_seen_at < ?2",
        params![clock::now_text(), clock::in_minutes(-idle)],
    )
}
