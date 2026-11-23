//! ENR: enrollments of the three kinds (OP-ENR-01..05, OP-ENR-08; BR-ENR-01..05, -08, -09). The price is stored
//! once, when the calendar row is picked, from the student's affiliate status at that moment (BR-ENR-01, D-14).
//! Balance, letter and passed come from `v_enrollments_course_lines` (BR-ENR-02, -04, -05).

use super::{count, new_uuid, next_serial, policy, DResult, DomainError, Kind, Who};
use crate::clock;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

/// Course enrollment statuses (no value list in the source; OD-04-02): new = ACTIVO (BR-ENR-09), cancel = BAJA.
pub const COURSE_STATUSES: &[&str] = &["ACTIVO", "BAJA"];
/// Seminar and short-course statuses (`irregular_course_statuses`).
pub const IRREGULAR_STATUSES: &[&str] = &["Asistió", "Canceló", "No Canceló"];
/// Payer values (`course_enrollment_paid_by`).
pub const PAYERS: &[&str] = &["COMPAÑIA", "INDIVIDUAL"];

const FIRST_COURSE_SERIAL: i64 = 10_000;
const FIRST_ID_NUM: i64 = 1_000;
const FIRST_INVOICE: i64 = 26_000;

/// What OP-ENR-01..03 stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enrolled {
    pub uuid: String,
    /// Cents (BR-ENR-01).
    pub price: Option<i64>,
    pub member_price: bool,
    pub serial: i64,
}

/// OP-ENR-01..03: enroll a student in a calendar row of the given kind. The row must be active (BR-CRS-01); the
/// price is the row's member price for an affiliate student, else its individual price (BR-ENR-01), read in this
/// transaction; the payer defaults to COMPAÑIA for an affiliate student, else INDIVIDUAL. A course enrollment starts
/// ACTIVO (BR-ENR-09).
pub fn enroll(tx: &Transaction, who: &Who, kind: Kind, student: &str, calendar: &str) -> DResult<Enrolled> {
    policy::ap03_domain(who)?;
    super::students::exists(tx, student)?;
    let (offering, status, member_price, individual_price): (String, String, Option<i64>, Option<i64>) = tx
        .query_row(
            &format!(
                "SELECT {}, status, cost_member, cost_individual FROM {} WHERE uuid = ?1",
                kind.calendar_offering_col(),
                kind.calendar_table()
            ),
            [calendar],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?
        .ok_or(DomainError::NotFound)?;
    if !super::offerings::is_active_status(&status) {
        return Err(DomainError::CalendarNotActive);
    }
    let member = super::students::is_affiliate_student(tx, student)?;
    let price = if member { member_price } else { individual_price };
    let payer = if member { "COMPAÑIA" } else { "INDIVIDUAL" };
    let uuid = new_uuid();
    let now = clock::now_text();
    let w = &who.actor.username;
    let serial = match kind {
        Kind::Course => {
            let serial = next_serial(tx, "enrollments_course", "serial", FIRST_COURSE_SERIAL)?;
            let id_num = next_serial(tx, "enrollments_course", "id_num", FIRST_ID_NUM)?;
            let invoice = next_serial(tx, "enrollments_course", "invoice_num", FIRST_INVOICE)?;
            tx.execute(
                "INSERT INTO enrollments_course (uuid, serial, student_uuid, course_uuid, course_calendar_uuid, costeado_por, costo,
                   pago, id_num, invoice_num, status, created_at, created_by, modified_at, modified_by)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9, 'ACTIVO', ?10, ?11, ?10, ?11)",
                params![uuid, serial, student, offering, calendar, payer, price, id_num, invoice, now, w],
            )?;
            serial
        }
        Kind::Seminar | Kind::ShortCourse => {
            let t = kind.enrollment_table();
            let serial = next_serial(tx, t, "serial", 0)?;
            let invoice = next_serial(tx, t, "invoice_number", FIRST_INVOICE)?;
            tx.execute(
                &format!(
                    "INSERT INTO {t} (uuid, serial, student_uuid, {}, {}, cargo, costeado_por, pago, invoice_number,
                       created_at, created_by, modified_at, modified_by)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9, ?10, ?9, ?10)",
                    kind.enrollment_offering_col(),
                    kind.enrollment_calendar_col()
                ),
                params![uuid, serial, student, offering, calendar, price, payer, invoice, now, w],
            )?;
            serial
        }
    };
    Ok(Enrolled { uuid, price, member_price: member, serial })
}

/// The payment of an enrollment (cents).
pub fn pago(conn: &Connection, kind: Kind, enrollment: &str) -> DResult<i64> {
    conn.query_row(&format!("SELECT pago FROM {} WHERE uuid = ?1", kind.enrollment_table()), [enrollment], |r| r.get(0))
        .optional()?
        .ok_or(DomainError::NotFound)
}

/// OP-ENR-05 cancel: a course enrollment goes to BAJA, a seminar or short-course one to Canceló (both roles; the way
/// out when delete is refused).
pub fn cancel_enrollment(tx: &Transaction, who: &Who, kind: Kind, enrollment: &str) -> DResult<()> {
    policy::ap03_domain(who)?;
    pago(tx, kind, enrollment)?;
    let status = if kind == Kind::Course { "BAJA" } else { "Canceló" };
    tx.execute(&format!("UPDATE {} SET status = ?1 WHERE uuid = ?2", kind.enrollment_table()), [status, enrollment])?;
    crate::audit::stamp_modified(tx, kind.enrollment_table(), "uuid", &enrollment, &who.actor)?;
    crate::audit::log(
        tx,
        who.actor.user_id,
        "enrollment.cancel",
        Some(kind.enrollment_table()),
        Some(enrollment),
        None,
    )?;
    Ok(())
}

/// OP-ENR-05 / BR-ENR-08 delete: two-step confirm; an enrollment with a payment is refused (409, cancel instead);
/// without a payment, ROLE-ADMIN deletes it (AP-05).
pub fn delete_enrollment(tx: &Transaction, who: &Who, kind: Kind, enrollment: &str, confirm: bool) -> DResult<()> {
    let paid = pago(tx, kind, enrollment)?;
    policy::ap05_enrollment_delete(who, paid)?;
    if !confirm {
        return Err(DomainError::ConfirmRequired);
    }
    tx.execute(&format!("DELETE FROM {} WHERE uuid = ?1", kind.enrollment_table()), [enrollment])?;
    crate::audit::log(
        tx,
        who.actor.user_id,
        "enrollment.delete",
        Some(kind.enrollment_table()),
        Some(enrollment),
        None,
    )?;
    Ok(())
}

/// One course enrollment as `v_enrollments_course_lines` shows it (UI-15 Cursos tab, UI-21).
#[derive(Debug, Clone, PartialEq)]
pub struct CourseLine {
    pub uuid: String,
    pub serial: Option<i64>,
    pub course: String,
    pub course_date: Option<String>,
    pub course_status: Option<String>,
    pub status: String,
    pub costo: Option<i64>,
    pub pago: i64,
    pub balance: i64,
    pub puntuacion: Option<i64>,
    pub nota: Option<String>,
    pub is_passed: bool,
}

pub fn course_lines(conn: &Connection, student: &str) -> rusqlite::Result<Vec<CourseLine>> {
    conn.prepare(
        "SELECT uuid, serial, course, course_date, course_status, status, costo, pago, balance, puntuacion, nota, is_passed
           FROM v_enrollments_course_lines WHERE student_uuid = ?1 ORDER BY course_date DESC, serial DESC",
    )?
    .query_map([student], |r| {
        Ok(CourseLine {
            uuid: r.get(0)?,
            serial: r.get(1)?,
            course: r.get(2)?,
            course_date: r.get(3)?,
            course_status: r.get(4)?,
            status: r.get(5)?,
            costo: r.get(6)?,
            pago: r.get(7)?,
            balance: r.get(8)?,
            puntuacion: r.get(9)?,
            nota: r.get(10)?,
            is_passed: r.get::<_, i64>(11)? == 1,
        })
    })?
    .collect()
}

/// Count of enrollments in a calendar row (for the confirm texts of the screens).
pub fn calendar_count(conn: &Connection, kind: Kind, calendar: &str) -> rusqlite::Result<i64> {
    count(
        conn,
        &format!("SELECT COUNT(*) FROM {} WHERE {} = ?1", kind.enrollment_table(), kind.enrollment_calendar_col()),
        calendar,
    )
}
