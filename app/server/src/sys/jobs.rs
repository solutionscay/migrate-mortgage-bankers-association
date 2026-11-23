//! JOB-02 housekeeping (05 "Jobs", D-50): every 15 minutes, in process. Safe to rerun: each pass only deletes rows
//! that are already dead.
//!
//! * `sessions` past `expires_at` (12 hours after sign-in) or idle longer than `session.idle_minutes` (10).
//!   `auth::session::lookup` already refuses such a session; this pass removes the rows left behind.
//! * Grade-import preview tokens older than 30 minutes: 04 has no table for them yet (OP-ENR-07 arrives in I3 and may
//!   keep previews in memory or add a table with migration 0002). Until then this step is a no-op.

use crate::auth::session::idle_minutes;
use crate::{clock, AppState};
use chrono::{Duration, NaiveDateTime};
use rusqlite::{params, Connection};

/// How often JOB-02 runs.
pub const INTERVAL: std::time::Duration = std::time::Duration::from_secs(15 * 60);
/// OP-ENR-07 preview tokens live 30 minutes (05).
pub const PREVIEW_TOKEN_MINUTES: i64 = 30;

/// What one pass removed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Housekeeping {
    pub sessions: usize,
    pub preview_tokens: usize,
}

/// One JOB-02 pass at time `now` (Puerto Rico civil time; tests pass a fixed clock).
pub fn housekeeping(conn: &Connection, now: NaiveDateTime) -> rusqlite::Result<Housekeeping> {
    let idle = idle_minutes(conn);
    let sessions = conn.execute(
        "DELETE FROM sessions WHERE expires_at <= ?1 OR last_seen_at < ?2",
        params![clock::fmt(now), clock::fmt(now - Duration::minutes(idle))],
    )?;
    // No grade-import preview table in 04 (see the module note). When I3 adds one, delete rows with
    // created_at < now - PREVIEW_TOKEN_MINUTES here.
    let preview_tokens = 0;
    Ok(Housekeeping { sessions, preview_tokens })
}

/// Starts JOB-02 on the tokio runtime: one pass at start, then every [`INTERVAL`]. A failed pass is logged and the
/// next one runs as normal.
pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let st = state.clone();
            match tokio::task::spawn_blocking(move || housekeeping(&st.db(), clock::now())).await {
                Ok(Ok(h)) if h.sessions + h.preview_tokens > 0 => {
                    tracing::info!(sessions = h.sessions, preview_tokens = h.preview_tokens, "JOB-02 housekeeping")
                }
                Ok(Ok(_)) => {}
                Ok(Err(e)) => tracing::error!(error = %e, "JOB-02 housekeeping failed"),
                Err(e) => tracing::error!(error = %e, "JOB-02 task failed"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(conn: &Connection, hash: &str, last_seen: &str, expires: &str) {
        conn.execute(
            "INSERT INTO sessions (token_hash, user_id, csrf_token, created_at, last_seen_at, expires_at)
             VALUES (?1, 1, 'c', '2026-11-23 08:00:00', ?2, ?3)",
            params![hash, last_seen, expires],
        )
        .unwrap();
    }

    /// JOB-02 with a fake clock: expired and idle sessions go, the live one stays; a rerun removes nothing more.
    #[test]
    fn purges_dead_sessions_and_is_safe_to_rerun() {
        let conn = crate::db::open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO users (id, username, display_name, password_hash, role) VALUES (1, 'u', 'U', 'x', 'staff')",
            [],
        )
        .unwrap();
        let now = clock::parse("2026-11-23 12:00:00").unwrap();
        session(&conn, "live", "2026-11-23 11:55:00", "2026-11-23 20:00:00");
        session(&conn, "idle", "2026-11-23 11:49:59", "2026-11-23 20:00:00");
        session(&conn, "expired", "2026-11-23 11:59:00", "2026-11-23 12:00:00");
        assert_eq!(housekeeping(&conn, now).unwrap(), Housekeeping { sessions: 2, preview_tokens: 0 });
        assert_eq!(housekeeping(&conn, now).unwrap(), Housekeeping::default());
        let left: Vec<String> = conn
            .prepare("SELECT token_hash FROM sessions")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(left, ["live"]);
        // 11 minutes later the live one is idle too.
        assert_eq!(housekeeping(&conn, now + Duration::minutes(11)).unwrap().sessions, 1);
    }
}
