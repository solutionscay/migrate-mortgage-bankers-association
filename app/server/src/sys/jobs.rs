//! JOB-02 housekeeping (05 "Jobs", D-50): every 15 minutes, in process. Safe to rerun: each pass only deletes rows
//! that are already dead.
//!
//! * `sessions` past `expires_at` (12 hours after sign-in) or idle longer than `session.idle_minutes` (10).
//!   `auth::session::lookup` already refuses such a session; this pass removes the rows left behind.
//! * Grade-import preview tokens older than 30 minutes (OP-ENR-07): kept in memory by `school::grades`
//!   (no table in 04), dropped here.
//! * `stored_files` rows that no record references and that are older than [`ORPHAN_FILE_HOURS`] (24 hours): an
//!   upload (OP-SYS-04) gets its row first and is attached by the next PATCH or form post, so a fresh row without a
//!   reference is normal; after a day it is a left-over (replaced logo, removed PDF, abandoned upload). The bytes
//!   under `<data dir>/files/` go too when no other row has the same SHA-256 (equal bytes are stored once).
//!   References: every column of 04 that holds a file uuid ([`FILE_REFS`]) and the `app_settings` keys that end in
//!   `file_uuid` (the print logo).

use crate::auth::session::idle_minutes;
use crate::{clock, AppState};
use chrono::{Duration, NaiveDateTime};
use rusqlite::{params, Connection};
use std::path::{Component, Path};

/// How often JOB-02 runs.
pub const INTERVAL: std::time::Duration = std::time::Duration::from_secs(15 * 60);
/// OP-ENR-07 preview tokens live 30 minutes (05).
pub const PREVIEW_TOKEN_MINUTES: i64 = 30;

/// Grace period of an unreferenced `stored_files` row.
pub const ORPHAN_FILE_HOURS: i64 = 24;

/// Every `(table, column)` of 04 that holds a `stored_files` uuid: events PDF, sponsor logo, billing file, mail batch
/// attachment, outbox attachment. The settings logo is an `app_settings` value (see [`referenced_files_sql`]).
pub const FILE_REFS: &[(&str, &str)] = &[
    ("events", "pdf_file_uuid"),
    ("sponsor", "logo_file_uuid"),
    ("billing", "file_uuid"),
    ("mail_batches", "attachment_file_uuid"),
    ("mail_outbox", "attachment_file_uuid"),
];

/// What one pass removed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Housekeeping {
    pub sessions: usize,
    pub preview_tokens: usize,
    /// Orphan `stored_files` rows deleted.
    pub files: usize,
    /// Files removed from disk (no other row had the same bytes).
    pub blobs: usize,
}

/// `SELECT` of every referenced file uuid.
pub fn referenced_files_sql() -> String {
    FILE_REFS
        .iter()
        .map(|(t, c)| format!("SELECT {c} FROM {t} WHERE {c} IS NOT NULL"))
        .chain(std::iter::once(
            "SELECT value FROM app_settings WHERE key LIKE '%file_uuid' AND value IS NOT NULL".to_owned(),
        ))
        .collect::<Vec<_>>()
        .join(" UNION ")
}

/// Deletes the orphan `stored_files` rows older than the grace period (one transaction), then the bytes no row uses
/// any more. Returns (rows, files on disk). A file already missing on disk counts as removed.
pub fn purge_orphan_files(conn: &Connection, data_dir: &Path, now: NaiveDateTime) -> rusqlite::Result<(usize, usize)> {
    let cutoff = clock::fmt(now - Duration::hours(ORPHAN_FILE_HOURS));
    let tx = conn.unchecked_transaction()?;
    let orphans: Vec<(String, String, String)> = tx
        .prepare(&format!(
            "SELECT uuid, sha256, storage_path FROM stored_files
              WHERE created_at < ?1 AND uuid NOT IN ({})",
            referenced_files_sql()
        ))?
        .query_map([&cutoff], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    for (uuid, _, _) in &orphans {
        tx.execute("DELETE FROM stored_files WHERE uuid = ?1", [uuid])?;
    }
    let mut paths: Vec<(String, String)> = Vec::new();
    for (_, sha, path) in &orphans {
        let still_used: i64 =
            tx.query_row("SELECT COUNT(*) FROM stored_files WHERE sha256 = ?1", [sha], |r| r.get(0))?;
        if still_used == 0 && !paths.iter().any(|(s, _)| s == sha) {
            paths.push((sha.clone(), path.clone()));
        }
    }
    tx.commit()?;
    let mut blobs = 0;
    for (_, rel) in paths {
        let p = Path::new(&rel);
        if !p.starts_with("files") || !p.components().all(|c| matches!(c, Component::Normal(_))) {
            tracing::warn!(path = %rel, "JOB-02: stored path outside files/, bytes kept");
            continue;
        }
        match std::fs::remove_file(data_dir.join(p)) {
            Ok(()) => blobs += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => blobs += 1,
            Err(e) => tracing::error!(error = %e, path = %rel, "JOB-02: could not remove file bytes"),
        }
    }
    Ok((orphans.len(), blobs))
}

/// One JOB-02 pass at time `now` (Puerto Rico civil time; tests pass a fixed clock).
pub fn housekeeping(conn: &Connection, data_dir: &Path, now: NaiveDateTime) -> rusqlite::Result<Housekeeping> {
    let idle = idle_minutes(conn);
    let sessions = conn.execute(
        "DELETE FROM sessions WHERE expires_at <= ?1 OR last_seen_at < ?2",
        params![clock::fmt(now), clock::fmt(now - Duration::minutes(idle))],
    )?;
    // Grade-import previews live in memory (no table in 04; `school::grades`): drop those older than 30 minutes.
    let preview_tokens = crate::school::grades::purge_previews(now);
    let (files, blobs) = purge_orphan_files(conn, data_dir, now)?;
    Ok(Housekeeping { sessions, preview_tokens, files, blobs })
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
            match tokio::task::spawn_blocking(move || housekeeping(&st.db(), &st.config.data_dir, clock::now())).await {
                Ok(Ok(h)) if h.sessions + h.preview_tokens + h.files > 0 => tracing::info!(
                    sessions = h.sessions,
                    preview_tokens = h.preview_tokens,
                    files = h.files,
                    blobs = h.blobs,
                    "JOB-02 housekeeping"
                ),
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
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(housekeeping(&conn, dir.path(), now).unwrap(), Housekeeping { sessions: 2, ..Default::default() });
        assert_eq!(housekeeping(&conn, dir.path(), now).unwrap(), Housekeeping::default());
        let left: Vec<String> = conn
            .prepare("SELECT token_hash FROM sessions")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(left, ["live"]);
        // 11 minutes later the live one is idle too.
        assert_eq!(housekeeping(&conn, dir.path(), now + Duration::minutes(11)).unwrap().sessions, 1);
    }

    fn stored(conn: &Connection, dir: &Path, uuid: &str, bytes: &[u8], created: &str) -> std::path::PathBuf {
        let sha = crate::db::sha256_hex(bytes);
        let rel = format!("files/{}/{}", &sha[..2], sha);
        let abs = dir.join(&rel);
        std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
        std::fs::write(&abs, bytes).unwrap();
        conn.execute(
            "INSERT INTO stored_files (uuid, filename, mime_type, size_bytes, sha256, storage_path, created_at)
             VALUES (?1, 'f.pdf', 'application/pdf', ?2, ?3, ?4, ?5)",
            params![uuid, bytes.len() as i64, sha, rel, created],
        )
        .unwrap();
        abs
    }

    /// T-05-22 (extended): JOB-02 with a fixed clock deletes `stored_files` rows that no record references and that
    /// are older than 24 hours, across every file column of 04 and the settings logo; the bytes go only when no other
    /// row has the same SHA-256; a second pass deletes nothing.
    #[test]
    fn t05_22_purges_orphan_files_after_the_grace_period() {
        let conn = crate::db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let now = clock::parse("2026-11-23 12:00:00").unwrap();
        let old = "2026-11-22 11:59:59"; // 24 h and 1 s ago
        let fresh = "2026-11-22 12:00:01";
        // Referenced files: one per column of 04, plus the settings logo. All old.
        for (i, _) in FILE_REFS.iter().enumerate() {
            stored(&conn, d, &format!("ref-{i}"), format!("ref bytes {i}").as_bytes(), old);
        }
        stored(&conn, d, "ref-logo", b"logo bytes", old);
        conn.execute_batch(
            "INSERT INTO events (uuid, name, status, pdf_file_uuid) VALUES ('e1', 'E', 'Activo', 'ref-0');
             INSERT INTO sponsor (uuid, event_uuid, sponsor_name, logo_file_uuid) VALUES ('s1', 'e1', 'S', 'ref-1');
             INSERT INTO billing (uuid, event_uuid, file_uuid) VALUES ('b1', 'e1', 'ref-2');
             UPDATE app_settings SET value = 'ref-logo' WHERE key = 'logo_file_uuid';",
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO mail_batches (uuid, attachment_file_uuid) VALUES ('mb1', 'ref-3');
             INSERT INTO mail_outbox (provider, from_addr, to_addrs, subject, html, attachment_file_uuid, status)
             VALUES ('mock', 'a@ejemplo.com', 'b@ejemplo.com', 'S', '<p>x</p>', 'ref-4', 'sent');",
        )
        .unwrap();

        // Orphans: an old one with its own bytes, an old one sharing bytes with a referenced file, a fresh one.
        let gone = stored(&conn, d, "orphan-old", b"orphan bytes", old);
        let shared = stored(&conn, d, "orphan-shared", b"ref bytes 0", old);
        let kept = stored(&conn, d, "orphan-fresh", b"fresh bytes", fresh);

        let h = housekeeping(&conn, d, now).unwrap();
        assert_eq!((h.files, h.blobs), (2, 1));
        assert!(!gone.exists(), "unique bytes of an old orphan are removed");
        assert!(shared.exists(), "bytes still used by a referenced row stay");
        assert!(kept.exists());
        let left: Vec<String> = conn
            .prepare("SELECT uuid FROM stored_files ORDER BY uuid")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(left, ["orphan-fresh", "ref-0", "ref-1", "ref-2", "ref-3", "ref-4", "ref-logo"]);
        // A second pass at the same time removes nothing; a day later the fresh orphan goes too.
        assert_eq!(housekeeping(&conn, d, now).unwrap(), Housekeeping::default());
        let later = housekeeping(&conn, d, now + Duration::hours(24)).unwrap();
        assert_eq!((later.files, later.blobs), (1, 1));
        assert!(!kept.exists());
    }
}
