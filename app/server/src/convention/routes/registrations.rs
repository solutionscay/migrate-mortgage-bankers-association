//! Phase-2 pane "registrations": UI-05 Registros (`GET /registrations`), UI-06 Registro (`/registrations/{id}` and
//! every `/registrations/{id}/...` route of 05: event and plan pickers, items, attributes, guests, payments,
//! cancel, delete; `POST /payments/{id}/void`), UI-07 Registro en puerta (`GET /events/{id}/door`,
//! `POST /registrations/{id}/attendance`).
//!
//! Domain: `convention::registrations`, `convention::registries::{patch, Resource::{Registration, Item, Guest}}`,
//! `convention::views::{TotalsBlock, totales_oob}`. Templates: `templates/convention/registrations/`.
//!
//! Save model (06 "Save model"): header fields, line fields and guest fields autosave one field per `PATCH`. Dialogs
//! that create or settle (payment, void, attributes, plan confirm, cancel, delete) are plain forms: the server
//! answers 303 back to the page, or the page again with the dialog open and the message (409/422).
//! `{id}` of a registration is its uuid; a registration number also opens the page (`/registrations/1283`).

use crate::audit::RequestId;
use crate::auth::{CurrentUser, RequireRole, RequireStaff, Shell};
use crate::autosave::{self, Control, FieldView, OneField, Outcome};
use crate::convention::registrations::{self as regs, AttrKind, Line, Totals};
use crate::convention::registries::{self, show_date, show_time, Patched, Resource, Target};
use crate::convention::views::{totales_oob, usd, TotalsBlock};
use crate::convention::{begin, plans, DomainError, Who};
use crate::error::AppError;
use crate::web::{is_htmx, render_page, render_page_status, urlencode, Page};
use crate::AppState;
use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Extension, Form, Router};
use rusqlite::types::Value;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use std::borrow::Borrow;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/registrations", get(list))
        .route("/registrations/{id}", get(page).patch(patch_header).delete(delete_registration))
        .route("/registrations/{id}/cancel", post(cancel))
        .route("/registrations/{id}/event-picker", get(event_picker))
        .route("/registrations/{id}/event", post(select_event))
        .route("/registrations/{id}/plan-picker", get(plan_picker))
        .route("/registrations/{id}/plan", post(select_plan))
        .route("/registrations/{id}/items", post(add_item))
        .route("/registrations/{id}/items/{item}", axum::routing::patch(patch_item).delete(remove_item))
        .route("/registrations/{id}/items/{item}/attributes", get(attributes).post(save_attributes))
        .route("/registrations/{id}/guests", post(add_guest))
        .route("/registrations/{id}/guests/{guest}", axum::routing::patch(patch_guest).delete(remove_guest))
        .route("/registrations/{id}/payments", post(add_payment))
        .route("/registrations/{id}/attendance", post(attendance))
        .route("/payments/{id}/void", post(void_payment))
        .route("/events/{id}/door", get(door))
}

type Rid = Option<Extension<RequestId>>;

fn rid(r: &Rid) -> Option<&str> {
    r.as_ref().map(|e| e.0 .0.as_str())
}

fn reg_url(uuid: &str) -> String {
    format!("/registrations/{uuid}")
}

/// The registration uuid of `{id}`: a uuid, or a registration number.
fn resolve(conn: &Connection, id: &str) -> Result<String, AppError> {
    let found: Option<String> = if let Ok(n) = id.parse::<i64>() {
        conn.query_row("SELECT uuid FROM registration WHERE registration_number = ?1", [n], |r| r.get(0)).optional()?
    } else {
        conn.query_row("SELECT uuid FROM registration WHERE uuid = ?1", [id], |r| r.get(0)).optional()?
    };
    found.ok_or(AppError::NotFound)
}

fn domain(e: DomainError) -> AppError {
    match e {
        DomainError::Forbidden => AppError::Forbidden,
        DomainError::NotFound => AppError::NotFound,
        e => AppError::Internal(format!("{e:?}")),
    }
}

/// "5/6/2027" from an ISO date.
fn d(iso: &Option<String>) -> String {
    show_date(&iso.clone().map(Value::Text).unwrap_or(Value::Null))
}

/// "8:30 AM" from `08:30`.
fn t(hm: &Option<String>) -> String {
    show_time(&hm.clone().map(Value::Text).unwrap_or(Value::Null))
}

/// "5/6 – 5/8/2027" (design event range).
fn date_range(start: &Option<String>, end: &Option<String>) -> String {
    use chrono::{Datelike, NaiveDate};
    let p = |s: &Option<String>| s.as_deref().and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
    match (p(start), p(end)) {
        (Some(a), Some(b)) if a != b && a.year() == b.year() => {
            format!("{}/{} – {}/{}/{}", a.month(), a.day(), b.month(), b.day(), b.year())
        }
        (Some(a), Some(b)) if a != b => format!("{} – {}", d(start), d(end)),
        (Some(_), _) => d(start),
        _ => String::new(),
    }
}

/// The tag class of a status (registros.html).
fn status_tag(status: &str) -> &'static str {
    match status {
        "Registrado" => "tag info",
        "Asistió" => "tag ok",
        "Canceló" => "tag err",
        "No Canceló" => "tag warn",
        _ => "tag",
    }
}

/// The short plan code of a plan name: "P2(B)" from "P2(B) · 15 marzo".
fn plan_code(name: &str) -> String {
    name.split(" · ").next().unwrap_or(name).trim().to_owned()
}

// ---------------------------------------------------------------------------------------------------------------
// UI-05 Registros
// ---------------------------------------------------------------------------------------------------------------

const PAGE_SIZE: i64 = 50;

#[derive(Deserialize, Default)]
struct ListQuery {
    event: Option<String>,
    status: Option<String>,
    tipo: Option<String>,
    q: Option<String>,
    view: Option<String>,
    page: Option<i64>,
    hecho: Option<String>,
}

pub struct ListRow {
    pub uuid: String,
    pub number: String,
    pub name: String,
    pub company: String,
    pub kind: String,
    pub status: String,
    pub tag: &'static str,
    pub total: String,
    pub balance: String,
}

#[derive(Template)]
#[template(path = "convention/registrations/list.html", blocks = ["content"])]
pub struct ListPage {
    pub shell: Shell,
    pub rows: Vec<ListRow>,
    /// (value, label, selected)
    pub events: Vec<(String, String, bool)>,
    pub statuses: Vec<(String, bool)>,
    pub tipos: Vec<(String, bool)>,
    pub q: String,
    pub event_uuid: Option<String>,
    pub event_name: String,
    pub table_view: bool,
    pub total: i64,
    pub first: i64,
    pub last: i64,
    pub page: i64,
    pub pages: Vec<i64>,
    pub page_count: i64,
    pub notice: Option<&'static str>,
}

impl Page for ListPage {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

impl ListPage {
    /// The list URL with the same filters and another page or view.
    pub fn href(&self, page: impl Borrow<i64>, table: impl Borrow<bool>) -> String {
        let (page, table) = (page.borrow(), table.borrow());
        let mut s = format!("/registrations?event={}", urlencode(self.event_uuid.as_deref().unwrap_or("todos")));
        if let Some((st, _)) = self.statuses.iter().find(|(_, on)| *on) {
            s.push_str(&format!("&status={}", urlencode(st)));
        }
        if let Some((ti, _)) = self.tipos.iter().find(|(_, on)| *on) {
            s.push_str(&format!("&tipo={}", urlencode(ti)));
        }
        if !self.q.is_empty() {
            s.push_str(&format!("&q={}", urlencode(&self.q)));
        }
        if !*table {
            s.push_str("&view=list");
        }
        if *page > 1 {
            s.push_str(&format!("&page={page}"));
        }
        s
    }
}

/// The default event of the list: the first Activo event by start date (06 UI-05).
fn default_event(conn: &Connection) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT uuid FROM events WHERE status = 'Activo' ORDER BY date_start, name LIMIT 1", [], |r| {
        r.get(0)
    })
    .optional()
}

async fn list(
    State(state): State<AppState>,
    RequireRole { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
    Query(q): Query<ListQuery>,
) -> Result<Html<String>, AppError> {
    let conn = state.db();
    // A search with no event given (UI-04 "Ver registros": `?q=<name>`) looks in every event; a plain list opens
    // on the first active event.
    let searching = q.q.as_deref().is_some_and(|s| !s.trim().is_empty());
    let event_uuid = match q.event.as_deref() {
        Some("todos") => None,
        Some(e) if !e.is_empty() => Some(e.to_owned()),
        _ if searching => None,
        _ => default_event(&conn)?,
    };
    let status = q.status.clone().filter(|s| regs::STATUSES.contains(&s.as_str()));
    let tipo = q.tipo.clone().filter(|s| s == "Convencionista" || s == "Participante");
    let search = q.q.clone().unwrap_or_default().trim().to_owned();
    let like = (!search.is_empty()).then(|| format!("%{search}%"));
    let filter = "FROM registration r JOIN people p ON p.uuid = r.person_uuid
                  JOIN v_registration_totals v ON v.registration_uuid = r.uuid
                 WHERE (?1 IS NULL OR r.event_uuid = ?1) AND (?2 IS NULL OR r.status = ?2)
                   AND (?3 IS NULL OR v.registration_type = ?3)
                   AND (?4 IS NULL OR TRIM(COALESCE(p.name_first, '') || ' ' || COALESCE(p.name_last, '')) LIKE ?4
                        OR p.company LIKE ?4 OR CAST(r.registration_number AS TEXT) LIKE ?4)";
    let total: i64 =
        conn.query_row(&format!("SELECT COUNT(*) {filter}"), params![event_uuid, status, tipo, like], |r| r.get(0))?;
    let page_count = ((total + PAGE_SIZE - 1) / PAGE_SIZE).max(1);
    let page = q.page.unwrap_or(1).clamp(1, page_count);
    let rows = conn
        .prepare(&format!(
            "SELECT r.uuid, r.registration_number, p.name_first, p.name_last, p.company, v.registration_type, r.status,
                    v.total, v.balance {filter}
              ORDER BY r.registration_number DESC LIMIT ?5 OFFSET ?6"
        ))?
        .query_map(params![event_uuid, status, tipo, like, PAGE_SIZE, (page - 1) * PAGE_SIZE], |r| {
            let st: String = r.get(6)?;
            Ok(ListRow {
                uuid: r.get(0)?,
                number: r.get::<_, Option<i64>>(1)?.map(|n| n.to_string()).unwrap_or_default(),
                name: crate::convention::people::full_name(
                    r.get::<_, Option<String>>(2)?.as_deref(),
                    r.get::<_, Option<String>>(3)?.as_deref(),
                ),
                company: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                kind: r.get(5)?,
                tag: status_tag(&st),
                status: st,
                total: usd(&r.get(7)?),
                balance: usd(&r.get(8)?),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let events = conn
        .prepare(
            "SELECT uuid, name FROM events ORDER BY status = 'Activo' DESC,
                    CASE WHEN status = 'Activo' THEN date_start END, date_start DESC, name",
        )?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let event_name = event_uuid
        .as_ref()
        .and_then(|e| events.iter().find(|(u, _)| u == e).map(|(_, n)| n.clone()))
        .unwrap_or_else(|| "Todos los eventos".into());
    let mut events: Vec<(String, String, bool)> =
        events.into_iter().map(|(u, n)| (u.clone(), n, Some(&u) == event_uuid.as_ref())).collect();
    events.push(("todos".into(), "Todos los eventos".into(), event_uuid.is_none()));
    let first = if total == 0 { 0 } else { (page - 1) * PAGE_SIZE + 1 };
    let last = ((page - 1) * PAGE_SIZE + rows.len() as i64).max(0);
    let lo = (page - 2).max(1);
    let hi = (lo + 4).min(page_count);
    let mut shell = Shell::new(&user, "convencion", "registros");
    shell.autosave = false;
    let page = ListPage {
        shell,
        rows,
        events,
        statuses: regs::STATUSES.iter().map(|s| (s.to_string(), status.as_deref() == Some(*s))).collect(),
        tipos: ["Convencionista", "Participante"]
            .iter()
            .map(|s| (s.to_string(), tipo.as_deref() == Some(*s)))
            .collect(),
        q: search,
        event_uuid,
        event_name,
        table_view: q.view.as_deref() != Some("list"),
        total,
        first,
        last,
        page,
        pages: (lo..=hi).collect(),
        page_count,
        notice: match q.hecho.as_deref() {
            Some("borrado") => Some("Registro borrado."),
            _ => None,
        },
    };
    drop(conn);
    render_page(&headers, &page)
}

// ---------------------------------------------------------------------------------------------------------------
// UI-06 Registro: view pieces
// ---------------------------------------------------------------------------------------------------------------

pub struct Header {
    pub uuid: String,
    pub number: String,
    pub status: String,
    pub costeado_por: String,
    pub comments: String,
    pub badges: String,
    pub event_uuid: Option<String>,
    pub package_uuid: Option<String>,
    pub person_uuid: String,
    pub company: String,
    pub first: String,
    pub last: String,
    pub badge: String,
    pub phone: String,
    pub fax: String,
    pub address: String,
    pub city: String,
    pub state: String,
    pub zip: String,
    pub email: String,
}

impl Header {
    pub fn name(&self) -> String {
        crate::convention::people::full_name(Some(&self.first), Some(&self.last))
    }
}

fn header(conn: &Connection, uuid: &str) -> Result<Header, AppError> {
    let s = |v: Option<String>| v.unwrap_or_default();
    conn.query_row(
        "SELECT r.uuid, r.registration_number, r.status, r.costeado_por, r.comments, r.guest_name_badges, r.event_uuid,
                r.package_uuid, p.uuid, p.company, p.name_first, p.name_last, p.name_badge, p.phone1, p.fax, p.address,
                p.city, p.state, p.zip, p.email
           FROM registration r JOIN people p ON p.uuid = r.person_uuid WHERE r.uuid = ?1",
        [uuid],
        |r| {
            Ok(Header {
                uuid: r.get(0)?,
                number: r.get::<_, Option<i64>>(1)?.map(|n| n.to_string()).unwrap_or_default(),
                status: r.get(2)?,
                costeado_por: s(r.get(3)?),
                comments: s(r.get(4)?),
                badges: s(r.get(5)?),
                event_uuid: r.get(6)?,
                package_uuid: r.get(7)?,
                person_uuid: r.get(8)?,
                company: s(r.get(9)?),
                first: s(r.get(10)?),
                last: s(r.get(11)?),
                badge: s(r.get(12)?),
                phone: s(r.get(13)?),
                fax: s(r.get(14)?),
                address: s(r.get(15)?),
                city: s(r.get(16)?),
                state: s(r.get(17)?),
                zip: s(r.get(18)?),
                email: s(r.get(19)?),
            })
        },
    )
    .optional()?
    .ok_or(AppError::NotFound)
}

/// One items-portal line (`<tbody class="line">`, design/registro.html "Detalles del registro").
#[derive(Template)]
#[template(path = "convention/registrations/line.html")]
pub struct LineView {
    pub url: String,
    pub uuid: String,
    pub kind: String,
    pub name: String,
    pub plan: String,
    pub d_start: String,
    pub t_start: String,
    pub d_end: String,
    pub t_end: String,
    pub details: String,
    pub details_muted: bool,
    pub price: String,
    pub qty: String,
    pub total: String,
    pub no_charge: bool,
    pub has_attr: bool,
    /// After a refused PATCH: (field, typed value, message).
    pub error: Option<(String, String, String)>,
}

impl LineView {
    fn new(reg_uuid: &str, l: &Line, plan: &str) -> Self {
        let kind = regs::attr_kind(&l.event_type);
        let (details, muted) = match kind {
            AttrKind::Hotel => match (&l.hotel_reservation_num, &l.adicional) {
                (Some(r), Some(a)) => (format!("Reserva {r} · {a} noches adic."), false),
                (Some(r), None) => (format!("Reserva {r}"), false),
                (None, Some(a)) => (format!("{a} noches adic."), false),
                (None, None) => ("Sin reserva".to_owned(), true),
            },
            AttrKind::Seating => match &l.mesa {
                Some(m) => (format!("Mesa #{m}"), false),
                None => ("Mesa sin asignar".to_owned(), true),
            },
            AttrKind::None => ("—".to_owned(), true),
        };
        Self {
            url: format!("/registrations/{reg_uuid}/items/{}", l.uuid),
            uuid: l.uuid.clone(),
            kind: l.event_type.clone(),
            name: l.event_item_name.clone(),
            plan: if l.from_package { plan_code(plan) } else { String::new() },
            d_start: d(&l.date_start),
            t_start: t(&l.time_start),
            d_end: d(&l.date_end),
            t_end: t(&l.time_end),
            details,
            details_muted: muted,
            price: autosave::dollars(l.price),
            qty: l.ticket_qty.to_string(),
            total: usd(&l.total),
            no_charge: l.no_charge,
            has_attr: kind != AttrKind::None,
            error: None,
        }
    }

    /// What the control of `field` shows: the typed value after a refusal, else the saved value.
    pub fn shown(&self, field: &str) -> String {
        match &self.error {
            Some((f, typed, _)) if f == field => typed.clone(),
            _ => match field {
                "price" => self.price.clone(),
                _ => self.qty.clone(),
            },
        }
    }

    pub fn bad(&self, field: &str) -> bool {
        matches!(&self.error, Some((f, _, _)) if f == field)
    }
}

/// The attributes dialog of one item (BR-REG-06): Cancel closes it and sends nothing; Aceptar posts the values.
#[derive(Template)]
#[template(path = "convention/registrations/attributes.html")]
pub struct AttrDialog {
    pub action: String,
    pub uuid: String,
    pub name: String,
    pub hotel: bool,
    pub reserva: String,
    pub adicional: String,
    pub mesa: String,
    pub csrf: String,
    pub error: Option<String>,
}

fn attr_dialog(
    conn: &Connection,
    reg_uuid: &str,
    item_uuid: &str,
    name: &str,
    csrf: &str,
) -> Result<Option<AttrDialog>, AppError> {
    let a = regs::item_attributes(conn, reg_uuid, item_uuid).map_err(domain)?;
    if a.kind == AttrKind::None {
        return Ok(None);
    }
    Ok(Some(AttrDialog {
        action: format!("/registrations/{reg_uuid}/items/{item_uuid}/attributes"),
        uuid: item_uuid.to_owned(),
        name: name.to_owned(),
        hotel: a.kind == AttrKind::Hotel,
        reserva: a.hotel_reservation_num.unwrap_or_default(),
        adicional: a.adicional.unwrap_or_default(),
        mesa: a.mesa.unwrap_or_default(),
        csrf: csrf.to_owned(),
        error: None,
    }))
}

pub struct GuestView {
    pub uuid: String,
    pub name: String,
    pub first: FieldView,
    pub last: FieldView,
    pub badge: FieldView,
}

fn guest_field(reg: &str, g: &str, name: &str, label: &str, saved: &str) -> FieldView {
    FieldView::new(format!("/registrations/{reg}/guests/{g}"), name, &format!("g-{g}-{name}"), Control::Text, saved)
        .aria(label)
}

fn guests(conn: &Connection, reg: &str) -> Result<Vec<GuestView>, AppError> {
    let rows = conn
        .prepare(
            "SELECT uuid, name_first, name_last, name_badge FROM registration_guests
              WHERE registration_uuid = ?1 ORDER BY created_at, rowid",
        )?
        .query_map([reg], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                r.get::<_, Option<String>>(3)?.unwrap_or_default(),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .map(|(u, f, l, b)| GuestView {
            name: crate::convention::people::full_name(Some(&f), Some(&l)),
            first: guest_field(reg, &u, "name_first", "Nombre", &f),
            last: guest_field(reg, &u, "name_last", "Apellido", &l),
            badge: guest_field(reg, &u, "name_badge", "Nombre badge", &b),
            uuid: u,
        })
        .collect())
}

pub struct PayView {
    pub uuid: String,
    pub date: String,
    pub number: String,
    pub kind: String,
    pub amount: String,
    pub voided: bool,
    pub reason: String,
}

fn payments(conn: &Connection, reg: &str) -> Result<Vec<PayView>, AppError> {
    use chrono::Datelike;
    Ok(conn
        .prepare(
            "SELECT uuid, created_at, payment_number, payment_type, payment, voided_at, void_reason FROM payments
              WHERE registration_uuid = ?1 ORDER BY created_at DESC, rowid DESC",
        )?
        .query_map([reg], |r| {
            let at: String = r.get(1)?;
            Ok(PayView {
                uuid: r.get(0)?,
                date: crate::clock::parse(&at)
                    .map(|t| format!("{}/{:02}/{}", t.month(), t.day(), t.year()))
                    .unwrap_or(at),
                number: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                kind: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                amount: usd(&r.get(4)?),
                voided: r.get::<_, Option<String>>(5)?.is_some(),
                reason: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

pub struct EventRow {
    pub uuid: String,
    pub name: String,
    pub location: String,
    pub start: String,
    pub end: String,
}

pub struct PlanRow {
    pub uuid: String,
    pub name: String,
    pub items: i64,
    pub price: String,
    pub current: bool,
}

/// The payment dialog after a refusal: typed values and messages.
#[derive(Default)]
pub struct PayForm {
    pub amount: String,
    pub kind: String,
    pub number: String,
    pub amount_err: Option<String>,
    pub kind_err: Option<String>,
}

/// Page messages and the dialog to open on load.
#[derive(Default)]
pub struct Extras {
    pub notice: Option<String>,
    pub error: Option<String>,
    pub open: Option<String>,
    pub pay: PayForm,
    /// (payment uuid, message) of a refused void.
    pub void_err: Option<(String, String)>,
    /// (item uuid, message) of refused attributes.
    pub attr_err: Option<(String, String)>,
}

#[derive(Template)]
#[template(path = "convention/registrations/registro.html", blocks = ["content"])]
pub struct RegPage {
    pub shell: Shell,
    pub h: Header,
    pub kind: String,
    pub event: Option<EventRow>,
    pub plan: Option<(String, String, String)>,
    pub plan_items: usize,
    pub totals: Totals,
    pub totals_html: String,
    pub lines: Vec<LineView>,
    pub attr_dialogs: Vec<String>,
    pub guests: Vec<GuestView>,
    pub payments: Vec<PayView>,
    pub add_items: Vec<(String, String)>,
    pub active_events: Vec<EventRow>,
    pub plans: Vec<PlanRow>,
    pub status: FieldView,
    pub badges: FieldView,
    pub comments: FieldView,
    pub costeado: Radios,
    pub payment_types: Vec<&'static str>,
    pub payments_count: usize,
    pub x: Extras,
}

impl Page for RegPage {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

impl RegPage {
    pub fn usd(&self, c: impl Borrow<i64>) -> String {
        usd(c.borrow())
    }
    pub fn line_html(&self, l: &LineView) -> String {
        l.render().unwrap_or_default()
    }
    pub fn event_picker_html(&self) -> String {
        let events = self
            .active_events
            .iter()
            .map(|e| EventRow {
                uuid: e.uuid.clone(),
                name: e.name.clone(),
                location: e.location.clone(),
                start: e.start.clone(),
                end: e.end.clone(),
            })
            .collect();
        EventPicker { action: format!("/registrations/{}/event", self.h.uuid), csrf: self.shell.csrf.clone(), events }
            .render()
            .unwrap_or_default()
    }
    pub fn plan_picker_html(&self) -> String {
        PlanPicker { page: self }.render().unwrap_or_default()
    }
    /// The payment dialog's default amount: the balance when something is due.
    pub fn balance_text(&self) -> String {
        if self.totals.balance > 0 {
            autosave::dollars(self.totals.balance)
        } else {
            String::new()
        }
    }
    pub fn void_error(&self, p: impl AsRef<str>) -> Option<String> {
        self.x.void_err.as_ref().filter(|(u, _)| u == p.as_ref()).map(|(_, m)| m.clone())
    }

    fn build(conn: &Connection, user: &CurrentUser, uuid: &str, x: Extras) -> Result<Self, AppError> {
        let h = header(conn, uuid)?;
        let totals = regs::totals(conn, uuid).map_err(domain)?;
        let totals_html = TotalsBlock { t: totals.clone() }.render()?;
        let event = match &h.event_uuid {
            Some(e) => conn
                .query_row("SELECT uuid, name, location, date_start, date_end FROM events WHERE uuid = ?1", [e], |r| {
                    let (s, en): (Option<String>, Option<String>) = (r.get(3)?, r.get(4)?);
                    Ok(EventRow {
                        uuid: r.get(0)?,
                        name: r.get(1)?,
                        location: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                        start: date_range(&s, &en),
                        end: d(&en),
                    })
                })
                .optional()?,
            None => None,
        };
        let plan = match &h.package_uuid {
            Some(p) => {
                let name: String = conn.query_row("SELECT name FROM packages WHERE uuid = ?1", [p], |r| r.get(0))?;
                let price = plans::plan_price(conn, p).map_err(domain)?;
                Some((p.clone(), name, usd(&price)))
            }
            None => None,
        };
        let plan_name = plan.as_ref().map(|p| p.1.clone()).unwrap_or_default();
        let raw_lines = regs::lines(conn, uuid)?;
        let plan_items = raw_lines.iter().filter(|l| l.from_package).count();
        let mut attr_dialogs = Vec::new();
        for l in &raw_lines {
            if let Some(mut dlg) = attr_dialog(conn, uuid, &l.uuid, &l.event_item_name, &user.csrf)? {
                if let Some((i, m)) = &x.attr_err {
                    if *i == l.uuid {
                        dlg.error = Some(m.clone());
                    }
                }
                attr_dialogs.push(dlg.render()?);
            }
        }
        let lines = raw_lines.iter().map(|l| LineView::new(uuid, l, &plan_name)).collect();
        let add_items = match &h.event_uuid {
            Some(e) => conn
                .prepare(
                    "SELECT uuid, type, name, date_start, time_start FROM event_items WHERE event_uuid = ?1
                      ORDER BY date_start, time_start, sort_order, name",
                )?
                .query_map([e], |r| {
                    let (ds, ts): (Option<String>, Option<String>) = (r.get(3)?, r.get(4)?);
                    let when = format!("{} {}", d(&ds), t(&ts)).trim().to_owned();
                    Ok((
                        r.get::<_, String>(0)?,
                        format!("{} · {} · {}", r.get::<_, String>(1)?, r.get::<_, String>(2)?, when),
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?,
            None => Vec::new(),
        };
        let active_events = if h.event_uuid.is_none() {
            conn.prepare(
                "SELECT uuid, name, location, date_start, date_end FROM events WHERE status = 'Activo'
                  ORDER BY date_start, name",
            )?
            .query_map([], |r| {
                Ok(EventRow {
                    uuid: r.get(0)?,
                    name: r.get(1)?,
                    location: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    start: d(&r.get(3)?),
                    end: d(&r.get(4)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
        } else {
            Vec::new()
        };
        let plans = match &h.event_uuid {
            Some(e) => plans::plans_of_event(conn, e)?
                .into_iter()
                .map(|(u, name, price)| {
                    let items: i64 = conn
                        .query_row(
                            "SELECT COUNT(*) FROM package_items WHERE package_uuid = ?1 AND active = 1",
                            [&u],
                            |r| r.get(0),
                        )
                        .unwrap_or(0);
                    PlanRow {
                        current: h.package_uuid.as_deref() == Some(u.as_str()),
                        uuid: u,
                        name,
                        items,
                        price: usd(&price),
                    }
                })
                .collect(),
            None => Vec::new(),
        };
        let url = reg_url(uuid);
        let status_opts = regs::STATUSES.iter().map(|s| (s.to_string(), s.to_string())).collect();
        let status = FieldView::new(&url, "status", "r-st", Control::Select(status_opts), &h.status);
        let badges = FieldView::new(&url, "guest_name_badges", "r-badges", Control::Text, &h.badges);
        let comments = FieldView::new(&url, "comments", "r-com", Control::Textarea(5), &h.comments);
        let costeado = Radios::new(&url, &h.costeado_por);
        let mut shell = Shell::new(user, "convencion", "registros");
        shell.autosave = true;
        let payments = payments(conn, uuid)?;
        Ok(Self {
            shell,
            kind: totals.registration_type.clone(),
            guests: guests(conn, uuid)?,
            payments_count: payments.len(),
            payments,
            event,
            plan,
            plan_items,
            totals,
            totals_html,
            lines,
            attr_dialogs,
            add_items,
            active_events,
            plans,
            status,
            badges,
            comments,
            costeado,
            payment_types: regs::PAYMENT_TYPES.to_vec(),
            h,
            x,
        })
    }
}

/// "Costeado por": two radios that autosave (design/registro.html).
#[derive(Template, Clone)]
#[template(path = "convention/registrations/radios.html")]
pub struct Radios {
    pub url: String,
    pub saved: String,
    pub error: Option<String>,
}

impl Radios {
    fn new(url: &str, saved: &str) -> Self {
        Self { url: url.to_owned(), saved: saved.to_owned(), error: None }
    }
    pub fn html(&self) -> String {
        self.render().unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------------------------------------------
// UI-06: the page and its writes
// ---------------------------------------------------------------------------------------------------------------

#[derive(Deserialize)]
struct PageQuery {
    hecho: Option<String>,
    n: Option<i64>,
}

fn notice(code: Option<&str>, n: Option<i64>) -> Option<String> {
    Some(match code? {
        "evento" => "Evento escogido. Ahora escoja el plan.".into(),
        "plan" => format!(
            "Plan escogido: {} actividad{} nueva{}.",
            n.unwrap_or(0),
            if n == Some(1) { "" } else { "es" },
            if n == Some(1) { "" } else { "s" }
        ),
        "actividad" => "Actividad añadida.".into(),
        "quitada" => "Actividad quitada.".into(),
        "atributos" => "Atributos guardados.".into(),
        "acompanante" => "Acompañante añadido; cantidades recalculadas.".into(),
        "acompanante-quitado" => "Acompañante quitado; cantidades recalculadas.".into(),
        "pago" => "Pago registrado.".into(),
        "anulado" => "Pago anulado.".into(),
        "cancelado" => "Registro cancelado (Canceló).".into(),
        _ => return None,
    })
}

async fn page(
    State(state): State<AppState>,
    RequireRole { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<PageQuery>,
) -> Result<Html<String>, AppError> {
    let conn = state.db();
    let uuid = resolve(&conn, &id)?;
    let x = Extras { notice: notice(q.hecho.as_deref(), q.n), ..Default::default() };
    let page = RegPage::build(&conn, &user, &uuid, x)?;
    drop(conn);
    render_page(&headers, &page)
}

/// The page again with a message and a dialog open, for a refused dialog form.
fn again(
    state: &AppState,
    user: &CurrentUser,
    headers: &HeaderMap,
    uuid: &str,
    status: StatusCode,
    x: Extras,
) -> Response {
    let built = RegPage::build(&state.db(), user, uuid, x);
    match built {
        Ok(p) => render_page_status(status, headers, &p),
        Err(e) => e.into_response(),
    }
}

/// A refused command of a dialog form: 403/404 pages, else the page with the message (409/422).
fn refused(
    state: &AppState,
    user: &CurrentUser,
    headers: &HeaderMap,
    uuid: &str,
    e: DomainError,
    open: Option<String>,
) -> Response {
    match e {
        DomainError::Forbidden | DomainError::NotFound | DomainError::Db(_) => e.into_response(),
        e => {
            let status = e.status();
            again(state, user, headers, uuid, status, Extras { error: Some(e.message()), open, ..Default::default() })
        }
    }
}

fn done(uuid: &str, code: &str) -> Response {
    Redirect::to(&format!("/registrations/{uuid}?hecho={code}")).into_response()
}

/// `PATCH /registrations/{id}`: status, costeado por, comments, guest badge text (one field).
async fn patch_header(
    State(state): State<AppState>,
    RequireRole { 0: user, .. }: RequireStaff,
    r: Rid,
    Path(id): Path<String>,
    field: OneField,
) -> Result<Response, AppError> {
    let who = Who::of(&user);
    let mut conn = state.db();
    let uuid = resolve(&conn, &id)?;
    let out = registries::patch(
        &mut conn,
        &who,
        rid(&r),
        Target::new(Resource::Registration, &uuid),
        &field.name,
        &field.value,
    )?;
    let Patched::Done(out) = out else {
        return Ok(autosave::contract_error("Este campo no se puede cambiar aquí."));
    };
    let h = header(&conn, &uuid)?;
    let url = reg_url(&uuid);
    let err = match out {
        Outcome::Invalid(m) => Some(m),
        Outcome::Saved { .. } => None,
    };
    if field.name == "costeado_por" {
        let mut rd = Radios::new(&url, &h.costeado_por);
        let status = if err.is_some() { StatusCode::UNPROCESSABLE_ENTITY } else { StatusCode::OK };
        rd.error = err;
        return Ok((status, Html(rd.html())).into_response());
    }
    let view = match field.name.as_str() {
        "status" => FieldView::new(
            &url,
            "status",
            "r-st",
            Control::Select(regs::STATUSES.iter().map(|s| (s.to_string(), s.to_string())).collect()),
            &h.status,
        ),
        "comments" => FieldView::new(&url, "comments", "r-com", Control::Textarea(5), &h.comments),
        _ => FieldView::new(&url, "guest_name_badges", "r-badges", Control::Text, &h.badges),
    };
    let view = match err {
        Some(m) => view.refused(&field.value, m),
        None => view,
    };
    Ok(autosave::respond(&view, ""))
}

#[derive(Deserialize)]
struct ConfirmForm {
    #[serde(default)]
    confirm: Option<String>,
}

fn confirmed(c: &Option<String>) -> bool {
    matches!(c.as_deref(), Some("1" | "on" | "true"))
}

/// `DELETE /registrations/{id}` (OP-REG-09, AP-05): payments -> 409 with the cancel offer; staff -> 403.
async fn delete_registration(
    State(state): State<AppState>,
    RequireRole { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let who = Who::of(&user);
    let uuid = resolve(&state.db(), &id)?;
    let res = {
        let mut conn = state.db();
        let tx = begin(&mut conn)?;
        let r = regs::delete_registration(&tx, &who, &uuid);
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    Ok(match res {
        Ok(()) => {
            if is_htmx(&headers) {
                let mut res = StatusCode::OK.into_response();
                res.headers_mut().insert("hx-redirect", "/registrations?hecho=borrado".parse().unwrap());
                res
            } else {
                Redirect::to("/registrations?hecho=borrado").into_response()
            }
        }
        Err(e @ DomainError::HasPayments(_)) => refused(&state, &user, &headers, &uuid, e, Some("dlg-cancelar".into())),
        Err(e) => refused(&state, &user, &headers, &uuid, e, None),
    })
}

/// `POST /registrations/{id}/cancel` (OP-REG-09): status Canceló, both roles.
async fn cancel(
    State(state): State<AppState>,
    RequireRole { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let who = Who::of(&user);
    let uuid = resolve(&state.db(), &id)?;
    let res = {
        let mut conn = state.db();
        let tx = begin(&mut conn)?;
        let r = regs::cancel_registration(&tx, &who, &uuid);
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    Ok(match res {
        Ok(()) => done(&uuid, "cancelado"),
        Err(e) => refused(&state, &user, &headers, &uuid, e, None),
    })
}

/// The event picker dialog (BR-REG-01). Shared by the page and `GET .../event-picker`.
#[derive(Template)]
#[template(path = "convention/registrations/event_picker.html")]
pub struct EventPicker {
    pub action: String,
    pub csrf: String,
    pub events: Vec<EventRow>,
}

/// `GET /registrations/{id}/event-picker`: the dialog of active events; 409 when the event is set or none is active.
async fn event_picker(
    State(state): State<AppState>,
    RequireRole { 0: user, .. }: RequireStaff,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let conn = state.db();
    let uuid = resolve(&conn, &id)?;
    match regs::event_picker(&conn, &uuid) {
        Ok(_) => {
            let page = RegPage::build(&conn, &user, &uuid, Extras::default())?;
            let html = EventPicker {
                action: format!("/registrations/{uuid}/event"),
                csrf: user.csrf.clone(),
                events: page.active_events,
            }
            .render()?;
            Ok(Html(html).into_response())
        }
        Err(e) => Ok(e.into_response()),
    }
}

#[derive(Deserialize)]
struct EventForm {
    event_uuid: String,
}

/// `POST /registrations/{id}/event` (OP-REG-02).
async fn select_event(
    State(state): State<AppState>,
    RequireRole { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(f): Form<EventForm>,
) -> Result<Response, AppError> {
    let who = Who::of(&user);
    let uuid = resolve(&state.db(), &id)?;
    let res = {
        let mut conn = state.db();
        let tx = begin(&mut conn)?;
        let r = regs::select_event(&tx, &who, &uuid, &f.event_uuid);
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    Ok(match res {
        Ok(()) => done(&uuid, "evento"),
        Err(e) => refused(&state, &user, &headers, &uuid, e, None),
    })
}

/// `GET /registrations/{id}/plan-picker`: the plans of the registration's event.
async fn plan_picker(
    State(state): State<AppState>,
    RequireRole { 0: user, .. }: RequireStaff,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let conn = state.db();
    let uuid = resolve(&conn, &id)?;
    let page = RegPage::build(&conn, &user, &uuid, Extras::default())?;
    if page.event.is_none() {
        return Ok(DomainError::NoEvent.into_response());
    }
    Ok(Html(PlanPicker { page: &page }.render()?).into_response())
}

#[derive(Template)]
#[template(path = "convention/registrations/plan_picker.html")]
pub struct PlanPicker<'a> {
    pub page: &'a RegPage,
}

#[derive(Deserialize)]
struct PlanForm {
    plan_uuid: String,
    #[serde(default)]
    confirm: Option<String>,
}

/// `POST /registrations/{id}/plan` (OP-REG-03, BR-REG-02). Without `confirm` while a plan is set: 200 with the
/// confirm dialog open, nothing written (T-05-03).
async fn select_plan(
    State(state): State<AppState>,
    RequireRole { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(f): Form<PlanForm>,
) -> Result<Response, AppError> {
    let who = Who::of(&user);
    let uuid = resolve(&state.db(), &id)?;
    let res = {
        let mut conn = state.db();
        let tx = begin(&mut conn)?;
        let r = regs::select_plan(&tx, &who, &uuid, &f.plan_uuid, confirmed(&f.confirm));
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    Ok(match res {
        Ok(n) => Redirect::to(&format!("/registrations/{uuid}?hecho=plan&n={n}")).into_response(),
        Err(DomainError::ConfirmRequired) => again(
            &state,
            &user,
            &headers,
            &uuid,
            StatusCode::OK,
            Extras { open: Some(format!("dlg-plan-confirm-{}", f.plan_uuid)), ..Default::default() },
        ),
        Err(e) => refused(&state, &user, &headers, &uuid, e, None),
    })
}

#[derive(Deserialize)]
struct ItemForm {
    event_item_uuid: String,
}

/// `POST /registrations/{id}/items` (OP-REG-04): one event item of the same event.
async fn add_item(
    State(state): State<AppState>,
    RequireRole { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(f): Form<ItemForm>,
) -> Result<Response, AppError> {
    let who = Who::of(&user);
    let uuid = resolve(&state.db(), &id)?;
    let res = {
        let mut conn = state.db();
        let tx = begin(&mut conn)?;
        let r = regs::add_item(&tx, &who, &uuid, &f.event_item_uuid);
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    Ok(match res {
        Ok(_) => done(&uuid, "actividad"),
        Err(e) => refused(&state, &user, &headers, &uuid, e, None),
    })
}

/// `DELETE /registrations/{id}/items/{item}` (OP-REG-04, AP-06): two-step confirm.
async fn remove_item(
    State(state): State<AppState>,
    RequireRole { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
    Path((id, item)): Path<(String, String)>,
    Form(f): Form<ConfirmForm>,
) -> Result<Response, AppError> {
    let who = Who::of(&user);
    let uuid = resolve(&state.db(), &id)?;
    let res = {
        let mut conn = state.db();
        let tx = begin(&mut conn)?;
        let r = regs::remove_item(&tx, &who, &uuid, &item, confirmed(&f.confirm));
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    Ok(match res {
        Ok(()) => done(&uuid, "quitada"),
        Err(DomainError::ConfirmRequired) => again(
            &state,
            &user,
            &headers,
            &uuid,
            StatusCode::OK,
            Extras { open: Some(format!("dlg-quitar-{item}")), ..Default::default() },
        ),
        Err(e) => refused(&state, &user, &headers, &uuid, e, None),
    })
}

fn line_view(conn: &Connection, reg: &str, item: &str) -> Result<LineView, AppError> {
    let plan_name: String = conn
        .query_row(
            "SELECT COALESCE(p.name, '') FROM registration r LEFT JOIN packages p ON p.uuid = r.package_uuid WHERE r.uuid = ?1",
            [reg],
            |r| r.get(0),
        )
        .unwrap_or_default();
    let l = regs::lines(conn, reg)?.into_iter().find(|l| l.uuid == item).ok_or(AppError::NotFound)?;
    Ok(LineView::new(reg, &l, &plan_name))
}

/// `PATCH /registrations/{id}/items/{item}` (OP-REG-05): price, qty (sets `qty_manual`, D-88), no charge or an
/// attribute. 200: the line and `#totales` out of band (T-05-13). 422: the line with the typed value and the
/// message; nothing written (T-05-14).
async fn patch_item(
    State(state): State<AppState>,
    RequireRole { 0: user, .. }: RequireStaff,
    r: Rid,
    Path((id, item)): Path<(String, String)>,
    field: OneField,
) -> Result<Response, AppError> {
    let who = Who::of(&user);
    let mut conn = state.db();
    let uuid = resolve(&conn, &id)?;
    let out = registries::patch(
        &mut conn,
        &who,
        rid(&r),
        Target::child(Resource::Item, &item, &uuid),
        &field.name,
        &field.value,
    )?;
    let Patched::Done(out) = out else {
        return Ok(autosave::contract_error("Este campo no se puede cambiar aquí."));
    };
    let mut view = line_view(&conn, &uuid, &item)?;
    match out {
        Outcome::Saved { .. } => {
            let mut html = view.render()?;
            html.push_str(&totales_oob(&conn, &uuid).map_err(domain)?);
            Ok(Html(html).into_response())
        }
        Outcome::Invalid(m) => {
            let label = match field.name.as_str() {
                "price" => "Precio",
                "qty" => "Qty.",
                "no_charge" => "Sin cargo",
                _ => "Atributo",
            };
            let saved = view.shown(&field.name);
            let msg = format!(
                "{label} de {}: {m} No se guardó. Corrija el valor o pulse Esc para volver a {saved}.",
                view.name
            );
            view.error = Some((field.name.clone(), field.value.clone(), msg));
            Ok((StatusCode::UNPROCESSABLE_ENTITY, Html(view.render()?)).into_response())
        }
    }
}

/// `GET /registrations/{id}/items/{item}/attributes`: the dialog (BR-REG-06). Read only; 404 for an item without
/// attributes.
async fn attributes(
    State(state): State<AppState>,
    RequireRole { 0: user, .. }: RequireStaff,
    Path((id, item)): Path<(String, String)>,
) -> Result<Response, AppError> {
    let conn = state.db();
    let uuid = resolve(&conn, &id)?;
    let name: String = conn
        .query_row(
            "SELECT ei.name FROM registration_items ri JOIN event_items ei ON ei.uuid = ri.event_item_uuid
              WHERE ri.uuid = ?1 AND ri.registration_uuid = ?2",
            [&item, &uuid],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    let dlg = attr_dialog(&conn, &uuid, &item, &name, &user.csrf)?.ok_or(AppError::NotFound)?;
    Ok(Html(dlg.render()?).into_response())
}

#[derive(Deserialize)]
struct AttrForm {
    hotel_reservation_num: Option<String>,
    adicional: Option<String>,
    mesa: Option<String>,
}

/// `POST /registrations/{id}/items/{item}/attributes` (OP-REG-05): Aceptar of the dialog. Each value goes through
/// the item registry (BR-REG-06 type check, one audit row per changed field). Cancel never calls this.
async fn save_attributes(
    State(state): State<AppState>,
    RequireRole { 0: user, .. }: RequireStaff,
    r: Rid,
    headers: HeaderMap,
    Path((id, item)): Path<(String, String)>,
    Form(f): Form<AttrForm>,
) -> Result<Response, AppError> {
    let who = Who::of(&user);
    let uuid = resolve(&state.db(), &id)?;
    let fields = [("hotel_reservation_num", f.hotel_reservation_num), ("adicional", f.adicional), ("mesa", f.mesa)];
    for (name, value) in fields {
        let Some(value) = value else { continue };
        let out = {
            let mut conn = state.db();
            registries::patch(&mut conn, &who, rid(&r), Target::child(Resource::Item, &item, &uuid), name, &value)?
        };
        if let Patched::Done(Outcome::Invalid(m)) = out {
            let x = Extras {
                open: Some(format!("dlg-attr-{item}")),
                attr_err: Some((item.clone(), m)),
                ..Default::default()
            };
            return Ok(again(&state, &user, &headers, &uuid, StatusCode::UNPROCESSABLE_ENTITY, x));
        }
    }
    Ok(done(&uuid, "atributos"))
}

#[derive(Deserialize)]
struct GuestForm {
    #[serde(default)]
    name_first: String,
    #[serde(default)]
    name_last: String,
    #[serde(default)]
    name_badge: String,
}

/// `POST /registrations/{id}/guests` (OP-REG-06): add, then BR-REG-03 in the same transaction.
async fn add_guest(
    State(state): State<AppState>,
    RequireRole { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(f): Form<GuestForm>,
) -> Result<Response, AppError> {
    let who = Who::of(&user);
    let uuid = resolve(&state.db(), &id)?;
    let g = regs::NewGuest {
        name_first: f.name_first,
        name_last: f.name_last,
        name_badge: f.name_badge,
        company: String::new(),
    };
    let res = {
        let mut conn = state.db();
        let tx = begin(&mut conn)?;
        let r = regs::add_guest(&tx, &who, &uuid, &g);
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    Ok(match res {
        Ok(_) => done(&uuid, "acompanante"),
        Err(e) => refused(&state, &user, &headers, &uuid, e, None),
    })
}

/// `PATCH /registrations/{id}/guests/{guest}` (OP-REG-06): one guest field; the registry reruns BR-REG-03.
async fn patch_guest(
    State(state): State<AppState>,
    RequireRole { 0: user, .. }: RequireStaff,
    r: Rid,
    Path((id, guest)): Path<(String, String)>,
    field: OneField,
) -> Result<Response, AppError> {
    let who = Who::of(&user);
    let mut conn = state.db();
    let uuid = resolve(&conn, &id)?;
    let out = registries::patch(
        &mut conn,
        &who,
        rid(&r),
        Target::child(Resource::Guest, &guest, &uuid),
        &field.name,
        &field.value,
    )?;
    let Patched::Done(out) = out else {
        return Ok(autosave::contract_error("Este campo no se puede cambiar aquí."));
    };
    let def = registries::GUEST.get(&field.name).ok_or(AppError::NotFound)?;
    let saved: Value =
        conn.query_row(&format!("SELECT {} FROM registration_guests WHERE uuid = ?1", def.column), [&guest], |r| {
            r.get(0)
        })?;
    let label = match field.name.as_str() {
        "name_first" => "Nombre",
        "name_last" => "Apellido",
        "name_badge" => "Nombre badge",
        _ => "Compañía",
    };
    let view = guest_field(&uuid, &guest, &field.name, label, &def.display(&saved));
    let view = match out {
        Outcome::Invalid(m) => view.refused(&field.value, m),
        Outcome::Saved { .. } => view,
    };
    Ok(autosave::respond(&view, ""))
}

/// `DELETE /registrations/{id}/guests/{guest}` (OP-REG-06, AP-06), then BR-REG-03.
async fn remove_guest(
    State(state): State<AppState>,
    RequireRole { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
    Path((id, guest)): Path<(String, String)>,
) -> Result<Response, AppError> {
    let who = Who::of(&user);
    let uuid = resolve(&state.db(), &id)?;
    let res = {
        let mut conn = state.db();
        let tx = begin(&mut conn)?;
        let r = regs::remove_guest(&tx, &who, &uuid, &guest);
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    Ok(match res {
        Ok(()) => done(&uuid, "acompanante-quitado"),
        Err(e) => refused(&state, &user, &headers, &uuid, e, None),
    })
}

#[derive(Deserialize)]
struct PayFormIn {
    #[serde(default)]
    payment: String,
    #[serde(default)]
    payment_type: String,
    #[serde(default)]
    payment_number: String,
}

/// `POST /registrations/{id}/payments` (OP-REG-07): amount > 0. A refusal reopens the dialog with the message.
async fn add_payment(
    State(state): State<AppState>,
    RequireRole { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(f): Form<PayFormIn>,
) -> Result<Response, AppError> {
    let who = Who::of(&user);
    let uuid = resolve(&state.db(), &id)?;
    let res = {
        let mut conn = state.db();
        let tx = begin(&mut conn)?;
        let r = regs::add_payment(&tx, &who, &uuid, &f.payment, &f.payment_type, &f.payment_number);
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    Ok(match res {
        Ok(_) => done(&uuid, "pago"),
        Err(DomainError::Validation(field, m)) => {
            let mut pay =
                PayForm { amount: f.payment, kind: f.payment_type, number: f.payment_number, ..Default::default() };
            if field == "payment_type" {
                pay.kind_err = Some(m);
            } else {
                pay.amount_err = Some(m);
            }
            let x = Extras { open: Some("dlg-pago".into()), pay, ..Default::default() };
            again(&state, &user, &headers, &uuid, StatusCode::UNPROCESSABLE_ENTITY, x)
        }
        Err(e) => refused(&state, &user, &headers, &uuid, e, None),
    })
}

#[derive(Deserialize)]
struct VoidForm {
    #[serde(default)]
    void_reason: String,
}

/// `POST /payments/{id}/void` (OP-REG-08, AP-05): ROLE-ADMIN with a reason. Staff -> 403; no reason -> 422.
async fn void_payment(
    State(state): State<AppState>,
    RequireRole { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(f): Form<VoidForm>,
) -> Result<Response, AppError> {
    let who = Who::of(&user);
    let reg: String = state
        .db()
        .query_row("SELECT registration_uuid FROM payments WHERE uuid = ?1", [&id], |r| r.get(0))
        .optional()?
        .ok_or(AppError::NotFound)?;
    let res = {
        let mut conn = state.db();
        let tx = begin(&mut conn)?;
        let r = regs::void_payment(&tx, &who, &id, &f.void_reason);
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    Ok(match res {
        Ok(()) => done(&reg, "anulado"),
        Err(DomainError::Validation(_, m)) => {
            let x = Extras {
                open: Some(format!("dlg-anular-{id}")),
                void_err: Some((id.clone(), m)),
                ..Default::default()
            };
            again(&state, &user, &headers, &reg, StatusCode::UNPROCESSABLE_ENTITY, x)
        }
        Err(e) => refused(&state, &user, &headers, &reg, e, None),
    })
}

// ---------------------------------------------------------------------------------------------------------------
// UI-07 Registro en puerta
// ---------------------------------------------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "convention/registrations/door_row.html")]
pub struct DoorRow {
    pub uuid: String,
    pub number: String,
    pub name: String,
    pub company: String,
    pub guests: i64,
    pub pressed: Option<&'static str>,
    pub csrf: String,
}

impl DoorRow {
    pub fn on(&self, s: &str) -> &'static str {
        if self.pressed == Some(s) {
            "true"
        } else {
            "false"
        }
    }
}

#[derive(Template)]
#[template(path = "convention/registrations/door.html", blocks = ["content"])]
pub struct DoorPage {
    pub shell: Shell,
    pub event_uuid: String,
    pub event_name: String,
    pub rows: Vec<String>,
    pub shown: usize,
    pub total: i64,
    pub attended: i64,
    pub q: String,
}

impl Page for DoorPage {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

fn door_rows(conn: &Connection, event: &str, q: &str, csrf: &str) -> rusqlite::Result<Vec<DoorRow>> {
    let like = (!q.is_empty()).then(|| format!("%{q}%"));
    conn.prepare(
        "SELECT r.uuid, r.registration_number, p.name_first, p.name_last, p.company, r.status,
                (SELECT COUNT(*) FROM registration_guests g WHERE g.registration_uuid = r.uuid)
           FROM registration r JOIN people p ON p.uuid = r.person_uuid
          WHERE r.event_uuid = ?1
            AND (?2 IS NULL OR TRIM(COALESCE(p.name_first, '') || ' ' || COALESCE(p.name_last, '')) LIKE ?2
                 OR p.company LIKE ?2 OR CAST(r.registration_number AS TEXT) LIKE ?2)
          ORDER BY r.registration_number",
    )?
    .query_map(params![event, like], |r| {
        let st: String = r.get(5)?;
        Ok(DoorRow {
            uuid: r.get(0)?,
            number: r.get::<_, Option<i64>>(1)?.map(|n| n.to_string()).unwrap_or_default(),
            name: crate::convention::people::full_name(
                r.get::<_, Option<String>>(2)?.as_deref(),
                r.get::<_, Option<String>>(3)?.as_deref(),
            ),
            company: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
            pressed: regs::door_status_of(&st),
            guests: r.get(6)?,
            csrf: csrf.to_owned(),
        })
    })?
    .collect()
}

#[derive(Deserialize)]
struct DoorQuery {
    q: Option<String>,
}

/// `GET /events/{id}/door` (UI-07). With `HX-Request` and `HX-Target: door-rows`: only the rows (search).
async fn door(
    State(state): State<AppState>,
    RequireRole { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<DoorQuery>,
) -> Result<Response, AppError> {
    let conn = state.db();
    let name: String = conn
        .query_row("SELECT name FROM events WHERE uuid = ?1", [&id], |r| r.get(0))
        .optional()?
        .ok_or(AppError::NotFound)?;
    let search = q.q.unwrap_or_default().trim().to_owned();
    let rows = door_rows(&conn, &id, &search, &user.csrf)?;
    let (total, attended): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(status = 'Asistió'), 0) FROM registration WHERE event_uuid = ?1",
        [&id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let rows = rows.iter().map(|r| r.render()).collect::<Result<Vec<_>, _>>()?;
    let page = DoorPage {
        shell: Shell::new(&user, "convencion", "convenciones"),
        event_uuid: id,
        event_name: name,
        shown: rows.len(),
        rows,
        total,
        attended,
        q: search,
    };
    drop(conn);
    if is_htmx(&headers) && headers.get("hx-target").is_some_and(|v| v == "door-rows") {
        let mut html = format!("<tbody id=\"door-rows\">{}</tbody>", page.rows.concat());
        html.push_str(&format!(
            "<span class=\"door-count\" id=\"door-count\" hx-swap-oob=\"true\">{} de {} · <span id=\"door-attended\">{}</span> asistieron</span>",
            page.shown, page.total, page.attended
        ));
        return Ok(Html(html).into_response());
    }
    Ok(render_page(&headers, &page)?.into_response())
}

#[derive(Deserialize)]
struct AttendanceForm {
    status: String,
}

/// `POST /registrations/{id}/attendance` (OP-REG-10): Asistió / No Asistió / Canceló. htmx: the row; else 303 to
/// the door list.
async fn attendance(
    State(state): State<AppState>,
    RequireRole { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(f): Form<AttendanceForm>,
) -> Result<Response, AppError> {
    let who = Who::of(&user);
    let uuid = resolve(&state.db(), &id)?;
    let res = {
        let mut conn = state.db();
        let tx = begin(&mut conn)?;
        let r = regs::set_attendance(&tx, &who, &uuid, &f.status);
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    if let Err(e) = res {
        return Ok(e.into_response());
    }
    let conn = state.db();
    let event: Option<String> =
        conn.query_row("SELECT event_uuid FROM registration WHERE uuid = ?1", [&uuid], |r| r.get(0))?;
    let event = event.unwrap_or_default();
    if !is_htmx(&headers) {
        return Ok(Redirect::to(&format!("/events/{event}/door")).into_response());
    }
    let row =
        door_rows(&conn, &event, "", &user.csrf)?.into_iter().find(|r| r.uuid == uuid).ok_or(AppError::NotFound)?;
    let attended: i64 = conn.query_row(
        "SELECT COUNT(*) FROM registration WHERE event_uuid = ?1 AND status = 'Asistió'",
        [&event],
        |r| r.get(0),
    )?;
    let mut html = row.render()?;
    html.push_str(&format!("<span id=\"door-attended\" hx-swap-oob=\"true\">{attended}</span>"));
    Ok(Html(html).into_response())
}
