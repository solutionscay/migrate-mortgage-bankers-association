//! The database: one SQLite file, WAL, foreign keys on (04 header). Migrations run at every open.
//!
//! Migration 0001 is `migration/04_database_schema.sql` verbatim (test `migration_0001_is_schema_04`), with the
//! value-list and settings seeds it carries. Later schema changes are new files (0002, ...), never edits of an
//! applied one: the runner stores each file's SHA-256 and refuses a database whose applied file differs.

use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::Path;

/// (version, name, SQL). Each file may carry its own `BEGIN;`/`COMMIT;` so it also runs alone in `sqlite3`; the runner
/// strips those lines and runs the file and its history row in one transaction.
pub const MIGRATIONS: &[(i64, &str, &str)] = &[
    (1, "0001_schema", include_str!("../migrations/0001_schema.sql")),
    (2, "0002_door_no_asistio", include_str!("../migrations/0002_door_no_asistio.sql")),
    (3, "0003_student_status_last_course", include_str!("../migrations/0003_student_status_last_course.sql")),
];

#[derive(Debug)]
pub enum DbError {
    Sqlite(rusqlite::Error),
    /// An applied migration's file changed since it ran: (version, name).
    Divergent(i64, String),
    Pragma(String),
    Check(String),
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DbError::Sqlite(e) => write!(f, "database: {e}"),
            DbError::Divergent(v, n) => write!(f, "migration {v} ({n}) changed after it was applied"),
            DbError::Pragma(p) => write!(f, "database pragma: {p}"),
            DbError::Check(c) => write!(f, "database check: {c}"),
        }
    }
}

impl std::error::Error for DbError {}

impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> Self {
        DbError::Sqlite(e)
    }
}

/// Opens (creates) the file, sets the pragmas and applies pending migrations.
pub fn open(path: &Path) -> Result<Connection, DbError> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| DbError::Pragma(format!("create {}: {e}", dir.display())))?;
    }
    let mut conn = Connection::open(path)?;
    set_pragmas(&conn, true)?;
    migrate(&mut conn)?;
    Ok(conn)
}

/// In-memory database, migrated (unit tests).
pub fn open_in_memory() -> Result<Connection, DbError> {
    let mut conn = Connection::open_in_memory()?;
    set_pragmas(&conn, false)?;
    migrate(&mut conn)?;
    Ok(conn)
}

pub fn set_pragmas(conn: &Connection, file: bool) -> Result<(), DbError> {
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")?;
    let fk: i64 = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
    if fk != 1 {
        return Err(DbError::Pragma("foreign_keys is off".into()));
    }
    if file {
        let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(DbError::Pragma(format!("journal_mode is {mode}, not wal")));
        }
        conn.execute_batch("PRAGMA synchronous = NORMAL;")?;
    }
    Ok(())
}

/// Applies the pending migrations. Returns the names applied by this call.
pub fn migrate(conn: &mut Connection) -> Result<Vec<String>, DbError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
           version INTEGER PRIMARY KEY,
           name TEXT NOT NULL,
           sha256 TEXT NOT NULL,
           applied_at TEXT NOT NULL
         );",
    )?;
    let mut applied = Vec::new();
    for (version, name, sql) in MIGRATIONS {
        let sum = sha256_hex(sql.as_bytes());
        let existing: Option<String> = conn
            .query_row("SELECT sha256 FROM schema_migrations WHERE version = ?1", [version], |r| r.get(0))
            .optional()?;
        match existing {
            Some(s) if s == sum => continue,
            Some(_) => return Err(DbError::Divergent(*version, (*name).to_owned())),
            None => {}
        }
        // Foreign keys off while a migration runs, so a table rebuild (DROP + RENAME) neither cascades nor fails;
        // foreign_key_check must then pass before the commit (ALTER TABLE "other changes" procedure).
        conn.execute_batch("PRAGMA foreign_keys = OFF;")?;
        let res = (|| -> Result<(), DbError> {
            let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute_batch(&strip_transaction(sql))?;
            foreign_key_check(&tx)?;
            tx.execute(
                "INSERT INTO schema_migrations (version, name, sha256, applied_at) VALUES (?1, ?2, ?3, ?4)",
                params![version, name, sum, crate::clock::now_text()],
            )?;
            tx.commit()?;
            Ok(())
        })();
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        res?;
        applied.push((*name).to_owned());
    }
    Ok(applied)
}

/// Drops the `PRAGMA foreign_keys = ON|OFF;`, `PRAGMA foreign_key_check;`, `BEGIN;` and `COMMIT;` lines (the runner
/// sets the pragma, checks the keys and owns the transaction). Panics unless the file has at most one BEGIN and as many COMMITs: a changed
/// file must be looked at, not run half outside a transaction.
pub fn strip_transaction(sql: &str) -> String {
    let (mut begins, mut commits) = (0, 0);
    let body: Vec<&str> = sql
        .lines()
        .filter(|line| match line.trim() {
            "BEGIN;" => {
                begins += 1;
                false
            }
            "COMMIT;" => {
                commits += 1;
                false
            }
            "PRAGMA foreign_keys = ON;" | "PRAGMA foreign_keys = OFF;" | "PRAGMA foreign_key_check;" => false,
            _ => true,
        })
        .collect();
    assert!(begins <= 1 && begins == commits, "expected one BEGIN and one COMMIT, found {begins} and {commits}");
    body.join("\n")
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// `PRAGMA integrity_check` must answer the single row `ok` (T-03-03).
pub fn integrity_check(conn: &Connection) -> Result<(), DbError> {
    let mut stmt = conn.prepare("PRAGMA integrity_check")?;
    let rows: Vec<String> = stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    if rows == ["ok"] {
        Ok(())
    } else {
        Err(DbError::Check(format!("integrity_check: {rows:?}")))
    }
}

/// `PRAGMA foreign_key_check` must return no row (T-03-03).
pub fn foreign_key_check(conn: &Connection) -> Result<(), DbError> {
    let mut stmt = conn.prepare("PRAGMA foreign_key_check")?;
    let rows: Vec<(String, Option<i64>, String)> =
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
    if rows.is_empty() {
        Ok(())
    } else {
        Err(DbError::Check(format!("foreign_key_check: {} rows, first {:?}", rows.len(), rows.first())))
    }
}

/// A setting of `app_settings`, or `None` when the key is absent or NULL.
pub fn setting(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM app_settings WHERE key = ?1", [key], |r| r.get::<_, Option<String>>(0))
        .optional()?
        .flatten())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_0001_is_schema_04() {
        let schema_04 = include_str!("../../../migration/04_database_schema.sql");
        assert_eq!(MIGRATIONS[0].2, schema_04, "0001 must equal 04_database_schema.sql byte for byte");
    }

    #[test]
    fn migrate_applies_once_and_seeds_reference_values() {
        let mut conn = open_in_memory().unwrap();
        assert!(migrate(&mut conn).unwrap().is_empty(), "second run applies nothing");
        let lists: i64 = conn.query_row("SELECT COUNT(DISTINCT list) FROM value_list_items", [], |r| r.get(0)).unwrap();
        assert!(lists >= 15, "value lists seeded: {lists}");
        assert_eq!(setting(&conn, "session.idle_minutes").unwrap().as_deref(), Some("10"));
        integrity_check(&conn).unwrap();
        foreign_key_check(&conn).unwrap();
    }

    /// 0002 rebuilds registration and mail_outbox: rows, children (no cascade on DROP) and keys survive;
    /// "No Asistió" and 'mocked' pass the new CHECKs.
    #[test]
    fn migration_0002_keeps_data_and_children() {
        let mut conn = Connection::open_in_memory().unwrap();
        set_pragmas(&conn, false).unwrap();
        conn.execute_batch("CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, sha256 TEXT NOT NULL, applied_at TEXT NOT NULL);").unwrap();
        conn.execute_batch(&strip_transaction(MIGRATIONS[0].2)).unwrap();
        conn.execute(
            "INSERT INTO schema_migrations VALUES (1, '0001_schema', ?1, 'x')",
            [sha256_hex(MIGRATIONS[0].2.as_bytes())],
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO people (uuid) VALUES ('p');
             INSERT INTO events (uuid, name) VALUES ('e', 'E');
             INSERT INTO event_items (uuid, event_uuid, name, type) VALUES ('i', 'e', 'I', 'Foro');
             INSERT INTO registration (uuid, person_uuid, event_uuid, registration_number, status)
               VALUES ('r', 'p', 'e', 1000, 'No Canceló');
             INSERT INTO registration_items (uuid, registration_uuid, event_item_uuid) VALUES ('ri', 'r', 'i');
             INSERT INTO payments (uuid, registration_uuid, payment) VALUES ('pay', 'r', 100);",
        )
        .unwrap();
        assert_eq!(migrate(&mut conn).unwrap(), ["0002_door_no_asistio", "0003_student_status_last_course"]);
        let n = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!(n("SELECT COUNT(*) FROM registration_items WHERE registration_uuid = 'r'"), 1);
        assert_eq!(n("SELECT COUNT(*) FROM payments WHERE registration_uuid = 'r'"), 1);
        assert_eq!(n("SELECT COUNT(*) FROM v_registration_totals"), 1);
        assert_eq!(n("PRAGMA foreign_keys"), 1);
        conn.execute("UPDATE registration SET status = 'No Asistió' WHERE uuid = 'r'", []).unwrap();
        assert!(conn.execute("UPDATE registration SET status = 'x' WHERE uuid = 'r'", []).is_err());
        assert!(conn.execute("DELETE FROM registration WHERE uuid = 'r'", []).is_err(), "payments FK still holds");
        conn.execute(
            "INSERT INTO mail_outbox (provider, from_addr, to_addrs, subject, html, status)
             VALUES ('mock', 'a@ejemplo.com', 'b@ejemplo.com', 's', 'h', 'mocked')",
            [],
        )
        .unwrap();
        integrity_check(&conn).unwrap();
        foreign_key_check(&conn).unwrap();
    }

    /// 0003 recreates `v_student_academic` only (D-95, D-96): every row stays; a BAJA enrollment no longer makes a
    /// student ACTIVO; the last course date skips Pendiente and Cancelado rows.
    #[test]
    fn migration_0003_keeps_data_and_changes_the_view() {
        let mut conn = Connection::open_in_memory().unwrap();
        set_pragmas(&conn, false).unwrap();
        conn.execute_batch("CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, sha256 TEXT NOT NULL, applied_at TEXT NOT NULL);").unwrap();
        for (v, name, sql) in &MIGRATIONS[..2] {
            conn.execute_batch("PRAGMA foreign_keys = OFF;").unwrap();
            conn.execute_batch(&strip_transaction(sql)).unwrap();
            conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
            conn.execute(
                "INSERT INTO schema_migrations VALUES (?1, ?2, ?3, 'x')",
                rusqlite::params![v, name, sha256_hex(sql.as_bytes())],
            )
            .unwrap();
        }
        conn.execute_batch(
            "INSERT INTO students (uuid, nombre) VALUES ('baja', 'Solo BAJA'), ('ok', 'Activo');
             INSERT INTO courses (uuid, course) VALUES ('c', 'Curso');
             INSERT INTO calendar_course (uuid, course_uuid, status) VALUES ('conf', 'c', 'Confirmado'), ('pend', 'c', 'Pendiente');
             INSERT INTO calendar_course_dates (uuid, calendar_course_uuid, date) VALUES ('d1', 'conf', '2026-09-14'), ('d2', 'pend', '2027-01-25');
             INSERT INTO enrollments_course (uuid, serial, student_uuid, course_uuid, course_calendar_uuid, status) VALUES
               ('e1', 1, 'baja', 'c', 'conf', 'BAJA'),
               ('e2', 2, 'ok', 'c', 'conf', 'ACTIVO'),
               ('e3', 3, 'ok', 'c', 'pend', 'ACTIVO');",
        )
        .unwrap();
        let status = |c: &Connection, s: &str| -> (String, Option<String>) {
            c.query_row("SELECT status, last_course_date FROM v_student_academic WHERE student_uuid = ?1", [s], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap()
        };
        assert_eq!(status(&conn, "baja").0, "ACTIVO", "before 0003 BAJA counted");
        assert_eq!(status(&conn, "ok").1.as_deref(), Some("2027-01-25"), "before 0003 Pendiente counted");
        let rows = |c: &Connection| -> i64 {
            c.query_row(
                "SELECT (SELECT COUNT(*) FROM students) + (SELECT COUNT(*) FROM enrollments_course) + (SELECT COUNT(*) FROM calendar_course)",
                [],
                |r| r.get(0),
            )
            .unwrap()
        };
        let before = rows(&conn);
        assert_eq!(migrate(&mut conn).unwrap(), ["0003_student_status_last_course"]);
        assert_eq!(rows(&conn), before, "no data changes");
        assert_eq!(status(&conn, "baja"), ("INACTIVO".to_owned(), Some("2026-09-14".to_owned())));
        assert_eq!(status(&conn, "ok"), ("ACTIVO".to_owned(), Some("2026-09-14".to_owned())));
        integrity_check(&conn).unwrap();
        foreign_key_check(&conn).unwrap();
    }

    #[test]
    fn changed_migration_is_refused() {
        let mut conn = open_in_memory().unwrap();
        conn.execute("UPDATE schema_migrations SET sha256 = 'x' WHERE version = 1", []).unwrap();
        assert!(matches!(migrate(&mut conn), Err(DbError::Divergent(1, _))));
    }

    #[test]
    fn file_database_is_wal() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(&dir.path().join("t.db")).unwrap();
        let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(mode, "wal");
    }

    /// T-08-21: no SSN column anywhere (D-06).
    #[test]
    fn no_ssn_column() {
        let conn = open_in_memory().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT m.name, p.name FROM sqlite_master m JOIN pragma_table_info(m.name) p
                 WHERE m.type = 'table' AND (lower(p.name) LIKE '%ssn%' OR lower(p.name) LIKE '%social%'
                       OR lower(p.name) IN ('s_s', 'ss'))",
            )
            .unwrap();
        let hits: Vec<(String, String)> =
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap();
        assert!(hits.is_empty(), "{hits:?}");
    }
}
