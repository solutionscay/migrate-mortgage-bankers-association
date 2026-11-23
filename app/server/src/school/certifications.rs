//! CRT: certifications, their requirements and professors (OP-CRT-01..03, OP-CRS-04; BR-CRT-01..04). These are
//! configuration (AP-08, ROLE-ADMIN; UI-32, D-19), except the graduation date and note of a student's
//! certification (OP-CRT-02, AP-03, autosave `registries::STUDENT_CERT`).

use super::{count, new_uuid, policy, DResult, DomainError, Who};
use crate::clock;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

/// `certification_requirements.type` values (BR-CRT-01).
pub const REQUIREMENT_TYPES: &[&str] = &["Requisito", "Electiva"];

/// OP-CRT-01 create (AP-08). Every existing student gets a row for it, so "one row per student and certification"
/// (BR-STU-01) stays true. Returns the certification uuid.
pub fn create_certification(tx: &Transaction, who: &Who, code: &str, description: &str) -> DResult<String> {
    policy::ap08_config(who)?;
    let code = crate::convention::required("certification", code, 60)?;
    let description = crate::convention::optional("description", description, 200)?;
    let uuid = new_uuid();
    let now = clock::now_text();
    tx.execute(
        "INSERT INTO certifications (uuid, certification, description, created_at, created_by, modified_at, modified_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?4, ?5)",
        params![uuid, code, description, now, who.actor.username],
    )?;
    let students: Vec<String> =
        tx.prepare("SELECT uuid FROM students")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    for st in &students {
        tx.execute(
            "INSERT INTO student_certifications (uuid, student_uuid, certification_uuid, date, created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?5, ?6)",
            params![new_uuid(), st, uuid, &now[..10], now, who.actor.username],
        )?;
    }
    Ok(uuid)
}

/// OP-CRT-01 add a requirement (AP-08): a course of type Requisito or Electiva; one per course (UNIQUE).
pub fn add_requirement(tx: &Transaction, who: &Who, certification: &str, course: &str, kind: &str) -> DResult<String> {
    policy::ap08_config(who)?;
    if !REQUIREMENT_TYPES.contains(&kind) {
        return Err(DomainError::Validation("type", "Escoja Requisito o Electiva.".into()));
    }
    super::offerings::exists(tx, "certifications", certification)?;
    super::offerings::exists(tx, "courses", course)
        .map_err(|_| DomainError::Validation("course_uuid", "Escoja un curso de la lista.".into()))?;
    let dup = tx
        .query_row(
            "SELECT 1 FROM certification_requirements WHERE certification_uuid = ?1 AND course_uuid = ?2",
            [certification, course],
            |_| Ok(()),
        )
        .optional()?;
    if dup.is_some() {
        return Err(DomainError::Validation("course_uuid", "Ese curso ya es requisito.".into()));
    }
    let uuid = new_uuid();
    let now = clock::now_text();
    tx.execute(
        "INSERT INTO certification_requirements (uuid, certification_uuid, course_uuid, type, created_at, created_by, modified_at, modified_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?5, ?6)",
        params![uuid, certification, course, kind, now, who.actor.username],
    )?;
    Ok(uuid)
}

/// OP-CRT-01 remove a requirement (AP-08).
pub fn remove_requirement(tx: &Transaction, who: &Who, requirement: &str) -> DResult<()> {
    policy::ap08_config(who)?;
    if tx.execute("DELETE FROM certification_requirements WHERE uuid = ?1", [requirement])? == 0 {
        return Err(DomainError::NotFound);
    }
    Ok(())
}

/// OP-CRT-01 delete (AP-04, AP-08): ROLE-ADMIN, refused while students have a progress row with a graduation date
/// or a passed required course (409). Untouched progress rows cascade with it.
pub fn delete_certification(tx: &Transaction, who: &Who, certification: &str) -> DResult<()> {
    super::offerings::exists(tx, "certifications", certification)?;
    policy::ap08_config(who)?;
    let used = count(
        tx,
        "SELECT COUNT(*) FROM v_student_certification_progress WHERE certification_uuid = ?1 AND (is_graduated = 1 OR completed > 0)",
        certification,
    )?;
    if used > 0 {
        return Err(DomainError::InUse(used));
    }
    tx.execute("DELETE FROM student_certification_interests WHERE certification_uuid = ?1", [certification])?;
    tx.execute("DELETE FROM student_certifications WHERE certification_uuid = ?1", [certification])?;
    tx.execute("DELETE FROM certifications WHERE uuid = ?1", [certification])?;
    crate::audit::log(
        tx,
        who.actor.user_id,
        "certification.delete",
        Some("certifications"),
        Some(certification),
        None,
    )?;
    Ok(())
}

/// BR-CRT-04: the graduation certificate prints only for a graduated certification (graduation date set).
pub fn certificate_allowed(conn: &Connection, student_certification: &str) -> DResult<bool> {
    Ok(conn
        .query_row(
            "SELECT is_graduated FROM v_student_certification_progress WHERE uuid = ?1",
            [student_certification],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
        .ok_or(DomainError::NotFound)?
        == 1)
}

/// OP-CRS-04 create a professor (AP-08); names are unique.
pub fn create_professor(tx: &Transaction, who: &Who, name: &str) -> DResult<String> {
    policy::ap08_config(who)?;
    let name = crate::convention::required("name", name, 200)?;
    let dup = tx.query_row("SELECT 1 FROM professors WHERE name = ?1", [&name], |_| Ok(())).optional()?;
    if dup.is_some() {
        return Err(DomainError::Validation("name", "Ya existe un profesor con ese nombre.".into()));
    }
    let uuid = new_uuid();
    let now = clock::now_text();
    tx.execute(
        "INSERT INTO professors (uuid, name, created_at, created_by, modified_at, modified_by) VALUES (?1, ?2, ?3, ?4, ?3, ?4)",
        params![uuid, name, now, who.actor.username],
    )?;
    Ok(uuid)
}

/// OP-CRS-04 delete a professor (AP-08): refused while a class date or seminar row names them (409).
pub fn delete_professor(tx: &Transaction, who: &Who, professor: &str) -> DResult<()> {
    super::offerings::exists(tx, "professors", professor)?;
    policy::ap08_config(who)?;
    let used = count(tx, "SELECT COUNT(*) FROM calendar_course_dates WHERE professor_uuid = ?1", professor)?
        + count(tx, "SELECT COUNT(*) FROM calendar_short_course_dates WHERE professor_uuid = ?1", professor)?
        + count(tx, "SELECT COUNT(*) FROM calendar_seminar WHERE professor_uuid = ?1", professor)?;
    if used > 0 {
        return Err(DomainError::InUse(used));
    }
    tx.execute("DELETE FROM professors WHERE uuid = ?1", [professor])?;
    Ok(())
}
