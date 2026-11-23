//! I2 convention domain layer (MBA): people, events and event items, plans and plan items, sponsors and billing
//! files, registrations with items, guests and payments.
//!
//! Every OP-* of 07 for I2 is a plain function over a rusqlite [`Transaction`] that the caller opens with [`begin`]
//! (one `BEGIN IMMEDIATE` per command, 05). The BR-* invariants are enforced here, on the server, whatever the page
//! sends. Totals come only from the SQL views of 04 (`v_registration_item_lines`, `v_package_price`,
//! `v_registration_totals`, `v_event_totals`); nothing is stored as a snapshot (D-03). Money is integer cents end to
//! end; dollars appear only in templates (`autosave::dollars`, `seed::usd`).
//!
//! Policies AP-03..AP-06 and AP-09 are in [`policy`]; every command that needs one takes a [`Who`] and checks it
//! first (handlers still put `RequireStaff` on the route). Field edits go through the autosave registries in
//! [`registries`] and `autosave::registration::ITEM`.
//!
//! The route modules for the screens are in [`routes`] (phase 2 fills them).

pub mod events;
pub mod people;
pub mod plans;
pub mod policy;
pub mod registrations;
pub mod registries;
pub mod routes;
pub mod sponsors;
pub mod views;

use crate::audit::Actor;
use crate::auth::{CurrentUser, Role};
use crate::error::AppError;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use rusqlite::{Connection, Transaction, TransactionBehavior};

/// Who runs a command: the audit actor and the role from the `users` row (08: never from the client).
#[derive(Clone, Debug)]
pub struct Who {
    pub actor: Actor,
    pub role: Role,
}

impl Who {
    pub fn of(user: &CurrentUser) -> Self {
        Self { actor: user.actor(), role: user.role }
    }
    pub fn new(user_id: i64, username: &str, role: Role) -> Self {
        Self { actor: Actor::user(user_id, username), role }
    }
}

/// Stable domain errors (05 "Errors, concurrency, and observability"). [`DomainError::status`] maps each to the HTTP
/// answer the routes give: 403 policy, 404 missing, 409 domain refusal, 422 bad value, 200 + dialog for a confirm.
#[derive(Debug, PartialEq, Eq)]
pub enum DomainError {
    /// The role may not do this (AP-04, AP-05, AP-08).
    Forbidden,
    NotFound,
    /// A field value is wrong: (field, Spanish message).
    Validation(&'static str, String),
    /// A two-step command was sent without `confirm=1` (05 "Confirmations"); nothing was written.
    ConfirmRequired,
    /// The registration has payments (count, voided included: payments are never deleted, BR-REG-09).
    HasPayments(i64),
    /// The person, event or plan has registrations (count; AP-04, BR-PPL-02).
    HasRegistrations(i64),
    /// A row that others reference (an event item used by registration items, OP-EVT-03).
    InUse(i64),
    /// BR-REG-01: no event has status Activo.
    NoActiveEvent,
    /// BR-REG-01: the registration already has an event; the picker does not open.
    EventAlreadySet,
    /// BR-REG-02 / OP-REG-04: the plan or event item is not of the registration's event.
    NotOfEvent,
    /// BR-REG-02: pick the event before the plan.
    NoEvent,
    /// BR-EVT-06: the plan's event has no items.
    NoEventItems,
    /// BR-REG-09: the payment is voided already.
    AlreadyVoided,
    /// I3 BR-STU-07 / AP-04: the student or offering has enrollments (count).
    HasEnrollments(i64),
    /// I3 AP-04: the offering has calendar rows (count).
    HasCalendar(i64),
    /// I3 AP-07 / BR-CRS-05: calendar rows are never deleted; status Cancelado instead. HTTP 405.
    NoDelete,
    /// I3 BR-CRS-01: the calendar row is not active (Pendiente or Confirmado).
    CalendarNotActive,
    /// I3 OP-CRS-05: the calendar row has no enrollments.
    NoEnrollments,
    /// I3 BR-STU-05: at most 6 certifications of interest.
    LimitReached(i64),
    /// I3 BR-ENR-08: the enrollment has a payment; cancel it (BAJA or Canceló) instead.
    EnrollmentPaid,
    Db(String),
}

impl From<rusqlite::Error> for DomainError {
    fn from(e: rusqlite::Error) -> Self {
        match e {
            rusqlite::Error::QueryReturnedNoRows => DomainError::NotFound,
            e => DomainError::Db(e.to_string()),
        }
    }
}

impl DomainError {
    pub fn status(&self) -> StatusCode {
        match self {
            DomainError::Forbidden => StatusCode::FORBIDDEN,
            DomainError::NotFound => StatusCode::NOT_FOUND,
            DomainError::Validation(..) => StatusCode::UNPROCESSABLE_ENTITY,
            DomainError::ConfirmRequired => StatusCode::OK,
            DomainError::NoDelete => StatusCode::METHOD_NOT_ALLOWED,
            DomainError::Db(_) => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::CONFLICT,
        }
    }

    /// The Spanish message for the page (source wording where 07 quotes it).
    pub fn message(&self) -> String {
        match self {
            DomainError::Forbidden => "Su rol no permite esta acción.".into(),
            DomainError::NotFound => "El registro no existe.".into(),
            DomainError::Validation(_, m) => m.clone(),
            DomainError::ConfirmRequired => "Confirme la acción.".into(),
            DomainError::HasPayments(n) => {
                format!("El registro tiene {n} pago{}. No se borra; puede cancelarlo.", if *n == 1 { "" } else { "s" })
            }
            DomainError::HasRegistrations(n) => {
                format!("Tiene {n} registro{}. No se borra.", if *n == 1 { "" } else { "s" })
            }
            DomainError::InUse(n) => {
                format!("Lo usan {n} línea{} de registros. No se borra.", if *n == 1 { "" } else { "s" })
            }
            DomainError::NoActiveEvent => "Al momento no hay eventos activos.".into(),
            DomainError::EventAlreadySet => "El registro ya tiene evento.".into(),
            DomainError::NotOfEvent => "No es del evento de este registro.".into(),
            DomainError::NoEvent => "Favor de seleccionar el evento.".into(),
            DomainError::NoEventItems => {
                "El Evento seleccionado no tiene detalles. Añada actividades en la convención.".into()
            }
            DomainError::AlreadyVoided => "El pago ya está anulado.".into(),
            DomainError::HasEnrollments(n) => {
                format!("Tiene {n} matrícula{}. No se borra.", if *n == 1 { "" } else { "s" })
            }
            DomainError::HasCalendar(n) => {
                format!("Tiene {n} fecha{} en el calendario. No se borra.", if *n == 1 { "" } else { "s" })
            }
            DomainError::NoDelete => "Una fecha no se borra: cámbiela a Cancelado.".into(),
            DomainError::CalendarNotActive => "Esa fecha no está activa (Pendiente o Confirmado).".into(),
            DomainError::NoEnrollments => "Esta fecha no tiene matrículas.".into(),
            DomainError::LimitReached(n) => format!("Hasta {n} elementos; no se añade otro."),
            DomainError::EnrollmentPaid => "La matrícula tiene un pago. No se borra: cámbiela a BAJA o Canceló.".into(),
            DomainError::Db(_) => "Ocurrió un error. Intente otra vez.".into(),
        }
    }
}

/// 403 and 404 become the shell's error pages; the rest a message fragment with its status.
impl IntoResponse for DomainError {
    fn into_response(self) -> Response {
        match self {
            DomainError::Forbidden => AppError::Forbidden.into_response(),
            DomainError::NotFound => AppError::NotFound.into_response(),
            DomainError::Db(e) => AppError::Internal(format!("convention: {e}")).into_response(),
            e => {
                let html = format!(
                    "<div class=\"banner err\" role=\"alert\"><svg class=\"i\"><use href=\"#i-alert\"/></svg><div>{}</div></div>",
                    askama::filters::escape(e.message(), askama::filters::Html).map(|s| s.to_string()).unwrap_or_default()
                );
                (e.status(), Html(html)).into_response()
            }
        }
    }
}

pub type DResult<T> = Result<T, DomainError>;

/// One command, one `BEGIN IMMEDIATE` transaction (05).
pub fn begin(conn: &mut Connection) -> rusqlite::Result<Transaction<'_>> {
    conn.transaction_with_behavior(TransactionBehavior::Immediate)
}

/// A new row key (UUID v4 text, 04 convention).
pub fn new_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// `YYYY-MM-DD`, or a Validation error on `field`.
pub fn iso_date(field: &'static str, s: &str) -> DResult<String> {
    let t = s.trim();
    chrono::NaiveDate::parse_from_str(t, "%Y-%m-%d")
        .map(|d| d.format("%Y-%m-%d").to_string())
        .map_err(|_| DomainError::Validation(field, "Use una fecha AAAA-MM-DD.".into()))
}

/// Text that must not be empty.
pub fn required(field: &'static str, s: &str, max: usize) -> DResult<String> {
    crate::autosave::required_text(s, max)
        .map(|v| crate::autosave::text_of(&v))
        .map_err(|m| DomainError::Validation(field, m))
}

/// Text that may be empty (None).
pub fn optional(field: &'static str, s: &str, max: usize) -> DResult<Option<String>> {
    match crate::autosave::optional_text(s, max) {
        Ok(rusqlite::types::Value::Text(t)) => Ok(Some(t)),
        Ok(_) => Ok(None),
        Err(m) => Err(DomainError::Validation(field, m)),
    }
}

/// `COUNT(*)` helper.
pub(crate) fn count(conn: &Connection, sql: &str, arg: &str) -> rusqlite::Result<i64> {
    conn.query_row(sql, [arg], |r| r.get(0))
}

/// Next value of an auto-serial column (registration number, invoice number), read inside the insert's transaction.
pub(crate) fn next_serial(conn: &Connection, table: &str, col: &str, floor: i64) -> rusqlite::Result<i64> {
    conn.query_row(&format!("SELECT MAX(COALESCE(MAX({col}), ?1), ?1) + 1 FROM {table}"), [floor], |r| r.get(0))
}
