//! STU: students (OP-STU-01..05; BR-STU-01..07). The academic summary is `v_student_academic` (BR-STU-03,
//! BR-STU-06); certification progress is `v_student_certification_progress` (BR-CRT-02..04). No SSN column or field
//! exists (D-06).

use super::{count, new_uuid, policy, today, DResult, DomainError, Kind, Who};
use crate::clock;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

/// Most certifications of interest per student (BR-STU-05).
pub const MAX_INTERESTS: i64 = 6;

/// The "Nuevo estudiante" dialog of UI-15: the name and, optionally, the affiliate picked by key (BR-STU-02).
#[derive(Debug, Default, Clone)]
pub struct NewStudent {
    pub nombre: String,
    pub member_uuid: Option<String>,
    pub email: String,
}

/// OP-STU-01: a student and one `student_certifications` row per certification (BR-STU-01), one transaction.
pub fn create_student(tx: &Transaction, who: &Who, s: &NewStudent) -> DResult<String> {
    policy::ap03_domain(who)?;
    let nombre = crate::convention::required("nombre", &s.nombre, 200)?;
    let email = crate::convention::optional("email", &s.email, 200)?;
    let (member, compania) = match s.member_uuid.as_deref().filter(|m| !m.is_empty()) {
        Some(m) => (Some(m.to_owned()), Some(affiliate_name(tx, m)?)),
        None => (None, None),
    };
    let uuid = new_uuid();
    let now = clock::now_text();
    tx.execute(
        "INSERT INTO students (uuid, nombre, email, member_uuid, compania, created_at, created_by, modified_at, modified_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?6, ?7)",
        params![uuid, nombre, email, member, compania, now, who.actor.username],
    )?;
    ensure_certification_rows(tx, who, &uuid)?;
    Ok(uuid)
}

/// BR-STU-01: one `student_certifications` row per certification; existing pairs are skipped (UNIQUE, D-31).
/// Returns the rows added.
pub fn ensure_certification_rows(tx: &Transaction, who: &Who, student: &str) -> rusqlite::Result<usize> {
    let certs: Vec<String> = tx
        .prepare(
            "SELECT c.uuid FROM certifications c
              WHERE NOT EXISTS (SELECT 1 FROM student_certifications sc WHERE sc.student_uuid = ?1 AND sc.certification_uuid = c.uuid)
              ORDER BY c.certification",
        )?
        .query_map([student], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let now = clock::now_text();
    for c in &certs {
        tx.execute(
            "INSERT INTO student_certifications (uuid, student_uuid, certification_uuid, date, created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?5, ?6)",
            params![new_uuid(), student, c, &now[..10], now, who.actor.username],
        )?;
    }
    Ok(certs.len())
}

/// The student columns a duplicate copies (everything of the profile; not the key, the student number or stamps).
const PROFILE_COLS: &str =
    "member_uuid, compania, dir_residencial, dir_trabajo, email, fax, nombre, residencial_2, telefono_r,
  telefono_t, email_work, phone_mobile, company_position, company_department, home_city, home_state, home_zip,
  emergency_contact, emergency_phone, emergency_contact_relationship, work_experience, nmls, notes";

/// OP-STU-02: duplicate a student (profile fields), then BR-STU-01 for the copy. Returns the new uuid.
pub fn duplicate_student(tx: &Transaction, who: &Who, student: &str) -> DResult<String> {
    policy::ap03_domain(who)?;
    exists(tx, student)?;
    let uuid = new_uuid();
    let now = clock::now_text();
    tx.execute(
        &format!(
            "INSERT INTO students (uuid, {PROFILE_COLS}, created_at, created_by, modified_at, modified_by)
             SELECT ?1, {PROFILE_COLS}, ?2, ?3, ?2, ?3 FROM students WHERE uuid = ?4"
        ),
        params![uuid, now, who.actor.username, student],
    )?;
    ensure_certification_rows(tx, who, &uuid)?;
    Ok(uuid)
}

/// BR-STU-02 (changed): link the student to an affiliate picked by key and copy its name into `compania`; `None`
/// unlinks and keeps the typed company name (as BR-AFF-02 does). `registries::patch` calls this for `member_uuid`.
pub fn set_affiliate(tx: &Transaction, who: &Who, student: &str, affiliate: Option<&str>) -> DResult<()> {
    policy::ap03_domain(who)?;
    exists(tx, student)?;
    match affiliate.filter(|a| !a.is_empty()) {
        Some(a) => {
            let name = affiliate_name(tx, a)?;
            tx.execute(
                "UPDATE students SET member_uuid = ?1, compania = ?2 WHERE uuid = ?3",
                params![a, name, student],
            )?;
        }
        None => {
            tx.execute("UPDATE students SET member_uuid = NULL WHERE uuid = ?1", [student])?;
        }
    }
    crate::audit::stamp_modified(tx, "students", "uuid", &student, &who.actor)?;
    Ok(())
}

fn affiliate_name(conn: &Connection, affiliate: &str) -> DResult<String> {
    conn.query_row("SELECT socio FROM affiliates WHERE uuid = ?1", [affiliate], |r| r.get(0))
        .optional()?
        .ok_or_else(|| DomainError::Validation("member_uuid", "Escoja un afiliado de la lista.".into()))
}

/// OP-STU-03 / BR-STU-05: add a certification of interest with today's date; at most 6, no duplicates (AP-06 for
/// the child row). Returns the interest uuid.
pub fn add_interest(tx: &Transaction, who: &Who, student: &str, certification: &str) -> DResult<String> {
    policy::ap06_child_remove(who)?;
    exists(tx, student)?;
    tx.query_row("SELECT 1 FROM certifications WHERE uuid = ?1", [certification], |_| Ok(()))
        .optional()?
        .ok_or_else(|| DomainError::Validation("certification_uuid", "Escoja una certificación de la lista.".into()))?;
    let dup = tx
        .query_row(
            "SELECT 1 FROM student_certification_interests WHERE student_uuid = ?1 AND certification_uuid = ?2",
            [student, certification],
            |_| Ok(()),
        )
        .optional()?;
    if dup.is_some() {
        return Err(DomainError::Validation("certification_uuid", "Ya está en la lista.".into()));
    }
    let used: Vec<i64> = tx
        .prepare("SELECT repetition FROM student_certification_interests WHERE student_uuid = ?1")?
        .query_map([student], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let Some(rep) = (1..=MAX_INTERESTS).find(|r| !used.contains(r)) else {
        return Err(DomainError::LimitReached(MAX_INTERESTS));
    };
    let uuid = new_uuid();
    tx.execute(
        "INSERT INTO student_certification_interests (uuid, student_uuid, repetition, certification_uuid, date) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![uuid, student, rep, certification, today()],
    )?;
    crate::audit::stamp_modified(tx, "students", "uuid", &student, &who.actor)?;
    Ok(uuid)
}

/// OP-STU-03: remove a certification of interest (AP-06).
pub fn remove_interest(tx: &Transaction, who: &Who, student: &str, interest: &str) -> DResult<()> {
    policy::ap06_child_remove(who)?;
    let n = tx.execute(
        "DELETE FROM student_certification_interests WHERE uuid = ?1 AND student_uuid = ?2",
        [interest, student],
    )?;
    if n == 0 {
        return Err(DomainError::NotFound);
    }
    Ok(())
}

/// Enrollments of a student over the three kinds (BR-STU-07 count).
pub fn enrollment_count(conn: &Connection, student: &str) -> rusqlite::Result<i64> {
    let mut n = 0;
    for k in Kind::ALL {
        n += count(conn, &format!("SELECT COUNT(*) FROM {} WHERE student_uuid = ?1", k.enrollment_table()), student)?;
    }
    Ok(n)
}

/// OP-STU-04 / BR-STU-07: ROLE-ADMIN only (AP-04), and only without enrollments (409). Certification rows and
/// interests cascade (04). A student who received a bulk e-mail is kept too (the e-mail rows reference it).
pub fn delete_student(tx: &Transaction, who: &Who, student: &str) -> DResult<()> {
    exists(tx, student)?;
    super::policy::ap04_admin(who)?;
    let n = enrollment_count(tx, student)?;
    if n > 0 {
        return Err(DomainError::HasEnrollments(n));
    }
    let mails = count(tx, "SELECT COUNT(*) FROM mail_emails WHERE student_uuid = ?1", student)?;
    if mails > 0 {
        return Err(DomainError::InUse(mails));
    }
    tx.execute("DELETE FROM students WHERE uuid = ?1", [student])?;
    crate::audit::log(tx, who.actor.user_id, "student.delete", Some("students"), Some(student), None)?;
    Ok(())
}

/// The outcome of the Quick Find (OP-STU-05).
#[derive(Debug, PartialEq, Eq)]
pub enum Found {
    /// 0 hits: the page shows a message.
    None,
    /// 1 hit: open the student.
    One(String),
    /// More: the list (uuids, by name), at most 500.
    Many(Vec<String>),
}

/// OP-STU-05: find students by name, company, e-mail or student number.
pub fn quick_find(conn: &Connection, q: &str) -> rusqlite::Result<Found> {
    let q = q.trim();
    if q.is_empty() {
        return Ok(Found::None);
    }
    let like = format!("%{q}%");
    let hits: Vec<String> = conn
        .prepare(
            "SELECT uuid FROM students
              WHERE nombre LIKE ?1 OR compania LIKE ?1 OR email LIKE ?1 OR email_work LIKE ?1 OR student_id = ?2
              ORDER BY nombre COLLATE NOCASE LIMIT 500",
        )?
        .query_map(params![like, q], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(match hits.len() {
        0 => Found::None,
        1 => Found::One(hits.into_iter().next().unwrap_or_default()),
        _ => Found::Many(hits),
    })
}

/// BR-STU-04: "name (company)" when the company is set.
pub fn display_name(nombre: &str, compania: Option<&str>) -> String {
    match compania.filter(|c| !c.trim().is_empty()) {
        Some(c) => format!("{nombre} ({c})"),
        None => nombre.to_owned(),
    }
}

/// BR-AFF-01: the student is linked to an ACTIVO affiliate (member prices).
pub fn is_affiliate_student(conn: &Connection, student: &str) -> DResult<bool> {
    Ok(conn
        .query_row("SELECT is_affiliate FROM v_student_academic WHERE student_uuid = ?1", [student], |r| {
            r.get::<_, i64>(0)
        })
        .optional()?
        .ok_or(DomainError::NotFound)?
        == 1)
}

/// `v_student_academic` (BR-STU-03, BR-STU-06) plus the name of the last course.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Academic {
    pub is_affiliate: bool,
    pub status: String,
    pub average_points: Option<i64>,
    pub total_credits: f64,
    pub total_hours: f64,
    pub last_course_date: Option<String>,
    pub last_course: Option<String>,
    pub passed_count: i64,
}

pub fn academic(conn: &Connection, student: &str) -> DResult<Academic> {
    let mut a = conn
        .query_row(
            "SELECT is_affiliate, status, average_points, total_credits, total_hours, last_course_date
               FROM v_student_academic WHERE student_uuid = ?1",
            [student],
            |r| {
                Ok(Academic {
                    is_affiliate: r.get::<_, i64>(0)? == 1,
                    status: r.get(1)?,
                    average_points: r.get::<_, Option<f64>>(2)?.map(|v| v as i64),
                    total_credits: r.get::<_, Option<f64>>(3)?.unwrap_or(0.0),
                    total_hours: r.get::<_, Option<f64>>(4)?.unwrap_or(0.0),
                    last_course_date: r.get(5)?,
                    ..Default::default()
                })
            },
        )
        .optional()?
        .ok_or(DomainError::NotFound)?;
    if let Some(d) = &a.last_course_date {
        a.last_course = conn
            .query_row(
                "SELECT course FROM v_enrollments_course_lines WHERE student_uuid = ?1 AND course_date = ?2 LIMIT 1",
                [student, d],
                |r| r.get(0),
            )
            .optional()?;
    }
    a.passed_count = count(
        conn,
        "SELECT COUNT(*) FROM v_enrollments_course_lines WHERE student_uuid = ?1 AND is_passed = 1",
        student,
    )?;
    Ok(a)
}

/// One row of "Progreso por certificación" (UI-15), in the certifications' own order (creation order: COH, CSH, CPP,
/// CCR in the seed, as the design lists them): `v_student_certification_progress` plus the names of the
/// required courses with no passed enrollment ("Cursos que faltan", BR-CRT-02).
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub uuid: String,
    pub certification: String,
    pub description: Option<String>,
    pub total: i64,
    pub completed: i64,
    pub left_count: i64,
    pub average_points: Option<i64>,
    pub total_credits: f64,
    pub total_hours: f64,
    pub graduation_date: Option<String>,
    pub is_graduated: bool,
    pub courses_left: Vec<String>,
}

pub fn certification_progress(conn: &Connection, student: &str) -> rusqlite::Result<Vec<Progress>> {
    let mut rows: Vec<Progress> = conn
        .prepare(
            "SELECT p.uuid, c.certification, c.description, p.total, p.completed, p.left_count, p.average_points,
                    p.total_credits, p.total_hours, p.graduation_date, p.is_graduated, p.certification_uuid
               FROM v_student_certification_progress p JOIN certifications c ON c.uuid = p.certification_uuid
              WHERE p.student_uuid = ?1 ORDER BY c.created_at, c.rowid",
        )?
        .query_map([student], |r| {
            Ok((
                Progress {
                    uuid: r.get(0)?,
                    certification: r.get(1)?,
                    description: r.get(2)?,
                    total: r.get(3)?,
                    completed: r.get(4)?,
                    left_count: r.get(5)?,
                    average_points: r.get::<_, Option<f64>>(6)?.map(|v| v as i64),
                    total_credits: r.get::<_, Option<f64>>(7)?.unwrap_or(0.0),
                    total_hours: r.get::<_, Option<f64>>(8)?.unwrap_or(0.0),
                    graduation_date: r.get(9)?,
                    is_graduated: r.get::<_, i64>(10)? == 1,
                    courses_left: Vec::new(),
                },
                r.get::<_, String>(11)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|(mut p, cert)| {
            p.courses_left = courses_left(conn, student, &cert).unwrap_or_default();
            p
        })
        .collect();
    rows.shrink_to_fit();
    Ok(rows)
}

/// Required courses (type Requisito) of a certification with no passed enrollment of the student.
pub fn courses_left(conn: &Connection, student: &str, certification: &str) -> rusqlite::Result<Vec<String>> {
    conn.prepare(
        "SELECT c.course FROM certification_requirements cr JOIN courses c ON c.uuid = cr.course_uuid
          WHERE cr.certification_uuid = ?2 AND cr.type = 'Requisito'
            AND NOT EXISTS (SELECT 1 FROM enrollments_course e WHERE e.student_uuid = ?1 AND e.course_uuid = cr.course_uuid AND e.puntuacion > 69)
          ORDER BY c.course",
    )?
    .query_map([student, certification], |r| r.get(0))?
    .collect()
}

/// EXT-02 for a student: the home address as a Google Maps link.
#[allow(clippy::type_complexity)]
pub fn maps_url(conn: &Connection, student: &str) -> DResult<Option<String>> {
    let (a1, a2, city, state, zip): (Option<String>, Option<String>, Option<String>, Option<String>, Option<String>) =
        conn.query_row(
            "SELECT dir_residencial, residencial_2, home_city, home_state, home_zip FROM students WHERE uuid = ?1",
            [student],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?
        .ok_or(DomainError::NotFound)?;
    let tail = [state.as_deref(), zip.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(" ");
    Ok(super::affiliates::maps_url(&[a1.as_deref(), a2.as_deref(), city.as_deref(), Some(tail.as_str())]))
}

pub(crate) fn exists(conn: &Connection, student: &str) -> DResult<()> {
    conn.query_row("SELECT 1 FROM students WHERE uuid = ?1", [student], |_| Ok(()))
        .optional()?
        .ok_or(DomainError::NotFound)
}
