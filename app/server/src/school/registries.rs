//! Autosave registries of the I3 record resources (05 per-field PATCH rows of STU, AFF, CRT, CRS, ENR and the
//! admin rows of UI-32) and [`patch`], the one entry point the phase-2 handlers call.
//!
//! Domain rules inside the save transaction (the check runs before the write, in the same transaction):
//! * `students.member_uuid` (BR-STU-02, changed): the affiliate must exist; its name is copied into `compania`.
//!   Empty unlinks and keeps the typed company (BR-AFF-02).
//! * `enrollments_course.promedio_final` (BR-ENR-03): `puntuacion` takes its rounded value.
//! * professors named on calendar rows and class dates must exist; professor names are unique.
//! * Certifications, requirements and professors are configuration: ROLE-ADMIN only (AP-08).

use super::{enrollments, offerings, DomainError, Who};
use crate::autosave::{self, Ctx, FieldDef, Outcome, Refusal, Registry, Stamp};
use crate::convention::registries::{parse_date, parse_money, parse_money_opt, show_date, show_money};
use crate::error::AppError;
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension};

// ---------------------------------------------------------------------------------------------------------------
// Parsers
// ---------------------------------------------------------------------------------------------------------------

fn t30(s: &str) -> Result<Value, String> {
    autosave::optional_text(s, 30)
}
fn t60(s: &str) -> Result<Value, String> {
    autosave::optional_text(s, 60)
}
fn t100(s: &str) -> Result<Value, String> {
    autosave::optional_text(s, 100)
}
fn t200(s: &str) -> Result<Value, String> {
    autosave::optional_text(s, 200)
}
fn note(s: &str) -> Result<Value, String> {
    autosave::optional_text(s, 4000)
}
fn name200(s: &str) -> Result<Value, String> {
    autosave::required_text(s, 200)
}
fn uuid_opt(s: &str) -> Result<Value, String> {
    autosave::optional_text(s, 64)
}
fn email(s: &str) -> Result<Value, String> {
    let v = autosave::optional_text(s, 200)?;
    if let Value::Text(t) = &v {
        let ok = t.split_once('@').is_some_and(|(a, d)| !a.is_empty() && d.contains('.') && !d.ends_with('.'));
        if !ok || t.contains(' ') {
            return Err("Escriba un e-mail como nombre@ejemplo.com.".into());
        }
    }
    Ok(v)
}
fn one_of(s: &str, allowed: &[&str], msg: &str) -> Result<Value, String> {
    let t = s.trim();
    allowed.iter().find(|a| **a == t).map(|a| Value::Text((*a).to_owned())).ok_or_else(|| msg.to_owned())
}
fn one_of_opt(s: &str, allowed: &[&str], msg: &str) -> Result<Value, String> {
    if s.trim().is_empty() {
        Ok(Value::Null)
    } else {
        one_of(s, allowed, msg)
    }
}
/// A grade 0..100 with up to two decimals; empty = NULL (BR-ENR-03 range).
pub fn grade(s: &str) -> Result<Value, String> {
    let t = s.trim().replace(',', ".");
    if t.is_empty() {
        return Ok(Value::Null);
    }
    match t.parse::<f64>() {
        Ok(v) if (0.0..=100.0).contains(&v) => Ok(Value::Real((v * 100.0).round() / 100.0)),
        Ok(_) => Err("Use una nota entre 0 y 100.".into()),
        Err(_) => Err("Escriba un número.".into()),
    }
}
/// The score: a whole number 0..100; empty = NULL (BR-ENR-03; T-07-11: 101 refused).
pub fn score(s: &str) -> Result<Value, String> {
    if s.trim().is_empty() {
        return Ok(Value::Null);
    }
    autosave::int_in(s, 0, 100).map(Value::Integer)
}
fn int_opt(s: &str, hi: i64) -> Result<Value, String> {
    if s.trim().is_empty() {
        Ok(Value::Null)
    } else {
        autosave::int_in(s, 0, hi).map(Value::Integer)
    }
}
fn small_int(s: &str) -> Result<Value, String> {
    int_opt(s, 999)
}
fn quorum(s: &str) -> Result<Value, String> {
    int_opt(s, 500)
}
fn hours(s: &str) -> Result<Value, String> {
    let t = s.trim().replace(',', ".");
    if t.is_empty() {
        return Ok(Value::Null);
    }
    match t.parse::<f64>() {
        Ok(v) if (0.0..=1000.0).contains(&v) => Ok(Value::Real(v)),
        _ => Err("Escriba un número de 0 a 1000.".into()),
    }
}
fn cust_type(s: &str) -> Result<Value, String> {
    one_of_opt(s, super::affiliates::CUST_TYPES, "Escoja ACTIVO, INACTIVO o No Afiliado.")
}
fn membership(s: &str) -> Result<Value, String> {
    one_of_opt(s, super::affiliates::MEMBERSHIPS, "Escoja una membresía de la lista.")
}
fn cal_status(s: &str) -> Result<Value, String> {
    one_of(s, offerings::CALENDAR_STATUSES, "Escoja un estatus de la lista.")
}
fn day_type(s: &str) -> Result<Value, String> {
    one_of(s, offerings::DAY_TYPES, "Escoja Día de Clase o Día de Fiesta.")
}
fn course_status(s: &str) -> Result<Value, String> {
    one_of(s, enrollments::COURSE_STATUSES, "Escoja ACTIVO o BAJA.")
}
fn irregular_status(s: &str) -> Result<Value, String> {
    one_of_opt(s, enrollments::IRREGULAR_STATUSES, "Escoja Asistió, Canceló o No Canceló.")
}
fn payer(s: &str) -> Result<Value, String> {
    one_of_opt(s, enrollments::PAYERS, "Escoja COMPAÑIA o INDIVIDUAL.")
}
fn req_type(s: &str) -> Result<Value, String> {
    one_of(s, super::certifications::REQUIREMENT_TYPES, "Escoja Requisito o Electiva.")
}
fn electives(s: &str) -> Result<Value, String> {
    autosave::int_in(s, 0, 20).map(Value::Integer)
}
fn show_real(v: &Value) -> String {
    match v {
        Value::Real(r) if r.fract() == 0.0 => format!("{}", *r as i64),
        Value::Real(r) => format!("{r}"),
        Value::Integer(i) => i.to_string(),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Registries
// ---------------------------------------------------------------------------------------------------------------

/// `PATCH /students/{id}` (UI-15 Datos personales, OP-STU-03). `member_uuid` picks the affiliate (BR-STU-02).
/// There is no SSN field (D-06).
pub static STUDENT: Registry = Registry {
    table: "students",
    key_col: "uuid",
    stamp: Stamp::Full,
    fields: &[
        FieldDef::new("nombre", "nombre", "Nombre", name200),
        FieldDef::new("member_uuid", "member_uuid", "Afiliado", uuid_opt),
        FieldDef::new("compania", "compania", "Compañía", t200),
        FieldDef::new("company_position", "company_position", "Puesto", t100),
        FieldDef::new("company_department", "company_department", "Departamento", t100),
        FieldDef::new("nmls", "nmls", "NMLS", t30),
        FieldDef::new("student_id", "student_id", "Número de estudiante", t30),
        FieldDef::new("email", "email", "E-mail", email),
        FieldDef::new("email_work", "email_work", "E-mail trabajo", email),
        FieldDef::new("telefono_r", "telefono_r", "Teléfono residencial", t30),
        FieldDef::new("telefono_t", "telefono_t", "Teléfono trabajo", t30),
        FieldDef::new("phone_mobile", "phone_mobile", "Teléfono móvil", t30),
        FieldDef::new("fax", "fax", "Fax", t30),
        FieldDef::new("dir_residencial", "dir_residencial", "Dirección 1", t200),
        FieldDef::new("residencial_2", "residencial_2", "Dirección 2", t200),
        FieldDef::new("dir_trabajo", "dir_trabajo", "Dirección trabajo", t200),
        FieldDef::new("home_city", "home_city", "Pueblo", t100),
        FieldDef::new("home_state", "home_state", "Estado", t30),
        FieldDef::new("home_zip", "home_zip", "Zip", t30),
        FieldDef::new("emergency_contact", "emergency_contact", "Emergencia", t100),
        FieldDef::new("emergency_phone", "emergency_phone", "Tel. emergencia", t30),
        FieldDef::new("emergency_contact_relationship", "emergency_contact_relationship", "Parentesco", t60),
        FieldDef::new("work_experience", "work_experience", "Experiencia", note),
        FieldDef::new("notes", "notes", "Notas", note),
    ],
};

/// `PATCH /student-certifications/{id}` (OP-CRT-02, BR-CRT-04): graduation date and note.
pub static STUDENT_CERT: Registry = Registry {
    table: "student_certifications",
    key_col: "uuid",
    stamp: Stamp::Full,
    fields: &[
        FieldDef::new("graduation_date", "graduation_date", "Graduación", parse_date).show(show_date),
        FieldDef::new("nota", "nota", "Nota", t200),
    ],
};

/// `PATCH /affiliates/{id}` (UI-17, OP-AFF-01).
pub static AFFILIATE: Registry = Registry {
    table: "affiliates",
    key_col: "uuid",
    stamp: Stamp::Full,
    fields: &[
        FieldDef::new("socio", "socio", "Afiliado", name200),
        FieldDef::new("other_name", "other_name", "Otro nombre", t200),
        FieldDef::new("tipo_de_negocio", "tipo_de_negocio", "Tipo de negocio", t100),
        FieldDef::new("miembro", "miembro", "Membresía", membership),
        FieldDef::new("cust_type", "cust_type", "Tipo", cust_type),
        FieldDef::new("e_mail", "e_mail", "Email", email),
        FieldDef::new("customer_e_mail", "customer_e_mail", "Email de facturación", email),
        FieldDef::new("telephone", "telephone", "Teléfono 1", t30),
        FieldDef::new("telephone_1", "telephone_1", "Teléfono 2", t30),
        FieldDef::new("fax", "fax", "Fax", t30),
        FieldDef::new("address", "address", "Dirección", t200),
        FieldDef::new("address1", "address1", "Facturar a", t200),
        FieldDef::new("city", "city", "Pueblo-Ciudad", t100),
        FieldDef::new("state_zip", "state_zip", "Estado", t30),
        FieldDef::new("zip", "zip", "Código postal", t30),
    ],
};

/// `PATCH /offerings/course/{id}` (UI-18 Detalles).
pub static COURSE: Registry = Registry {
    table: "courses",
    key_col: "uuid",
    stamp: Stamp::Full,
    fields: &[
        FieldDef::new("course", "course", "Curso", name200),
        FieldDef::new("course_description", "course_description", "Descripción", note),
        FieldDef::new("course_requirements", "course_requirements", "Requisitos", note),
        FieldDef::new("credits", "credits", "Créditos", hours).show(show_real),
        FieldDef::new("hours", "hours", "Horas", hours).show(show_real),
        FieldDef::new("quorum", "quorum", "Quórum", quorum),
        FieldDef::new("cost_member", "cost_member", "Precio afiliado", parse_money_opt).show(show_money),
        FieldDef::new("cost_individual", "cost_individual", "Precio no afiliado", parse_money_opt).show(show_money),
    ],
};

/// `PATCH /offerings/seminar/{id}`.
pub static SEMINAR: Registry = Registry {
    table: "seminars",
    key_col: "uuid",
    stamp: Stamp::Full,
    fields: &[
        FieldDef::new("seminar", "seminar", "Seminario", name200),
        FieldDef::new("description", "description", "Descripción", note),
        FieldDef::new("hours", "hours", "Horas", hours).show(show_real),
        FieldDef::new("cost_affiliate", "cost_affiliate", "Precio afiliado", parse_money_opt).show(show_money),
        FieldDef::new("cost_individual", "cost_individual", "Precio no afiliado", parse_money_opt).show(show_money),
    ],
};

/// `PATCH /offerings/short-course/{id}`; `nmls_fee` drives BR-CRS-04.
pub static SHORT_COURSE: Registry = Registry {
    table: "short_courses",
    key_col: "uuid",
    stamp: Stamp::Full,
    fields: &[
        FieldDef::new("short_course", "short_course", "Curso corto", name200),
        FieldDef::new("description", "description", "Descripción", note),
        FieldDef::new("hours", "hours", "Horas", hours).show(show_real),
        FieldDef::new("nmls_fee", "nmls_fee", "Cargo NMLS", autosave::flag),
        FieldDef::new("cost_affiliate", "cost_affiliate", "Precio afiliado", parse_money_opt).show(show_money),
        FieldDef::new("cost_individual", "cost_individual", "Precio no afiliado", parse_money_opt).show(show_money),
    ],
};

/// `PATCH /calendar/course/{id}` (UI-19). No delete exists (AP-07); status Cancelado instead.
pub static CAL_COURSE: Registry = Registry {
    table: "calendar_course",
    key_col: "uuid",
    stamp: Stamp::Full,
    fields: &[
        FieldDef::new("status", "status", "Estatus", cal_status),
        FieldDef::new("semester", "semester", "Semestre", t60),
        FieldDef::new("cost_member", "cost_member", "Precio socio", parse_money_opt).show(show_money),
        FieldDef::new("cost_individual", "cost_individual", "Precio individual", parse_money_opt).show(show_money),
    ],
};

/// `PATCH /calendar/seminar/{id}`.
pub static CAL_SEMINAR: Registry = Registry {
    table: "calendar_seminar",
    key_col: "uuid",
    stamp: Stamp::Full,
    fields: &[
        FieldDef::new("status", "status", "Estatus", cal_status),
        FieldDef::new("date", "date", "Fecha", parse_date).show(show_date),
        FieldDef::new("quorum", "quorum", "Quórum", quorum),
        FieldDef::new("professor_uuid", "professor_uuid", "Profesor", uuid_opt),
        FieldDef::new("cost_member", "cost_member", "Precio socio", parse_money_opt).show(show_money),
        FieldDef::new("cost_individual", "cost_individual", "Precio individual", parse_money_opt).show(show_money),
    ],
};

/// `PATCH /calendar/short-course/{id}`.
pub static CAL_SHORT: Registry = Registry {
    table: "calendar_short_course",
    key_col: "uuid",
    stamp: Stamp::Full,
    fields: &[
        FieldDef::new("status", "status", "Estatus", cal_status),
        FieldDef::new("quorum", "quorum", "Quórum", quorum),
        FieldDef::new("hours", "hours", "Horas", hours).show(show_real),
        FieldDef::new("cost_member", "cost_member", "Precio socio", parse_money_opt).show(show_money),
        FieldDef::new("cost_individual", "cost_individual", "Precio individual", parse_money_opt).show(show_money),
    ],
};

/// A course class date (UI-19 "Fechas del curso").
pub static DATE_COURSE: Registry = Registry {
    table: "calendar_course_dates",
    key_col: "uuid",
    stamp: Stamp::Full,
    fields: &[
        FieldDef::new("date", "date", "Fecha", parse_date).show(show_date),
        FieldDef::new("status", "status", "Tipo de día", day_type),
        FieldDef::new("note", "note", "Nota", t200),
        FieldDef::new("professor_uuid", "professor_uuid", "Profesor", uuid_opt),
    ],
};

/// A short-course class date (up to 15; no stamp columns in 04).
pub static DATE_SHORT: Registry = Registry {
    table: "calendar_short_course_dates",
    key_col: "uuid",
    stamp: Stamp::None,
    fields: &[
        FieldDef::new("date", "date", "Fecha", parse_date).show(show_date),
        FieldDef::new("status", "status", "Tipo de día", day_type),
        FieldDef::new("professor_uuid", "professor_uuid", "Profesor", uuid_opt),
    ],
};

/// `PATCH /enrollments/course/{id}` (UI-15, UI-21 line fields; OP-ENR-04, OP-ENR-08). The price (`costo`) is stored
/// at enrollment (BR-ENR-01) and is not edited here.
pub static ENR_COURSE: Registry = Registry {
    table: "enrollments_course",
    key_col: "uuid",
    stamp: Stamp::Full,
    fields: &[
        FieldDef::new("status", "status", "Estatus", course_status),
        FieldDef::new("costeado_por", "costeado_por", "Costeado por", payer),
        FieldDef::new("pago", "pago", "Pago", parse_money).show(show_money),
        FieldDef::new("fecha_de_pago", "fecha_de_pago", "Fecha de pago", parse_date).show(show_date),
        FieldDef::new("ausencias", "ausencias", "Ausencias", small_int),
        FieldDef::new("tardanzas", "tardanzas", "Tardanzas", small_int),
        FieldDef::new("indicar_cambios", "indicar_cambios", "Cambios", note),
        FieldDef::new("expectations", "expectations", "Expectativas", note),
        FieldDef::new("examen", "examen", "Examen", grade).show(show_real),
        FieldDef::new("primera", "primera", "1ra", grade).show(show_real),
        FieldDef::new("segunda", "segunda", "2da", grade).show(show_real),
        FieldDef::new("tercera", "tercera", "3ra", grade).show(show_real),
        FieldDef::new("cuarta", "cuarta", "4ta", grade).show(show_real),
        FieldDef::new("quinta", "quinta", "5ta", grade).show(show_real),
        FieldDef::new("sexta", "sexta", "6ta", grade).show(show_real),
        FieldDef::new("septima", "septima", "7ma", grade).show(show_real),
        FieldDef::new("octava", "octava", "8va", grade).show(show_real),
        FieldDef::new("promedio_asignaciones", "promedio_asignaciones", "Prom. asignaciones", grade).show(show_real),
        FieldDef::new("asistencia_y_puntualidad", "asistencia_y_puntualidad", "Asistencia", grade).show(show_real),
        FieldDef::new("promedio_final", "promedio_final", "Promedio final", grade).show(show_real),
        FieldDef::new("puntuacion", "puntuacion", "Puntuación", score),
    ],
};

/// `PATCH /enrollments/seminar/{id}`.
pub static ENR_SEMINAR: Registry = Registry {
    table: "enrollments_seminars",
    key_col: "uuid",
    stamp: Stamp::Full,
    fields: &[
        FieldDef::new("status", "status", "Estatus", irregular_status),
        FieldDef::new("costeado_por", "costeado_por", "Costeado por", payer),
        FieldDef::new("pago", "pago", "Pago", parse_money).show(show_money),
        FieldDef::new("date_of_payment", "date_of_payment", "Fecha de pago", parse_date).show(show_date),
        FieldDef::new("check_number", "check_number", "Cheque", t30),
        FieldDef::new("notas", "notas", "Notas", note),
        FieldDef::new("expectations", "expectations", "Expectativas", note),
    ],
};

/// `PATCH /enrollments/short-course/{id}`.
pub static ENR_SHORT: Registry = Registry {
    table: "enrollments_short_course",
    key_col: "uuid",
    stamp: Stamp::Full,
    fields: &[
        FieldDef::new("status", "status", "Estatus", irregular_status),
        FieldDef::new("costeado_por", "costeado_por", "Costeado por", payer),
        FieldDef::new("pago", "pago", "Pago", parse_money).show(show_money),
        FieldDef::new("date_of_payment", "date_of_payment", "Fecha de pago", parse_date).show(show_date),
        FieldDef::new("check_number", "check_number", "Cheque", t30),
        FieldDef::new("license_number", "license_number", "Licencia", t30),
        FieldDef::new("notas", "notas", "Notas", note),
        FieldDef::new("expectations", "expectations", "Expectativas", note),
    ],
};

/// `PATCH /admin/certifications/{id}` (UI-32, AP-08).
pub static CERTIFICATION: Registry = Registry {
    table: "certifications",
    key_col: "uuid",
    stamp: Stamp::Full,
    fields: &[
        FieldDef::new("certification", "certification", "Certificación", name200),
        FieldDef::new("description", "description", "Nombre", t200),
        FieldDef::new(
            "number_of_required_electives",
            "number_of_required_electives",
            "Electivas requeridas",
            electives,
        ),
    ],
};

/// A requirement row of UI-32 (AP-08): type and other requirements.
pub static REQUIREMENT: Registry = Registry {
    table: "certification_requirements",
    key_col: "uuid",
    stamp: Stamp::Full,
    fields: &[
        FieldDef::new("type", "type", "Tipo", req_type),
        FieldDef::new("other_requirements", "other_requirements", "Otros requisitos", note),
    ],
};

/// `PATCH /admin/professors/{id}` (UI-32, OP-CRS-04, AP-08).
pub static PROFESSOR: Registry = Registry {
    table: "professors",
    key_col: "uuid",
    stamp: Stamp::Full,
    fields: &[FieldDef::new("name", "name", "Profesor", name200)],
};

// ---------------------------------------------------------------------------------------------------------------
// The entry point
// ---------------------------------------------------------------------------------------------------------------

/// The I3 autosave resources.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resource {
    Student,
    StudentCert,
    Affiliate,
    Offering(super::Kind),
    Calendar(super::Kind),
    DateCourse,
    DateShort,
    Enrollment(super::Kind),
    Certification,
    Requirement,
    Professor,
}

impl Resource {
    pub fn registry(self) -> &'static Registry {
        use super::Kind::*;
        match self {
            Resource::Student => &STUDENT,
            Resource::StudentCert => &STUDENT_CERT,
            Resource::Affiliate => &AFFILIATE,
            Resource::Offering(Course) => &COURSE,
            Resource::Offering(Seminar) => &SEMINAR,
            Resource::Offering(ShortCourse) => &SHORT_COURSE,
            Resource::Calendar(Course) => &CAL_COURSE,
            Resource::Calendar(Seminar) => &CAL_SEMINAR,
            Resource::Calendar(ShortCourse) => &CAL_SHORT,
            Resource::DateCourse => &DATE_COURSE,
            Resource::DateShort => &DATE_SHORT,
            Resource::Enrollment(Course) => &ENR_COURSE,
            Resource::Enrollment(Seminar) => &ENR_SEMINAR,
            Resource::Enrollment(ShortCourse) => &ENR_SHORT,
            Resource::Certification => &CERTIFICATION,
            Resource::Requirement => &REQUIREMENT,
            Resource::Professor => &PROFESSOR,
        }
    }

    /// Configuration resources (AP-08).
    pub fn admin_only(self) -> bool {
        matches!(self, Resource::Certification | Resource::Requirement | Resource::Professor)
    }

    fn label(self) -> &'static str {
        match self {
            Resource::Student => "Estudiante",
            Resource::StudentCert => "Certificación del estudiante",
            Resource::Affiliate => "Afiliado",
            Resource::Offering(_) => "Ofrecimiento",
            Resource::Calendar(_) => "Fecha del curso",
            Resource::DateCourse | Resource::DateShort => "Día de clase",
            Resource::Enrollment(_) => "Matrícula",
            Resource::Certification => "Certificación",
            Resource::Requirement => "Requisito",
            Resource::Professor => "Profesor",
        }
    }
}

pub use crate::convention::registries::Patched;

/// Saves one field of one I3 record with its domain rules. Policy: AP-03 (both roles), AP-08 for configuration.
pub fn patch(
    conn: &mut Connection,
    who: &Who,
    request_id: Option<&str>,
    resource: Resource,
    key: &str,
    name: &str,
    raw: &str,
) -> Result<Patched, AppError> {
    super::policy::ap03_domain(who).map_err(|_| AppError::Forbidden)?;
    if resource.admin_only() {
        super::policy::ap08_config(who).map_err(|_| AppError::Forbidden)?;
    }
    let reg = resource.registry();
    let Some(def) = reg.get(name) else { return Ok(Patched::Unknown) };
    let n: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {} WHERE uuid = ?1", reg.table), [key], |r| r.get(0))?;
    if n == 0 {
        return Err(AppError::NotFound);
    }
    let ctx = Ctx { actor: &who.actor, request_id, record: resource.label() };
    let keyv = Value::Text(key.to_owned());
    let outcome = autosave::save(conn, reg, &keyv, def, raw, &ctx, |tx, _old, new| {
        match (resource, def.name) {
            (Resource::Student, "member_uuid") => {
                if let Value::Text(a) = new {
                    let socio: Option<String> =
                        tx.query_row("SELECT socio FROM affiliates WHERE uuid = ?1", [a], |r| r.get(0)).optional()?;
                    let Some(socio) = socio else {
                        return Err(Refusal::Invalid("Escoja un afiliado de la lista.".into()));
                    };
                    tx.execute("UPDATE students SET compania = ?1 WHERE uuid = ?2", [&socio, key])?;
                }
            }
            (Resource::Enrollment(super::Kind::Course), "promedio_final") => {
                let score = match new {
                    Value::Real(f) => Some(f.round() as i64),
                    Value::Integer(i) => Some(*i),
                    _ => None,
                };
                if let Some(s) = score {
                    tx.execute(
                        "UPDATE enrollments_course SET puntuacion = ?1 WHERE uuid = ?2",
                        rusqlite::params![s, key],
                    )?;
                }
            }
            (_, "professor_uuid") => {
                if let Value::Text(p) = new {
                    let ok =
                        tx.query_row("SELECT 1 FROM professors WHERE uuid = ?1", [p], |_| Ok(())).optional()?.is_some();
                    if !ok {
                        return Err(Refusal::Invalid("Escoja un profesor de la lista.".into()));
                    }
                }
            }
            (Resource::Professor, "name") => {
                if let Value::Text(n) = new {
                    let dup = tx
                        .query_row("SELECT 1 FROM professors WHERE name = ?1 AND uuid <> ?2", [n, key], |_| Ok(()))
                        .optional()?;
                    if dup.is_some() {
                        return Err(Refusal::Invalid("Ya existe un profesor con ese nombre.".into()));
                    }
                }
            }
            _ => {}
        }
        Ok(())
    })?;
    Ok(Patched::Done(outcome))
}

/// The refusal message of a domain error inside a save check.
pub fn refusal(e: DomainError) -> Refusal {
    match e {
        DomainError::Db(m) => Refusal::Db(rusqlite::Error::InvalidParameterName(m)),
        e => Refusal::Invalid(e.message()),
    }
}

/// Saved outcome helper for tests and handlers.
pub fn saved(p: &Patched) -> bool {
    matches!(p, Patched::Done(Outcome::Saved { .. }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsers() {
        assert_eq!(score("100").unwrap(), Value::Integer(100));
        assert!(score("101").is_err() && score("-1").is_err() && score("9.5").is_err());
        assert_eq!(score("").unwrap(), Value::Null);
        assert_eq!(grade("91,5").unwrap(), Value::Real(91.5));
        assert!(grade("100.5").is_err());
        assert_eq!(show_real(&Value::Real(45.0)), "45");
    }
}
