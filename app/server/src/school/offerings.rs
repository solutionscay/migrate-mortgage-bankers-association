//! CRS: offerings of three kinds (courses, seminars, short courses), their calendar rows and class dates
//! (OP-CRS-01..03, OP-CRS-05; BR-CRS-01..06). Summaries come from `v_calendar_*_summary` (BR-CRS-03).

use super::{count, new_uuid, next_serial, policy, DResult, DomainError, Who};
use crate::autosave;
use crate::clock;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

/// The `{kind}` of the school routes (05): `course` | `seminar` | `short-course`. Each maps to three table
/// families of 04. An unknown kind is 404.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Course,
    Seminar,
    ShortCourse,
}

/// Calendar statuses (`course_statuses`); active = Pendiente or Confirmado (BR-CRS-01).
pub const CALENDAR_STATUSES: &[&str] = &["Pendiente", "Confirmado", "Completado", "Cancelado"];
/// Class day types (`course_calendar_day_type`, BR-CRS-06).
pub const DAY_TYPES: &[&str] = &["Día de Clase", "Día de Fiesta"];
/// Short-course class dates: up to 15 (repeating field of the source, D-26).
pub const MAX_SHORT_DATES: i64 = 15;

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Course, Kind::Seminar, Kind::ShortCourse];

    pub fn from_slug(s: &str) -> Option<Kind> {
        match s {
            "course" => Some(Kind::Course),
            "seminar" => Some(Kind::Seminar),
            "short-course" => Some(Kind::ShortCourse),
            _ => None,
        }
    }
    pub fn slug(self) -> &'static str {
        match self {
            Kind::Course => "course",
            Kind::Seminar => "seminar",
            Kind::ShortCourse => "short-course",
        }
    }
    /// Spanish plural for tabs: "Cursos especializados", "Seminarios", "Cursos cortos".
    pub fn label(self) -> &'static str {
        match self {
            Kind::Course => "Cursos especializados",
            Kind::Seminar => "Seminarios",
            Kind::ShortCourse => "Cursos cortos",
        }
    }
    pub fn offering_table(self) -> &'static str {
        match self {
            Kind::Course => "courses",
            Kind::Seminar => "seminars",
            Kind::ShortCourse => "short_courses",
        }
    }
    pub fn name_col(self) -> &'static str {
        match self {
            Kind::Course => "course",
            Kind::Seminar => "seminar",
            Kind::ShortCourse => "short_course",
        }
    }
    /// Offering price columns: (member, individual).
    pub fn offering_prices(self) -> (&'static str, &'static str) {
        match self {
            Kind::Course => ("cost_member", "cost_individual"),
            _ => ("cost_affiliate", "cost_individual"),
        }
    }
    pub fn calendar_table(self) -> &'static str {
        match self {
            Kind::Course => "calendar_course",
            Kind::Seminar => "calendar_seminar",
            Kind::ShortCourse => "calendar_short_course",
        }
    }
    /// The calendar's foreign key to its offering.
    pub fn calendar_offering_col(self) -> &'static str {
        match self {
            Kind::Course => "course_uuid",
            Kind::Seminar => "seminar_uuid",
            Kind::ShortCourse => "short_course_uuid",
        }
    }
    pub fn enrollment_table(self) -> &'static str {
        match self {
            Kind::Course => "enrollments_course",
            Kind::Seminar => "enrollments_seminars",
            Kind::ShortCourse => "enrollments_short_course",
        }
    }
    /// The enrollment's foreign key to the offering.
    pub fn enrollment_offering_col(self) -> &'static str {
        self.calendar_offering_col()
    }
    /// The enrollment's foreign key to the calendar row.
    pub fn enrollment_calendar_col(self) -> &'static str {
        match self {
            Kind::Course => "course_calendar_uuid",
            Kind::Seminar => "seminar_calendar_uuid",
            Kind::ShortCourse => "short_course_calendar_uuid",
        }
    }
    /// The stored price column of an enrollment (BR-ENR-01).
    pub fn enrollment_price_col(self) -> &'static str {
        match self {
            Kind::Course => "costo",
            _ => "cargo",
        }
    }
    pub fn summary_view(self) -> (&'static str, &'static str) {
        match self {
            Kind::Course => ("v_calendar_course_summary", "calendar_course_uuid"),
            Kind::Seminar => ("v_calendar_seminar_summary", "calendar_seminar_uuid"),
            Kind::ShortCourse => ("v_calendar_short_course_summary", "calendar_short_course_uuid"),
        }
    }
    /// SQL of the first class date of calendar row `?1`.
    fn first_date_sql(self) -> &'static str {
        match self {
            Kind::Course => "SELECT MIN(date) FROM calendar_course_dates WHERE calendar_course_uuid = ?1",
            Kind::Seminar => "SELECT date FROM calendar_seminar WHERE uuid = ?1",
            Kind::ShortCourse => {
                "SELECT MIN(date) FROM calendar_short_course_dates WHERE calendar_short_course_uuid = ?1"
            }
        }
    }
}

/// The create form of an offering (UI-18 "Nuevo curso").
#[derive(Debug, Default, Clone)]
pub struct NewOffering {
    pub name: String,
    /// Dollar text.
    pub member_price: String,
    pub individual_price: String,
}

fn money_opt(field: &'static str, s: &str) -> DResult<Option<i64>> {
    if s.trim().is_empty() {
        Ok(None)
    } else {
        autosave::cents(s).map(Some).map_err(|m| DomainError::Validation(field, m))
    }
}

/// OP-CRS-01 create (AP-03).
pub fn create_offering(tx: &Transaction, who: &Who, kind: Kind, o: &NewOffering) -> DResult<String> {
    policy::ap03_domain(who)?;
    let name = crate::convention::required("name", &o.name, 200)?;
    let (m, i) = kind.offering_prices();
    let uuid = new_uuid();
    let now = clock::now_text();
    tx.execute(
        &format!(
            "INSERT INTO {} (uuid, {}, {m}, {i}, created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?5, ?6)",
            kind.offering_table(),
            kind.name_col()
        ),
        params![
            uuid,
            name,
            money_opt("member_price", &o.member_price)?,
            money_opt("individual_price", &o.individual_price)?,
            now,
            who.actor.username
        ],
    )?;
    Ok(uuid)
}

/// OP-CRS-01 delete: ROLE-ADMIN (AP-04); refused while calendar rows or enrollments exist (409).
pub fn delete_offering(tx: &Transaction, who: &Who, kind: Kind, uuid: &str) -> DResult<()> {
    exists(tx, kind.offering_table(), uuid)?;
    super::policy::ap04_admin(who)?;
    let cal = count(
        tx,
        &format!("SELECT COUNT(*) FROM {} WHERE {} = ?1", kind.calendar_table(), kind.calendar_offering_col()),
        uuid,
    )?;
    if cal > 0 {
        return Err(DomainError::HasCalendar(cal));
    }
    let enr = count(
        tx,
        &format!("SELECT COUNT(*) FROM {} WHERE {} = ?1", kind.enrollment_table(), kind.enrollment_offering_col()),
        uuid,
    )?;
    if enr > 0 {
        return Err(DomainError::HasEnrollments(enr));
    }
    if kind == Kind::Course {
        let req = count(tx, "SELECT COUNT(*) FROM certification_requirements WHERE course_uuid = ?1", uuid)?;
        if req > 0 {
            return Err(DomainError::InUse(req));
        }
    }
    tx.execute(&format!("DELETE FROM {} WHERE uuid = ?1", kind.offering_table()), [uuid])?;
    crate::audit::log(tx, who.actor.user_id, "offering.delete", Some(kind.offering_table()), Some(uuid), None)?;
    Ok(())
}

/// The "Abrir curso" form: optional semester (course) or date (seminar).
#[derive(Debug, Default, Clone)]
pub struct NewCalendar {
    pub semester: String,
    /// Seminar date `YYYY-MM-DD` (the seminar's one date lives on its calendar row).
    pub date: String,
}

/// OP-CRS-02 / BR-CRS-02: a new calendar row, status Pendiente, prices copied from the offering once. Short
/// courses also copy their hours. Returns the calendar uuid.
pub fn open_calendar(tx: &Transaction, who: &Who, kind: Kind, offering: &str, c: &NewCalendar) -> DResult<String> {
    policy::ap03_domain(who)?;
    let (m, i) = kind.offering_prices();
    let (member, individual): (Option<i64>, Option<i64>) = tx
        .query_row(&format!("SELECT {m}, {i} FROM {} WHERE uuid = ?1", kind.offering_table()), [offering], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?
        .ok_or(DomainError::NotFound)?;
    let uuid = new_uuid();
    let serial = next_serial(tx, kind.calendar_table(), "serial", 0)?;
    let now = clock::now_text();
    let who_name = &who.actor.username;
    match kind {
        Kind::Course => {
            tx.execute(
                "INSERT INTO calendar_course (uuid, serial, course_uuid, status, semester, cost_member, cost_individual,
                   created_at, created_by, modified_at, modified_by)
                 VALUES (?1, ?2, ?3, 'Pendiente', ?4, ?5, ?6, ?7, ?8, ?7, ?8)",
                params![
                    uuid,
                    serial,
                    offering,
                    crate::convention::optional("semester", &c.semester, 60)?,
                    member,
                    individual,
                    now,
                    who_name
                ],
            )?;
        }
        Kind::Seminar => {
            let date =
                if c.date.trim().is_empty() { None } else { Some(crate::convention::iso_date("date", &c.date)?) };
            tx.execute(
                "INSERT INTO calendar_seminar (uuid, serial, seminar_uuid, status, date, cost_member, cost_individual,
                   created_at, created_by, modified_at, modified_by)
                 VALUES (?1, ?2, ?3, 'Pendiente', ?4, ?5, ?6, ?7, ?8, ?7, ?8)",
                params![uuid, serial, offering, date, member, individual, now, who_name],
            )?;
        }
        Kind::ShortCourse => {
            tx.execute(
                "INSERT INTO calendar_short_course (uuid, serial, short_course_uuid, status, cost_member, cost_individual,
                   hours, created_at, created_by, modified_at, modified_by)
                 VALUES (?1, ?2, ?3, 'Pendiente', ?4, ?5, (SELECT hours FROM short_courses WHERE uuid = ?3), ?6, ?7, ?6, ?7)",
                params![uuid, serial, offering, member, individual, now, who_name],
            )?;
        }
    }
    Ok(uuid)
}

/// OP-CRS-03 add a class date: courses take any number (`calendar_course_dates`); short courses up to 15
/// (`calendar_short_course_dates`, next repetition); a seminar has its one date on the row. Returns the date uuid.
pub fn add_class_date(
    tx: &Transaction,
    who: &Who,
    kind: Kind,
    calendar: &str,
    date: &str,
    professor: Option<&str>,
) -> DResult<String> {
    policy::ap03_domain(who)?;
    exists(tx, kind.calendar_table(), calendar)?;
    let date = crate::convention::iso_date("date", date)?;
    if let Some(p) = professor {
        exists(tx, "professors", p)
            .map_err(|_| DomainError::Validation("professor_uuid", "Escoja un profesor de la lista.".into()))?;
    }
    let uuid = new_uuid();
    match kind {
        Kind::Course => {
            let now = clock::now_text();
            tx.execute(
                "INSERT INTO calendar_course_dates (uuid, calendar_course_uuid, date, status, professor_uuid, created_at, created_by, modified_at, modified_by)
                 VALUES (?1, ?2, ?3, 'Día de Clase', ?4, ?5, ?6, ?5, ?6)",
                params![uuid, calendar, date, professor, now, who.actor.username],
            )?;
        }
        Kind::ShortCourse => {
            let n = count(
                tx,
                "SELECT COUNT(*) FROM calendar_short_course_dates WHERE calendar_short_course_uuid = ?1",
                calendar,
            )?;
            if n >= MAX_SHORT_DATES {
                return Err(DomainError::LimitReached(MAX_SHORT_DATES));
            }
            let rep = next_free_repetition(tx, calendar)?;
            tx.execute(
                "INSERT INTO calendar_short_course_dates (uuid, calendar_short_course_uuid, repetition, date, status, professor_uuid)
                 VALUES (?1, ?2, ?3, ?4, 'Día de Clase', ?5)",
                params![uuid, calendar, rep, date, professor],
            )?;
        }
        Kind::Seminar => {
            return Err(DomainError::Validation(
                "date",
                "Un seminario tiene una sola fecha: cámbiela en la fila.".into(),
            ));
        }
    }
    Ok(uuid)
}

/// The lowest free repetition 1..15.
fn next_free_repetition(conn: &Connection, calendar: &str) -> rusqlite::Result<i64> {
    let used: Vec<i64> = conn
        .prepare("SELECT repetition FROM calendar_short_course_dates WHERE calendar_short_course_uuid = ?1")?
        .query_map([calendar], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok((1..=MAX_SHORT_DATES).find(|r| !used.contains(r)).unwrap_or(MAX_SHORT_DATES))
}

/// OP-CRS-03 remove a class date (AP-06).
pub fn remove_class_date(tx: &Transaction, who: &Who, kind: Kind, date_uuid: &str) -> DResult<()> {
    policy::ap06_child_remove(who)?;
    let table = match kind {
        Kind::Course => "calendar_course_dates",
        Kind::ShortCourse => "calendar_short_course_dates",
        Kind::Seminar => return Err(DomainError::NotFound),
    };
    if tx.execute(&format!("DELETE FROM {table} WHERE uuid = ?1"), [date_uuid])? == 0 {
        return Err(DomainError::NotFound);
    }
    Ok(())
}

/// AP-07: a calendar row is never deleted (405). The routes register no DELETE; this keeps the domain answer.
pub fn delete_calendar(_tx: &Transaction, who: &Who, _kind: Kind, _calendar: &str) -> DResult<()> {
    super::policy::ap07_calendar_delete(who)
}

/// BR-CRS-01: the row is active (Pendiente or Confirmado).
pub fn is_active_status(status: &str) -> bool {
    matches!(status, "Pendiente" | "Confirmado")
}

/// BR-CRS-04: NMLS total in cents = hours x the NMLS rate setting, when the short course has `nmls_fee` = 1.
pub fn nmls_total(conn: &Connection, short_course: &str) -> DResult<Option<i64>> {
    let (fee, hours): (i64, Option<f64>) = conn
        .query_row("SELECT nmls_fee, hours FROM short_courses WHERE uuid = ?1", [short_course], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?
        .ok_or(DomainError::NotFound)?;
    if fee != 1 {
        return Ok(None);
    }
    let rate: i64 = crate::db::setting(conn, "nmls_cost_x_hour_cents")?.and_then(|v| v.parse().ok()).unwrap_or(150);
    Ok(Some((hours.unwrap_or(0.0) * rate as f64).round() as i64))
}

/// One calendar row with its summary (`v_calendar_*_summary`; BR-CRS-03, BR-RPT-02, -03). Money in cents.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CalendarRow {
    pub uuid: String,
    pub serial: Option<i64>,
    pub status: String,
    pub is_active: bool,
    pub first_date: Option<String>,
    pub semester: Option<String>,
    pub cost_member: Option<i64>,
    pub cost_individual: Option<i64>,
    pub quorum: Option<i64>,
    pub enrollments: i64,
    pub needed: Option<i64>,
    /// Course rows only (other kinds: None).
    pub count_activo: Option<i64>,
    pub count_baja: Option<i64>,
    pub revenue_member: Option<i64>,
    pub revenue_individual: Option<i64>,
    pub revenue_total: i64,
}

/// The calendar rows of an offering, newest first, from the summary view.
pub fn calendar_of(conn: &Connection, kind: Kind, offering: &str) -> rusqlite::Result<Vec<CalendarRow>> {
    let uuids: Vec<String> = conn
        .prepare(&format!("SELECT uuid FROM {} WHERE {} = ?1", kind.calendar_table(), kind.calendar_offering_col()))?
        .query_map([offering], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let mut rows = Vec::new();
    for u in uuids {
        if let Some(r) = calendar_row(conn, kind, &u)? {
            rows.push(r);
        }
    }
    rows.sort_by(|a, b| b.first_date.cmp(&a.first_date).then(b.serial.cmp(&a.serial)));
    Ok(rows)
}

/// One calendar row with its summary.
pub fn calendar_row(conn: &Connection, kind: Kind, uuid: &str) -> rusqlite::Result<Option<CalendarRow>> {
    let (view, key) = kind.summary_view();
    let course = kind == Kind::Course;
    let extra = if course {
        "v.count_activo, v.count_baja, v.revenue_member, v.revenue_individual, c.semester"
    } else {
        "NULL, NULL, NULL, NULL, NULL"
    };
    let sql = format!(
        "SELECT c.uuid, c.serial, c.status, v.is_active, c.cost_member, c.cost_individual, v.quorum, v.enrollments,
                v.needed, v.revenue_total, {extra}
           FROM {} c JOIN {view} v ON v.{key} = c.uuid WHERE c.uuid = ?1",
        kind.calendar_table()
    );
    let row = conn
        .query_row(&sql, [uuid], |r| {
            Ok(CalendarRow {
                uuid: r.get(0)?,
                serial: r.get(1)?,
                status: r.get(2)?,
                is_active: r.get::<_, i64>(3)? == 1,
                cost_member: r.get(4)?,
                cost_individual: r.get(5)?,
                quorum: r.get(6)?,
                enrollments: r.get(7)?,
                needed: r.get(8)?,
                revenue_total: r.get(9)?,
                count_activo: r.get(10)?,
                count_baja: r.get(11)?,
                revenue_member: r.get(12)?,
                revenue_individual: r.get(13)?,
                semester: r.get(14)?,
                first_date: None,
            })
        })
        .optional()?;
    Ok(match row {
        Some(mut r) => {
            r.first_date = conn.query_row(kind.first_date_sql(), [uuid], |x| x.get(0)).optional()?.flatten();
            Some(r)
        }
        None => None,
    })
}

/// A calendar row offered to an enrollment, with the price this student pays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarOption {
    pub uuid: String,
    pub first_date: Option<String>,
    pub status: String,
    /// Cents: member price for an affiliate student, else the individual price (BR-ENR-01).
    pub price: Option<i64>,
    pub member_price_applies: bool,
}

/// `GET /calendar-options/{kind}?offering=&student=` (T-05-11): only active rows of the offering (BR-CRS-01), each
/// with the price the student pays (BR-ENR-01, BR-AFF-01).
pub fn calendar_options(conn: &Connection, kind: Kind, offering: &str, student: &str) -> DResult<Vec<CalendarOption>> {
    let member = super::students::is_affiliate_student(conn, student)?;
    let uuids: Vec<(String, String, Option<i64>, Option<i64>)> = conn
        .prepare(&format!(
            "SELECT uuid, status, cost_member, cost_individual FROM {} WHERE {} = ?1 AND status IN ('Pendiente', 'Confirmado')",
            kind.calendar_table(),
            kind.calendar_offering_col()
        ))?
        .query_map([offering], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<Result<_, _>>()?;
    let mut out = Vec::new();
    for (uuid, status, m, i) in uuids {
        let first_date = conn.query_row(kind.first_date_sql(), [&uuid], |x| x.get(0)).optional()?.flatten();
        out.push(CalendarOption {
            uuid,
            first_date,
            status,
            price: if member { m } else { i },
            member_price_applies: member,
        });
    }
    out.sort_by(|a, b| a.first_date.cmp(&b.first_date));
    Ok(out)
}

/// OP-CRS-05: the enrollments of a calendar row (uuid, student name, status); none -> `NoEnrollments`.
pub fn calendar_enrollments(
    conn: &Connection,
    kind: Kind,
    calendar: &str,
) -> DResult<Vec<(String, String, Option<String>)>> {
    exists(conn, kind.calendar_table(), calendar)?;
    let rows: Vec<(String, String, Option<String>)> = conn
        .prepare(&format!(
            "SELECT e.uuid, s.nombre, e.status FROM {} e JOIN students s ON s.uuid = e.student_uuid
              WHERE e.{} = ?1 ORDER BY s.nombre",
            kind.enrollment_table(),
            kind.enrollment_calendar_col()
        ))?
        .query_map([calendar], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    if rows.is_empty() {
        return Err(DomainError::NoEnrollments);
    }
    Ok(rows)
}

pub(crate) fn exists(conn: &Connection, table: &str, uuid: &str) -> DResult<()> {
    conn.query_row(&format!("SELECT 1 FROM {table} WHERE uuid = ?1"), [uuid], |_| Ok(()))
        .optional()?
        .ok_or(DomainError::NotFound)
}
