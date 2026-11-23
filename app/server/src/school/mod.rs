//! I3 school domain layer (MBS): affiliates, students, offerings (courses, seminars, short courses) with their
//! calendar rows and class dates, enrollments of the three kinds with prices by affiliate status, grades and the
//! grades XLSX round trip, certifications and professors.
//!
//! Same pattern as `crate::convention`: every OP-* is a plain function over a rusqlite `Transaction` the caller
//! opens with [`begin`]; BR-* invariants are enforced here; money is integer cents; academic summaries, progress,
//! counts and revenue come only from the 04 views (`v_enrollments_course_lines`, `v_calendar_*_summary`,
//! `v_student_academic`, `v_student_certification_progress`). Errors are the shared `DomainError`.
//!
//! Policies: AP-07 (owned here) and AP-08 for configuration in [`policy`]; AP-03..AP-06 and AP-09 reuse
//! `convention::policy`. Field edits go through [`registries::patch`]. Routes are in [`routes`] (phase 2).

pub mod affiliates;
pub mod certifications;
pub mod enrollments;
pub mod grades;
pub mod offerings;
pub mod policy;
pub mod registries;
pub mod routes;
pub mod students;

pub use crate::convention::{begin, new_uuid, DResult, DomainError, Who};
pub use offerings::Kind;

use rusqlite::Connection;

/// `COUNT(*)` with one text argument.
pub(crate) fn count(conn: &Connection, sql: &str, arg: &str) -> rusqlite::Result<i64> {
    conn.query_row(sql, [arg], |r| r.get(0))
}

/// Next value of an auto-serial column, read inside the insert's transaction.
pub(crate) fn next_serial(conn: &Connection, table: &str, col: &str, floor: i64) -> rusqlite::Result<i64> {
    conn.query_row(&format!("SELECT MAX(COALESCE(MAX({col}), ?1), ?1) + 1 FROM {table}"), [floor], |r| r.get(0))
}

/// Today's date `YYYY-MM-DD` (Puerto Rico).
pub(crate) fn today() -> String {
    crate::clock::now_text()[..10].to_owned()
}

/// BR-ENR-04: letter grade of a score (same scale as `v_enrollments_course_lines.nota`).
pub fn letter(score: Option<i64>) -> Option<&'static str> {
    score.map(|s| match s {
        90.. => "A",
        80..=89 => "B",
        70..=79 => "C",
        60..=69 => "D",
        _ => "F",
    })
}

/// BR-ENR-05: passed when the score is more than 69.
pub fn passed(score: Option<i64>) -> bool {
    score.is_some_and(|s| s > 69)
}
