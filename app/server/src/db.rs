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
pub const MIGRATIONS: &[(i64, &str, &str)] = &[(1, "0001_schema", include_str!("../migrations/0001_schema.sql"))];

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
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute_batch(&strip_transaction(sql))?;
        tx.execute(
            "INSERT INTO schema_migrations (version, name, sha256, applied_at) VALUES (?1, ?2, ?3, ?4)",
            params![version, name, sum, crate::clock::now_text()],
        )?;
        tx.commit()?;
        applied.push((*name).to_owned());
    }
    Ok(applied)
}

/// Drops the `PRAGMA foreign_keys = ON;`, `BEGIN;` and `COMMIT;` lines (the connection already has foreign keys on,
/// and the runner owns the transaction). Panics unless the file has at most one BEGIN and as many COMMITs: a changed
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
            "PRAGMA foreign_keys = ON;" => false,
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
