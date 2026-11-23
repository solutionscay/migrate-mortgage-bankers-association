//! Phase-2 pane "events": UI-08 Convenciones (`GET /events`, `/events/new`, `POST /events`), UI-09 Convención
//! (`/events/{id}` with `?tab=items|sponsors|billing`, PATCH, DELETE, PDF, `POST /events/{id}/items`,
//! `POST /events/{id}/sponsors`, `POST /events/{id}/billing`, `DELETE /billing/{id}`), UI-10 Detalle de actividad
//! (`/event-items/{id}`), UI-11 Auspiciador (`/sponsors/{id}`).
//!
//! Domain: `convention::events`, `convention::sponsors`, `convention::registries::{patch, Resource::{Event,
//! EventItem, Sponsor}}`, files through `sys::files::{store, FileField}`. Templates: `templates/convention/events/`.
//! Note: `GET /events/{id}/door` (UI-07) belongs to the registrations pane.
//!
//! Field edits autosave one field per PATCH (05). Deletes and creates are plain forms (`_method=DELETE`), so a
//! refusal (409 in use, 409 has registrations, 422 bad value) comes back as the full page with its status and the
//! refusal dialog open (`body[data-open]`); htmx does not swap 4xx answers. The billing upload is an htmx post that
//! answers with the page fragment (200, or 422 with the message).
//! Print menu items point at `/print/convention/...` and the CSV at `/exports/accounting.csv` (I5; 404 until then).

use crate::audit::RequestId;
use crate::auth::{CurrentUser, RequireStaff, Shell};
use crate::autosave::{self, Control, Ctx, FieldDef, FieldView, OneField, Outcome, Refusal, Registry, Stamp};
use crate::convention::events::{self as ev, EventTotals, NewEvent, NewEventItem};
use crate::convention::registries::{self as reg, Patched, Resource, Target};
use crate::convention::{self as cv, sponsors as sp, DomainError, Who};
use crate::error::AppError;
use crate::sys::files::{self, FileField, StoreError, StoredFile};
use crate::web::{is_htmx, render_page, render_page_status, Page};
use crate::AppState;
use askama::Template;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{delete, get, post};
use axum::{Extension, Form, Router};
use rusqlite::types::Value;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/events", get(list).post(create_event))
        .route("/events/new", get(new_event_page))
        .route("/events/{id}", get(event_page).patch(patch_event).delete(delete_event))
        .route("/events/{id}/pdf", post(attach_pdf).delete(remove_pdf))
        .route("/events/{id}/items", post(add_item))
        .route("/events/{id}/sponsors", post(add_sponsor))
        .route("/events/{id}/billing", post(upload_billing))
        .route("/event-items/{id}", get(item_page).patch(patch_item).delete(delete_item))
        .route("/sponsors/{id}", get(sponsor_page).patch(patch_sponsor).delete(delete_sponsor))
        .route("/billing/{id}", delete(delete_billing))
}

// ---------------------------------------------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------------------------------------------

fn usd(c: i64) -> String {
    crate::seed::usd(c)
}

fn val(v: &Option<String>) -> Value {
    v.clone().map(Value::Text).unwrap_or(Value::Null)
}

fn show_d(v: &Option<String>) -> String {
    reg::show_date(&val(v))
}

fn show_t(v: &Option<String>) -> String {
    reg::show_time(&val(v))
}

fn rid(r: &Option<Extension<RequestId>>) -> Option<&str> {
    r.as_ref().map(|r| r.0 .0.as_str())
}

/// `<span class="tag ok lg">Activo</span>` (BR-EVT-02 badge: green when Activo, neutral otherwise).
fn status_tag(status: &str, lg: bool) -> String {
    let kind = if status == "Activo" { " ok" } else { "" };
    format!("<span class=\"tag{kind}{}\">{}</span>", if lg { " lg" } else { "" }, esc(status))
}

fn esc(s: &str) -> String {
    askama::filters::escape(s, askama::filters::Html).map(|s| s.to_string()).unwrap_or_default()
}

/// Registration status tag class (design registros.html).
pub fn reg_tag(status: &str) -> &'static str {
    match status {
        "Registrado" => "tag info",
        "Asistió" => "tag ok",
        "No Canceló" | "No Asistió" => "tag warn",
        "Canceló" => "tag err",
        _ => "tag",
    }
}

/// A domain refusal that is not a 409/422 page state.
fn app(e: DomainError) -> AppError {
    match e {
        DomainError::Forbidden => AppError::Forbidden,
        DomainError::NotFound => AppError::NotFound,
        e => AppError::Internal(format!("events: {e:?}")),
    }
}

fn see_other(url: &str) -> Response {
    Redirect::to(url).into_response()
}

/// After a delete: htmx follows `HX-Redirect`, a plain form follows the 303.
fn go(headers: &HeaderMap, url: &str) -> Response {
    if is_htmx(headers) {
        let mut res = StatusCode::OK.into_response();
        if let Ok(v) = HeaderValue::from_str(url) {
            res.headers_mut().insert("hx-redirect", v);
        }
        res
    } else {
        see_other(url)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Autosave fields: control and id per registry field
// ---------------------------------------------------------------------------------------------------------------

fn opts(list: &[&str]) -> Vec<(String, String)> {
    list.iter().map(|v| ((*v).to_owned(), (*v).to_owned())).collect()
}

/// The control, element id and aria label of a field of an I2 resource on these screens.
fn spec(res: Resource, name: &str) -> Option<(&'static str, Control, Option<&'static str>)> {
    use Control::*;
    Some(match (res, name) {
        (Resource::Event, "name") => ("e-nombre", Text, None),
        (Resource::Event, "location") => ("e-lugar", Text, None),
        (Resource::Event, "date_start") => ("e-ini", Text, None),
        (Resource::Event, "date_end") => ("e-fin", Text, None),
        (Resource::Event, "status") => ("e-st", Select(opts(&["Activo", "Inactivo"])), None),
        (Resource::EventItem, "type") => ("a-tipo", Select(opts(ev::ITEM_TYPES)), None),
        (Resource::EventItem, "name") => ("a-nombre", Text, None),
        (Resource::EventItem, "location_room") => ("a-lugar", Text, None),
        (Resource::EventItem, "price") => ("a-precio", Money, None),
        (Resource::EventItem, "qty") => ("a-qty", Number, None),
        (Resource::EventItem, "date_start") => ("a-f1", Text, None),
        (Resource::EventItem, "time_start") => ("a-h1", Text, None),
        (Resource::EventItem, "time_end") => ("a-h2", Text, None),
        (Resource::Sponsor, "sponsor_name") => ("s-nombre", Text, None),
        (Resource::Sponsor, "type") => {
            let mut o = vec![(String::new(), "—".to_owned())];
            o.extend(opts(sp::CATEGORIES));
            ("s-cat", Select(o), None)
        }
        (Resource::Sponsor, "contact_name") => ("s-contacto", Text, None),
        (Resource::Sponsor, "title") => ("s-titulo", Text, None),
        (Resource::Sponsor, "greeting") => ("s-saludo", Text, None),
        (Resource::Sponsor, "phone") => ("s-tel", Text, None),
        (Resource::Sponsor, "address") => ("s-dir", Text, None),
        (Resource::Sponsor, "city") => ("s-ciudad", Text, Some("Ciudad")),
        (Resource::Sponsor, "state") => ("s-estado", Text, Some("Estado")),
        (Resource::Sponsor, "zip") => ("s-zip", Text, Some("Zip")),
        (Resource::Sponsor, "requested_amount") => ("s-pedido", Money, None),
        (Resource::Sponsor, "promised") => ("s-prometido", Money, None),
        (Resource::Sponsor, "amount_received") => ("s-recibido", Money, None),
        _ => return None,
    })
}

fn url_of(res: Resource, key: &str) -> String {
    match res {
        Resource::Event => format!("/events/{key}"),
        Resource::EventItem => format!("/event-items/{key}"),
        _ => format!("/sponsors/{key}"),
    }
}

/// The field as saved now.
fn field(conn: &Connection, res: Resource, key: &str, name: &str) -> Result<Option<FieldView>, AppError> {
    let r = res.registry();
    let (Some(def), Some((id, control, aria))) = (r.get(name), spec(res, name)) else { return Ok(None) };
    let v: Value = conn
        .query_row(&format!("SELECT {} FROM {} WHERE uuid = ?1", def.column, r.table), [key], |row| row.get(0))
        .optional()?
        .ok_or(AppError::NotFound)?;
    let mut f = FieldView::new(url_of(res, key), name, id, control, def.display(&v));
    if let Some(a) = aria {
        f = f.aria(a);
    }
    Ok(Some(f))
}

/// Field HTML with an id on its wrapper (`w-<id>`), so another PATCH can replace it out of band.
fn wrapped(f: &FieldView) -> String {
    f.html().replacen("<div class=\"field-wrap\"", &format!("<div id=\"w-{}\" class=\"field-wrap\"", f.id), 1)
}

fn oob_field(f: &FieldView) -> String {
    f.html().replacen(
        "<div class=\"field-wrap\"",
        &format!("<div id=\"w-{}\" hx-swap-oob=\"true\" class=\"field-wrap\"", f.id),
        1,
    )
}

/// Page helper: the wrapped field HTML of one record field.
fn fhtml(conn: &Connection, res: Resource, key: &str, name: &str) -> Result<String, AppError> {
    Ok(field(conn, res, key, name)?.map(|f| wrapped(&f)).unwrap_or_default())
}

#[allow(clippy::large_enum_variant)] // one per request, never stored
enum Patch {
    Contract,
    Field { view: FieldView, changed: bool },
}

/// One autosave PATCH through the registry (domain check and after-rule inside), answered with the field.
fn patch_field(
    conn: &mut Connection,
    who: &Who,
    request_id: Option<&str>,
    res: Resource,
    key: &str,
    f: &OneField,
) -> Result<Patch, AppError> {
    let outcome = match reg::patch(conn, who, request_id, Target::new(res, key), &f.name, &f.value)? {
        Patched::Unknown => return Ok(Patch::Contract),
        Patched::Done(o) => o,
    };
    let Some(view) = field(conn, res, key, &f.name)? else { return Ok(Patch::Contract) };
    Ok(match outcome {
        Outcome::Saved { changed, .. } => Patch::Field { view, changed },
        Outcome::Invalid(m) => Patch::Field { view: view.refused(&f.value, m), changed: false },
    })
}

fn field_response(view: &FieldView, extra: &str) -> Response {
    let status = if view.error.is_some() { StatusCode::UNPROCESSABLE_ENTITY } else { StatusCode::OK };
    (status, Html(format!("{}{extra}", wrapped(view)))).into_response()
}

// ---------------------------------------------------------------------------------------------------------------
// UI-08 Convenciones
// ---------------------------------------------------------------------------------------------------------------

pub struct EventRow {
    pub uuid: String,
    pub name: String,
    pub location: String,
    pub start: String,
    pub end: String,
    pub status: String,
    pub registrations: i64,
    pub total: i64,
}

#[derive(Template)]
#[template(path = "convention/events/list.html", blocks = ["content"])]
pub struct ListPage {
    pub shell: Shell,
    pub rows: Vec<EventRow>,
    pub q: String,
    pub status: String,
    pub notice: Option<&'static str>,
}

impl Page for ListPage {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

impl ListPage {
    pub fn usd(&self, c: &i64) -> String {
        usd(*c)
    }
    pub fn tag(&self, s: &str) -> String {
        status_tag(s, false)
    }
}

/// Query `event_list` (05): name, place, dates, status, registrations and total from `v_event_totals`.
pub fn event_list(conn: &Connection, q: &str, status: &str) -> rusqlite::Result<Vec<EventRow>> {
    let like = format!("%{}%", q.trim());
    conn.prepare(
        "SELECT e.uuid, e.name, e.location, e.date_start, e.date_end, e.status, t.count_registrations, t.total
           FROM events e JOIN v_event_totals t ON t.event_uuid = e.uuid
          WHERE (?1 = '%%' OR e.name LIKE ?1 OR COALESCE(e.location, '') LIKE ?1)
            AND (?2 = '' OR e.status = ?2)
          ORDER BY e.date_start DESC, e.name",
    )?
    .query_map(params![like, status], |r| {
        Ok(EventRow {
            uuid: r.get(0)?,
            name: r.get(1)?,
            location: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
            start: show_d(&r.get(3)?),
            end: show_d(&r.get(4)?),
            status: r.get(5)?,
            registrations: r.get(6)?,
            total: r.get(7)?,
        })
    })?
    .collect()
}

#[derive(Deserialize, Default)]
pub struct ListQuery {
    #[serde(default)]
    q: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    borrada: Option<String>,
}

async fn list(
    State(state): State<AppState>,
    RequireStaff { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
    Query(q): Query<ListQuery>,
) -> Result<Html<String>, AppError> {
    let status = if matches!(q.status.as_str(), "Activo" | "Inactivo") { q.status.clone() } else { String::new() };
    let rows = event_list(&state.db(), &q.q, &status)?;
    let page = ListPage {
        shell: Shell::new(&user, "convencion", "convenciones"),
        rows,
        q: q.q,
        status,
        notice: q.borrada.map(|_| "Convención borrada."),
    };
    render_page(&headers, &page)
}

// ---------------------------------------------------------------------------------------------------------------
// New event (OP-EVT-01 create): a create form with Guardar
// ---------------------------------------------------------------------------------------------------------------

#[derive(Deserialize, Default)]
pub struct NewEventForm {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub date_start: String,
    #[serde(default)]
    pub date_end: String,
}

#[derive(Template)]
#[template(path = "convention/events/new.html", blocks = ["content"])]
pub struct NewEventPage {
    pub shell: Shell,
    pub f: NewEventForm,
    pub errors: Vec<String>,
}

impl Page for NewEventPage {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

async fn new_event_page(
    RequireStaff { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
) -> Result<Html<String>, AppError> {
    let page = NewEventPage {
        shell: Shell::new(&user, "convencion", "convenciones"),
        f: NewEventForm::default(),
        errors: vec![],
    };
    render_page(&headers, &page)
}

/// A typed date ("5/6/2027" or ISO) as ISO text for the domain; empty stays empty.
fn iso_of(s: &str) -> Result<String, String> {
    match reg::parse_date(s)? {
        Value::Text(t) => Ok(t),
        _ => Ok(String::new()),
    }
}

async fn create_event(
    State(state): State<AppState>,
    RequireStaff { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
    Form(f): Form<NewEventForm>,
) -> Response {
    let who = Who::of(&user);
    let mut errors = vec![];
    let start = iso_of(&f.date_start).map_err(|m| errors.push(format!("Comienza: {m}"))).unwrap_or_default();
    let end = iso_of(&f.date_end).map_err(|m| errors.push(format!("Termina: {m}"))).unwrap_or_default();
    if errors.is_empty() {
        let mut conn = state.db();
        let result = (|| {
            let tx = cv::begin(&mut conn)?;
            let e = NewEvent { name: f.name.clone(), location: f.location.clone(), date_start: start, date_end: end };
            let uuid = ev::create_event(&tx, &who, &e)?;
            tx.commit()?;
            Ok::<_, DomainError>(uuid)
        })();
        match result {
            Ok(uuid) => return see_other(&format!("/events/{uuid}")),
            Err(DomainError::Validation(_, m)) => errors.push(format!("Nombre: {m}")),
            Err(e) => return app(e).into_response(),
        }
    }
    let page = NewEventPage { shell: Shell::new(&user, "convencion", "convenciones"), f, errors };
    render_page_status(StatusCode::UNPROCESSABLE_ENTITY, &headers, &page)
}

// ---------------------------------------------------------------------------------------------------------------
// UI-09 Convención
// ---------------------------------------------------------------------------------------------------------------

pub struct ItemRow {
    pub uuid: String,
    pub d1: String,
    pub t1: String,
    pub d2: String,
    pub t2: String,
    pub kind: String,
    pub name: String,
    pub room: String,
    pub qty: i64,
    pub price: i64,
    /// Registration items that use it (AP-06 / OP-EVT-03: in use, no delete).
    pub used: i64,
}

pub struct SponsorRow {
    pub uuid: String,
    pub name: String,
    pub kind: String,
    pub contact: String,
    pub phone: String,
    pub requested: Option<i64>,
    pub received: Option<i64>,
    pub promised: Option<i64>,
}

pub struct BillRow {
    pub uuid: String,
    pub when: String,
    pub file: Option<StoredFile>,
}

/// The "Añadir actividad" dialog: typed values after a refusal.
#[derive(Deserialize, Default, Debug)]
pub struct NewItemForm {
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub date_start: String,
    #[serde(default)]
    pub time_start: String,
    #[serde(default)]
    pub time_end: String,
    #[serde(default)]
    pub location_room: String,
    #[serde(default)]
    pub price: String,
    #[serde(default)]
    pub qty: String,
}

/// The inline "Añadir" row of the Auspiciadores tab.
#[derive(Deserialize, Default, Debug)]
pub struct NewSponsorForm {
    #[serde(default)]
    pub sponsor_name: String,
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub contact_name: String,
    #[serde(default)]
    pub requested_amount: String,
}

#[derive(Template)]
#[template(path = "convention/events/stats.html")]
pub struct Stats {
    pub t: EventTotals,
}

impl Stats {
    pub fn usd(&self, c: &i64) -> String {
        usd(*c)
    }
}

#[derive(Template)]
#[template(path = "convention/events/event.html", blocks = ["content"])]
pub struct EventPage {
    pub shell: Shell,
    pub uuid: String,
    pub name: String,
    pub location: String,
    pub status: String,
    pub dates: String,
    pub f_name: String,
    pub f_location: String,
    pub f_start: String,
    pub f_end: String,
    pub f_status: String,
    pub pdf: String,
    pub stats: String,
    pub totals: EventTotals,
    pub tab: &'static str,
    pub items: Vec<ItemRow>,
    pub sponsors: Vec<SponsorRow>,
    pub sp_requested: i64,
    pub billing: Vec<BillRow>,
    pub n_items: i64,
    pub n_sponsors: i64,
    pub n_billing: i64,
    pub registrations: i64,
    pub new_item: NewItemForm,
    pub new_sponsor: NewSponsorForm,
    pub notice: Option<String>,
    pub error: Option<String>,
    pub open: Option<String>,
}

impl Page for EventPage {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

impl EventPage {
    pub fn usd(&self, c: &i64) -> String {
        usd(*c)
    }
    pub fn usd_opt(&self, c: &Option<i64>) -> String {
        c.map(usd).unwrap_or_default()
    }
    pub fn tag(&self) -> String {
        status_tag(&self.status, true)
    }
    pub fn types(&self) -> &'static [&'static str] {
        ev::ITEM_TYPES
    }
    pub fn categories(&self) -> &'static [&'static str] {
        sp::CATEGORIES
    }
    pub fn sel(&self, a: &str, b: &str) -> &'static str {
        if a == b {
            " selected"
        } else {
            ""
        }
    }
    pub fn tab_attr(&self, t: &str) -> &'static str {
        if self.tab == t {
            "true"
        } else {
            "false"
        }
    }
}

fn tab_of(t: Option<&str>) -> &'static str {
    match t {
        Some("sponsors") => "sponsors",
        Some("billing") => "billing",
        _ => "items",
    }
}

fn pdf_widget(conn: &Connection, event: &str) -> Result<FileField, AppError> {
    let uuid: Option<String> = conn
        .query_row("SELECT pdf_file_uuid FROM events WHERE uuid = ?1", [event], |r| r.get(0))
        .optional()?
        .ok_or(AppError::NotFound)?;
    let file = match uuid {
        Some(u) => files::get_file(conn, &u)?,
        None => None,
    };
    Ok(FileField::new("pdf_file_uuid", "PDF", file).with_save_url(format!("/events/{event}")).with_accept(".pdf"))
}

fn stats_html(conn: &Connection, event: &str) -> Result<String, AppError> {
    Ok(Stats { t: ev::event_totals(conn, event).map_err(app)? }.render()?)
}

/// Query `event_get` (05) with the three tabs and the totals of `v_event_totals`.
fn build_event(conn: &Connection, user: &CurrentUser, id: &str, tab: &'static str) -> Result<EventPage, AppError> {
    type Head = (String, Option<String>, String, Option<String>, Option<String>);
    let (name, location, status, ds, de): Head = conn
        .query_row("SELECT name, location, status, date_start, date_end FROM events WHERE uuid = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .optional()?
        .ok_or(AppError::NotFound)?;
    let totals = ev::event_totals(conn, id).map_err(app)?;
    let items = conn
        .prepare(
            "SELECT ei.uuid, ei.date_start, ei.time_start, ei.date_end, ei.time_end, ei.type, ei.name, ei.location_room,
                    ei.qty, ei.price,
                    (SELECT COUNT(*) FROM registration_items ri WHERE ri.event_item_uuid = ei.uuid)
               FROM event_items ei WHERE ei.event_uuid = ?1
              ORDER BY ei.date_start, ei.time_start, ei.sort_order",
        )?
        .query_map([id], |r| {
            Ok(ItemRow {
                uuid: r.get(0)?,
                d1: show_d(&r.get(1)?),
                t1: show_t(&r.get(2)?),
                d2: show_d(&r.get(3)?),
                t2: show_t(&r.get(4)?),
                kind: r.get(5)?,
                name: r.get(6)?,
                room: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
                qty: r.get(8)?,
                price: r.get(9)?,
                used: r.get(10)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let sponsors = conn
        .prepare(
            "SELECT uuid, sponsor_name, type, contact_name, title, phone, requested_amount, amount_received, promised
               FROM sponsor WHERE event_uuid = ?1 ORDER BY sponsor_name COLLATE NOCASE",
        )?
        .query_map([id], |r| {
            let contact: Option<String> = r.get(3)?;
            let title: Option<String> = r.get(4)?;
            Ok(SponsorRow {
                uuid: r.get(0)?,
                name: r.get(1)?,
                kind: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                contact: [title, contact].into_iter().flatten().collect::<Vec<_>>().join(" "),
                phone: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
                requested: r.get(6)?,
                received: r.get(7)?,
                promised: r.get(8)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    // Requested is not in `v_event_totals` (promised and received are, BR-SPN-01): a plain read-time sum.
    let sp_requested: i64 =
        conn.query_row("SELECT COALESCE(SUM(requested_amount), 0) FROM sponsor WHERE event_uuid = ?1", [id], |r| {
            r.get(0)
        })?;
    let billing = conn
        .prepare("SELECT uuid, timestamp, file_uuid FROM billing WHERE event_uuid = ?1 ORDER BY timestamp DESC")?
        .query_map([id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .map(|(uuid, ts, f)| {
            let file = match f {
                Some(f) => files::get_file(conn, &f)?,
                None => None,
            };
            Ok(BillRow { uuid, when: ts.as_deref().map(bill_time).unwrap_or_default(), file })
        })
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut shell = Shell::new(user, "convencion", "convenciones");
    shell.autosave = true;
    let dates = match (show_d(&ds), show_d(&de)) {
        (a, b) if a.is_empty() => b,
        (a, b) if b.is_empty() || a == b => a,
        (a, b) => format!("{a} – {b}"),
    };
    Ok(EventPage {
        shell,
        uuid: id.to_owned(),
        location: location.unwrap_or_default(),
        dates,
        f_name: fhtml(conn, Resource::Event, id, "name")?,
        f_location: fhtml(conn, Resource::Event, id, "location")?,
        f_start: fhtml(conn, Resource::Event, id, "date_start")?,
        f_end: fhtml(conn, Resource::Event, id, "date_end")?,
        f_status: fhtml(conn, Resource::Event, id, "status")?,
        pdf: pdf_widget(conn, id)?.render()?,
        stats: Stats { t: totals.clone() }.render()?,
        totals,
        tab,
        n_items: items.len() as i64,
        n_sponsors: sponsors.len() as i64,
        n_billing: billing.len() as i64,
        items,
        sponsors,
        sp_requested,
        billing,
        registrations: ev::registration_count(conn, id)?,
        new_item: NewItemForm { kind: "Cena".into(), qty: "1".into(), ..Default::default() },
        new_sponsor: NewSponsorForm { kind: "Bronce".into(), ..Default::default() },
        notice: None,
        error: None,
        open: None,
        name,
        status,
    })
}

/// "5/04/2027 4:42:18 PM" (design format of the billing timestamp).
fn bill_time(stored: &str) -> String {
    chrono::NaiveDateTime::parse_from_str(stored, "%Y-%m-%d %H:%M:%S")
        .map(|t| t.format("%-m/%d/%Y %-I:%M:%S %p").to_string())
        .unwrap_or_else(|_| stored.to_owned())
}

#[derive(Deserialize, Default)]
pub struct TabQuery {
    tab: Option<String>,
}

async fn event_page(
    State(state): State<AppState>,
    RequireStaff { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<TabQuery>,
) -> Result<Html<String>, AppError> {
    let page = build_event(&state.db(), &user, &id, tab_of(q.tab.as_deref()))?;
    render_page(&headers, &page)
}

/// A refused form: the event page with its status, a message and a dialog to open.
fn event_refusal(
    conn: &Connection,
    user: &CurrentUser,
    headers: &HeaderMap,
    id: &str,
    tab: &'static str,
    status: StatusCode,
    edit: impl FnOnce(&mut EventPage),
) -> Response {
    match build_event(conn, user, id, tab) {
        Ok(mut page) => {
            edit(&mut page);
            render_page_status(status, headers, &page)
        }
        Err(e) => e.into_response(),
    }
}

/// `PATCH /events/{id}` (OP-EVT-02): one header field; the stat tiles come back out of band. `pdf_file_uuid` is the
/// PDF widget (OP-EVT-01 attach / remove).
async fn patch_event(
    State(state): State<AppState>,
    RequireStaff { 0: user, .. }: RequireStaff,
    r: Option<Extension<RequestId>>,
    Path(id): Path<String>,
    f: OneField,
) -> Result<Response, AppError> {
    let who = Who::of(&user);
    let mut conn = state.db();
    if f.name == "pdf_file_uuid" {
        return pdf_set(&mut conn, &who, &id, &f.value);
    }
    let (view, changed) = match patch_field(&mut conn, &who, rid(&r), Resource::Event, &id, &f)? {
        Patch::Contract => return Ok(autosave::contract_error("Este campo no se cambia aquí.")),
        Patch::Field { view, changed } => (view, changed),
    };
    let mut extra = autosave::oob("ev-stats", &stats_html(&conn, &id)?);
    if changed {
        match f.name.as_str() {
            // BR-EVT-01: an empty end date took the start date; show it.
            "date_start" | "date_end" => {
                let other = if f.name == "date_start" { "date_end" } else { "date_start" };
                if let Some(o) = field(&conn, Resource::Event, &id, other)? {
                    extra.push_str(&oob_field(&o));
                }
            }
            "status" => extra.push_str(&autosave::oob("ev-tag", &status_tag(&view.saved, true))),
            "name" => extra.push_str(&autosave::oob("ev-name", &esc(&view.saved))),
            _ => {}
        }
    }
    Ok(field_response(&view, &extra))
}

/// Attach (uuid) or remove (empty) the event PDF; answers with the widget (422 + message for a non-PDF).
fn pdf_set(conn: &mut Connection, who: &Who, id: &str, value: &str) -> Result<Response, AppError> {
    let result = (|| {
        let tx = cv::begin(conn)?;
        if value.trim().is_empty() {
            ev::remove_event_pdf(&tx, who, id)?;
        } else {
            ev::set_event_pdf(&tx, who, id, value.trim())?;
        }
        tx.commit()?;
        Ok::<_, DomainError>(())
    })();
    let mut w = pdf_widget(conn, id)?;
    let status = match result {
        Ok(()) => StatusCode::OK,
        Err(DomainError::Validation(..)) => {
            w.file = None;
            w.error = Some("Use un archivo PDF.");
            StatusCode::UNPROCESSABLE_ENTITY
        }
        Err(e) => return Err(app(e)),
    };
    Ok((status, Html(w.render()?)).into_response())
}

#[derive(Deserialize)]
pub struct PdfForm {
    #[serde(default)]
    pdf_file_uuid: String,
}

/// `POST /events/{id}/pdf` (05): attach a stored PDF by uuid.
async fn attach_pdf(
    State(state): State<AppState>,
    RequireStaff { 0: user, .. }: RequireStaff,
    Path(id): Path<String>,
    Form(f): Form<PdfForm>,
) -> Result<Response, AppError> {
    if f.pdf_file_uuid.trim().is_empty() {
        let w = pdf_widget(&state.db(), &id)?;
        return Ok((StatusCode::UNPROCESSABLE_ENTITY, Html(w.render()?)).into_response());
    }
    pdf_set(&mut state.db(), &Who::of(&user), &id, &f.pdf_file_uuid)
}

/// `DELETE /events/{id}/pdf` (05).
async fn remove_pdf(
    State(state): State<AppState>,
    RequireStaff { 0: user, .. }: RequireStaff,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    pdf_set(&mut state.db(), &Who::of(&user), &id, "")
}

/// `DELETE /events/{id}` (OP-EVT-01, AP-04): ROLE-ADMIN, only without registrations.
async fn delete_event(
    State(state): State<AppState>,
    RequireStaff { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let who = Who::of(&user);
    let mut conn = state.db();
    let result = (|| {
        let tx = cv::begin(&mut conn)?;
        ev::delete_event(&tx, &who, &id)?;
        tx.commit()?;
        Ok::<_, DomainError>(())
    })();
    match result {
        Ok(()) => go(&headers, "/events?borrada=1"),
        Err(e @ DomainError::HasRegistrations(_)) => {
            let msg = e.message();
            event_refusal(&conn, &user, &headers, &id, "items", e.status(), |p| {
                p.error = Some(msg);
                p.open = Some("dlg-borrar-ev".into());
            })
        }
        Err(e) => app(e).into_response(),
    }
}

/// `POST /events/{id}/items` (OP-EVT-03 add, BR-EVT-03 order, BR-EVT-01 end date): then the activity page.
async fn add_item(
    State(state): State<AppState>,
    RequireStaff { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(f): Form<NewItemForm>,
) -> Response {
    let who = Who::of(&user);
    let mut conn = state.db();
    let parsed = (|| {
        let date = iso_of(&f.date_start).map_err(|m| DomainError::Validation("date_start", m))?;
        let time = |s: &str, field| match reg::parse_time(s) {
            Ok(Value::Text(t)) => Ok(t),
            Ok(_) => Ok(String::new()),
            Err(m) => Err(DomainError::Validation(field, m)),
        };
        Ok::<_, DomainError>(NewEventItem {
            name: f.name.clone(),
            kind: f.kind.clone(),
            date_start: date,
            time_start: time(&f.time_start, "time_start")?,
            time_end: time(&f.time_end, "time_end")?,
            location_room: f.location_room.clone(),
            price: f.price.clone(),
            qty: f.qty.clone(),
        })
    })();
    let result = parsed.and_then(|it| {
        let tx = cv::begin(&mut conn)?;
        let uuid = ev::add_event_item(&tx, &who, &id, &it)?;
        tx.commit()?;
        Ok(uuid)
    });
    match result {
        Ok(uuid) => see_other(&format!("/event-items/{uuid}")),
        Err(DomainError::Validation(field, m)) => {
            let label = match field {
                "name" => "Nombre",
                "type" => "Tipo",
                "date_start" => "Fecha",
                "time_start" => "Hora comienza",
                "time_end" => "Hora termina",
                "price" => "Precio",
                "qty" => "Qty.",
                _ => "Lugar/Salón",
            };
            event_refusal(&conn, &user, &headers, &id, "items", StatusCode::UNPROCESSABLE_ENTITY, |p| {
                p.new_item = f;
                p.error = Some(format!("{label}: {m}"));
                p.open = Some("dlg-act-nueva".into());
            })
        }
        Err(e) => app(e).into_response(),
    }
}

/// `POST /events/{id}/sponsors` (OP-SPN-01 add): name, category, contact and requested amount in one insert.
async fn add_sponsor(
    State(state): State<AppState>,
    RequireStaff { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(f): Form<NewSponsorForm>,
) -> Response {
    let who = Who::of(&user);
    let mut conn = state.db();
    let result = (|| {
        let requested = reg::parse_money_opt(&f.requested_amount)
            .map_err(|m| DomainError::Validation("requested_amount", format!("Pedido: {m}")))?;
        let contact = cv::optional("contact_name", &f.contact_name, 200)?;
        let tx = cv::begin(&mut conn)?;
        let uuid = sp::create_sponsor(&tx, &who, &id, &f.sponsor_name, &f.kind)?;
        tx.execute(
            "UPDATE sponsor SET contact_name = ?1, requested_amount = ?2 WHERE uuid = ?3",
            params![contact, requested, uuid],
        )?;
        tx.commit()?;
        Ok::<_, DomainError>(uuid)
    })();
    match result {
        Ok(_) => see_other(&format!("/events/{id}?tab=sponsors")),
        Err(DomainError::Validation(field, m)) => {
            let m = if field == "sponsor_name" { format!("Auspiciador: {m}") } else { m };
            event_refusal(&conn, &user, &headers, &id, "sponsors", StatusCode::UNPROCESSABLE_ENTITY, |p| {
                p.new_sponsor = f;
                p.error = Some(m);
            })
        }
        Err(e) => app(e).into_response(),
    }
}

/// `POST /events/{id}/billing` (OP-SPN-02, D-16): multipart `file`, PDF only. Stores the bytes (OP-SYS-04), then the
/// billing row. htmx gets the page fragment on the Facturación tab; a plain form gets a 303.
async fn upload_billing(
    State(state): State<AppState>,
    RequireStaff { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
    mut mp: Multipart,
) -> Response {
    let mut picked: Option<(String, Vec<u8>)> = None;
    let mut too_large = false;
    while let Ok(Some(mut field)) = mp.next_field().await {
        if field.name() != Some("file") || picked.is_some() {
            continue;
        }
        let name = field.file_name().unwrap_or_default().to_owned();
        let mut buf = Vec::new();
        while let Ok(Some(c)) = field.chunk().await {
            if buf.len() + c.len() > files::MAX_FILE_BYTES {
                too_large = true;
                break;
            }
            buf.extend_from_slice(&c);
        }
        picked = Some((name, buf));
    }
    let refuse = |msg: &str| {
        let conn = state.db();
        let msg = msg.to_owned();
        event_refusal(&conn, &user, &headers, &id, "billing", StatusCode::UNPROCESSABLE_ENTITY, |p| p.error = Some(msg))
    };
    if state
        .db()
        .query_row("SELECT 1 FROM events WHERE uuid = ?1", [&id], |_| Ok(()))
        .optional()
        .ok()
        .flatten()
        .is_none()
    {
        return AppError::NotFound.into_response();
    }
    let (name, bytes) = match picked {
        _ if too_large => return refuse(files::FileError::TooLarge.message()),
        Some((n, b)) if !n.is_empty() => (n, b),
        _ => return refuse(files::FileError::NoFile.message()),
    };
    if files::mime_for(&name) != Some("application/pdf") {
        return refuse("Use un archivo PDF.");
    }
    let actor = user.actor();
    let st = state.clone();
    let stored = match tokio::task::spawn_blocking(move || files::store(&st, &actor, &name, &bytes)).await {
        Ok(Ok(f)) => f,
        Ok(Err(StoreError::Rejected(e))) => return refuse(e.message()),
        Ok(Err(StoreError::Internal(m))) => return AppError::Internal(m).into_response(),
        Err(e) => return AppError::Internal(format!("upload task: {e}")).into_response(),
    };
    let who = Who::of(&user);
    let mut conn = state.db();
    let result = (|| {
        let tx = cv::begin(&mut conn)?;
        sp::add_billing(&tx, &who, &id, &stored.uuid)?;
        tx.commit()?;
        Ok::<_, DomainError>(())
    })();
    match result {
        Ok(()) if is_htmx(&headers) => event_refusal(&conn, &user, &headers, &id, "billing", StatusCode::OK, |p| {
            p.notice = Some(format!("PDF subido: {}.", stored.filename))
        }),
        Ok(()) => see_other(&format!("/events/{id}?tab=billing")),
        Err(DomainError::Validation(_, m)) => {
            drop(conn);
            refuse(&m)
        }
        Err(e) => app(e).into_response(),
    }
}

/// `DELETE /billing/{id}` (OP-SPN-02, AP-06).
async fn delete_billing(
    State(state): State<AppState>,
    RequireStaff { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let who = Who::of(&user);
    let mut conn = state.db();
    let event: String = conn
        .query_row("SELECT event_uuid FROM billing WHERE uuid = ?1", [&id], |r| r.get(0))
        .optional()?
        .ok_or(AppError::NotFound)?;
    let tx = cv::begin(&mut conn)?;
    sp::delete_billing(&tx, &who, &id).map_err(app)?;
    tx.commit()?;
    Ok(go(&headers, &format!("/events/{event}?tab=billing")))
}

// ---------------------------------------------------------------------------------------------------------------
// UI-10 Detalle de actividad
// ---------------------------------------------------------------------------------------------------------------

pub struct LineRow {
    pub reg_uuid: String,
    pub number: String,
    pub name: String,
    pub company: String,
    pub qty: i64,
    pub total: i64,
    pub status: String,
}

#[derive(Template)]
#[template(path = "convention/events/item.html", blocks = ["content"])]
pub struct ItemPage {
    pub shell: Shell,
    pub uuid: String,
    pub name: String,
    pub kind: String,
    pub event_uuid: String,
    pub event_name: String,
    pub event_location: String,
    pub f_type: String,
    pub f_name: String,
    pub f_room: String,
    pub f_price: String,
    pub f_qty: String,
    pub f_start: String,
    pub f_t1: String,
    pub f_t2: String,
    pub end: String,
    pub lines: Vec<LineRow>,
    pub line_count: i64,
    pub line_qty: i64,
    pub line_total: i64,
}

impl Page for ItemPage {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

impl ItemPage {
    pub fn usd(&self, c: &i64) -> String {
        usd(*c)
    }
    pub fn tag_class(&self, s: &str) -> &'static str {
        reg_tag(s)
    }
    pub fn end_html(&self) -> String {
        end_field(&self.end, false)
    }
}

/// BR-EVT-01: an activity's end date is its start date. Read only; it follows Comienza.
fn end_field(end: &str, oob: bool) -> String {
    format!(
        "<div id=\"w-a-f2\"{} class=\"field-wrap\"><input class=\"field\" id=\"a-f2\" value=\"{}\" readonly aria-readonly=\"true\" title=\"Igual a Comienza (BR-EVT-01)\"></div>",
        if oob { " hx-swap-oob=\"true\"" } else { "" },
        esc(end)
    )
}

const LINE_LIMIT: i64 = 100;

/// Query `event_item_get` (05): the item and the registration items that use it (portal key 18).
fn build_item(conn: &Connection, user: &CurrentUser, id: &str) -> Result<ItemPage, AppError> {
    type Head = (String, String, Option<String>, String, String, Option<String>);
    let (name, kind, end, event_uuid, event_name, event_location): Head = conn
        .query_row(
            "SELECT ei.name, ei.type, ei.date_end, e.uuid, e.name, e.location
               FROM event_items ei JOIN events e ON e.uuid = ei.event_uuid WHERE ei.uuid = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    let lines = conn
        .prepare(
            "SELECT r.uuid, r.registration_number, p.name_first, p.name_last, p.company, l.ticket_qty, l.total, r.status
               FROM v_registration_item_lines l
               JOIN registration r ON r.uuid = l.registration_uuid
               JOIN people p ON p.uuid = r.person_uuid
              WHERE l.event_item_uuid = ?1
              ORDER BY r.registration_number LIMIT ?2",
        )?
        .query_map(params![id, LINE_LIMIT], |r| {
            Ok(LineRow {
                reg_uuid: r.get(0)?,
                number: r.get::<_, Option<i64>>(1)?.map(|n| n.to_string()).unwrap_or_default(),
                name: cv::people::full_name(
                    r.get::<_, Option<String>>(2)?.as_deref(),
                    r.get::<_, Option<String>>(3)?.as_deref(),
                ),
                company: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                qty: r.get(5)?,
                total: r.get(6)?,
                status: r.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let (line_count, line_qty, line_total): (i64, i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(ticket_qty), 0), COALESCE(SUM(total), 0)
           FROM v_registration_item_lines WHERE event_item_uuid = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let mut shell = Shell::new(user, "convencion", "convenciones");
    shell.autosave = true;
    let f = |n| fhtml(conn, Resource::EventItem, id, n);
    Ok(ItemPage {
        shell,
        uuid: id.to_owned(),
        f_type: f("type")?,
        f_name: f("name")?,
        f_room: f("location_room")?,
        f_price: f("price")?,
        f_qty: f("qty")?,
        f_start: f("date_start")?,
        f_t1: f("time_start")?,
        f_t2: f("time_end")?,
        end: show_d(&end),
        name,
        kind,
        event_uuid,
        event_name,
        event_location: event_location.unwrap_or_default(),
        lines,
        line_count,
        line_qty,
        line_total,
    })
}

async fn item_page(
    State(state): State<AppState>,
    RequireStaff { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Html<String>, AppError> {
    let page = build_item(&state.db(), &user, &id)?;
    render_page(&headers, &page)
}

/// `PATCH /event-items/{id}` (OP-EVT-03): one field; a new start date also moves the read-only end date (BR-EVT-01).
async fn patch_item(
    State(state): State<AppState>,
    RequireStaff { 0: user, .. }: RequireStaff,
    r: Option<Extension<RequestId>>,
    Path(id): Path<String>,
    f: OneField,
) -> Result<Response, AppError> {
    let who = Who::of(&user);
    let mut conn = state.db();
    let (view, changed) = match patch_field(&mut conn, &who, rid(&r), Resource::EventItem, &id, &f)? {
        Patch::Contract => return Ok(autosave::contract_error("Este campo no se cambia aquí.")),
        Patch::Field { view, changed } => (view, changed),
    };
    let mut extra = String::new();
    if changed {
        match f.name.as_str() {
            "date_start" => {
                let end: Option<String> =
                    conn.query_row("SELECT date_end FROM event_items WHERE uuid = ?1", [&id], |r| r.get(0))?;
                extra.push_str(&end_field(&show_d(&end), true));
            }
            "name" => extra.push_str(&autosave::oob("ai-name", &esc(&view.saved))),
            "type" => extra.push_str(&autosave::oob("ai-type", &format!("· {}", esc(&view.saved)))),
            _ => {}
        }
    }
    Ok(field_response(&view, &extra))
}

/// `DELETE /event-items/{id}` (OP-EVT-03, AP-06): 409 while a registration item uses it.
async fn delete_item(
    State(state): State<AppState>,
    RequireStaff { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let who = Who::of(&user);
    let mut conn = state.db();
    let event: String = conn
        .query_row("SELECT event_uuid FROM event_items WHERE uuid = ?1", [&id], |r| r.get(0))
        .optional()?
        .ok_or(AppError::NotFound)?;
    let result = (|| {
        let tx = cv::begin(&mut conn)?;
        ev::delete_event_item(&tx, &who, &id)?;
        tx.commit()?;
        Ok::<_, DomainError>(())
    })();
    Ok(match result {
        Ok(()) => go(&headers, &format!("/events/{event}?tab=items")),
        Err(e @ DomainError::InUse(_)) => {
            let msg = e.message();
            event_refusal(&conn, &user, &headers, &event, "items", e.status(), |p| {
                p.error = Some(msg);
                p.open = Some(format!("dlg-quitar-act-{id}"));
            })
        }
        Err(e) => app(e).into_response(),
    })
}

// ---------------------------------------------------------------------------------------------------------------
// UI-11 Auspiciador
// ---------------------------------------------------------------------------------------------------------------

/// The sponsor logo (OP-SPN-01, `sponsor.logo_file_uuid`): the uuid of an image stored by OP-SYS-04.
static SPONSOR_LOGO: Registry = Registry {
    table: "sponsor",
    key_col: "uuid",
    stamp: Stamp::Full,
    fields: &[FieldDef::new("logo_file_uuid", "logo_file_uuid", "Logo", |s| autosave::optional_text(s, 64))],
};

#[derive(Template)]
#[template(path = "convention/events/sponsor.html", blocks = ["content"])]
pub struct SponsorPage {
    pub shell: Shell,
    pub uuid: String,
    pub name: String,
    pub kind: String,
    pub event_uuid: String,
    pub event_name: String,
    pub event_dates: String,
    pub fields: Vec<(&'static str, String)>,
    pub logo: String,
}

impl Page for SponsorPage {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

impl SponsorPage {
    pub fn f(&self, name: &str) -> String {
        self.fields.iter().find(|(n, _)| *n == name).map(|(_, h)| h.clone()).unwrap_or_default()
    }
    pub fn tag(&self) -> String {
        sponsor_tag(&self.kind)
    }
}

fn sponsor_tag(kind: &str) -> String {
    if kind.is_empty() {
        String::new()
    } else {
        format!("<span class=\"tag info lg\">{}</span>", esc(kind))
    }
}

fn logo_widget(conn: &Connection, id: &str) -> Result<FileField, AppError> {
    let uuid: Option<String> = conn
        .query_row("SELECT logo_file_uuid FROM sponsor WHERE uuid = ?1", [id], |r| r.get(0))
        .optional()?
        .ok_or(AppError::NotFound)?;
    let file = match uuid {
        Some(u) => files::get_file(conn, &u)?,
        None => None,
    };
    Ok(FileField::new("logo_file_uuid", "logo", file)
        .with_save_url(format!("/sponsors/{id}"))
        .with_accept(files::IMAGE_ACCEPT))
}

const SPONSOR_FIELDS: &[&str] = &[
    "sponsor_name",
    "type",
    "contact_name",
    "title",
    "greeting",
    "phone",
    "address",
    "city",
    "state",
    "zip",
    "requested_amount",
    "promised",
    "amount_received",
];

/// Query `sponsor_get` (05).
fn build_sponsor(conn: &Connection, user: &CurrentUser, id: &str) -> Result<SponsorPage, AppError> {
    type Head = (String, Option<String>, String, String, Option<String>, Option<String>);
    let (name, kind, event_uuid, event_name, ds, de): Head = conn
        .query_row(
            "SELECT s.sponsor_name, s.type, e.uuid, e.name, e.date_start, e.date_end
               FROM sponsor s JOIN events e ON e.uuid = s.event_uuid WHERE s.uuid = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    let mut fields = vec![];
    for n in SPONSOR_FIELDS {
        fields.push((*n, fhtml(conn, Resource::Sponsor, id, n)?));
    }
    let mut shell = Shell::new(user, "convencion", "convenciones");
    shell.autosave = true;
    let event_dates = [show_d(&ds), show_d(&de)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" – ");
    Ok(SponsorPage {
        shell,
        uuid: id.to_owned(),
        name,
        kind: kind.unwrap_or_default(),
        event_uuid,
        event_name,
        event_dates,
        fields,
        logo: logo_widget(conn, id)?.render()?,
    })
}

async fn sponsor_page(
    State(state): State<AppState>,
    RequireStaff { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Html<String>, AppError> {
    let page = build_sponsor(&state.db(), &user, &id)?;
    render_page(&headers, &page)
}

/// `PATCH /sponsors/{id}` (OP-SPN-01): one field, or the logo widget (`logo_file_uuid`, images only).
async fn patch_sponsor(
    State(state): State<AppState>,
    RequireStaff { 0: user, .. }: RequireStaff,
    r: Option<Extension<RequestId>>,
    Path(id): Path<String>,
    f: OneField,
) -> Result<Response, AppError> {
    let who = Who::of(&user);
    let mut conn = state.db();
    if f.name == "logo_file_uuid" {
        return logo_set(&mut conn, &who, rid(&r), &id, &f.value);
    }
    let (view, changed) = match patch_field(&mut conn, &who, rid(&r), Resource::Sponsor, &id, &f)? {
        Patch::Contract => return Ok(autosave::contract_error("Este campo no se cambia aquí.")),
        Patch::Field { view, changed } => (view, changed),
    };
    let mut extra = String::new();
    if changed {
        match f.name.as_str() {
            "type" => extra.push_str(&autosave::oob("sp-tag", &sponsor_tag(&view.saved))),
            "sponsor_name" => extra.push_str(&autosave::oob("sp-name", &esc(&view.saved))),
            _ => {}
        }
    }
    Ok(field_response(&view, &extra))
}

fn logo_set(
    conn: &mut Connection,
    who: &Who,
    request_id: Option<&str>,
    id: &str,
    value: &str,
) -> Result<Response, AppError> {
    cv::policy::ap03_domain(who).map_err(app)?;
    let mut w = logo_widget(conn, id)?;
    let def = &SPONSOR_LOGO.fields[0];
    let ctx = Ctx { actor: &who.actor, request_id, record: "Auspiciador" };
    let key = Value::Text(id.to_owned());
    let outcome = autosave::save(conn, &SPONSOR_LOGO, &key, def, value, &ctx, |tx, _old, new| {
        if let Value::Text(uuid) = new {
            let mime: Option<String> =
                tx.query_row("SELECT mime_type FROM stored_files WHERE uuid = ?1", [uuid], |r| r.get(0)).optional()?;
            if !mime.is_some_and(|m| m.starts_with("image/")) {
                return Err(Refusal::Invalid("Use una imagen PNG, JPEG, GIF o WebP.".into()));
            }
        }
        Ok(())
    })?;
    let status = match outcome {
        Outcome::Saved { .. } => {
            w = logo_widget(conn, id)?;
            StatusCode::OK
        }
        Outcome::Invalid(_) => {
            w.file = None;
            w.error = Some("Use una imagen PNG, JPEG, GIF o WebP.");
            StatusCode::UNPROCESSABLE_ENTITY
        }
    };
    Ok((status, Html(w.render()?)).into_response())
}

/// `DELETE /sponsors/{id}` (OP-SPN-01, AP-06).
async fn delete_sponsor(
    State(state): State<AppState>,
    RequireStaff { 0: user, .. }: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let who = Who::of(&user);
    let mut conn = state.db();
    let event: String = conn
        .query_row("SELECT event_uuid FROM sponsor WHERE uuid = ?1", [&id], |r| r.get(0))
        .optional()?
        .ok_or(AppError::NotFound)?;
    let tx = cv::begin(&mut conn)?;
    sp::delete_sponsor(&tx, &who, &id).map_err(app)?;
    tx.commit()?;
    Ok(go(&headers, &format!("/events/{event}?tab=sponsors")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(bill_time("2027-05-04 16:42:18"), "5/04/2027 4:42:18 PM");
        assert_eq!(status_tag("Activo", true), "<span class=\"tag ok lg\">Activo</span>");
        assert_eq!(reg_tag("Canceló"), "tag err");
        let f = FieldView::new("/events/x", "name", "e-nombre", Control::Text, "A");
        assert!(wrapped(&f).starts_with("<div id=\"w-e-nombre\" class=\"field-wrap\""));
        assert!(oob_field(&f).contains("hx-swap-oob=\"true\""));
        for n in SPONSOR_FIELDS {
            assert!(spec(Resource::Sponsor, n).is_some() && Resource::Sponsor.registry().get(n).is_some(), "{n}");
        }
        for d in reg::EVENT_ITEM.fields {
            assert!(spec(Resource::EventItem, d.name).is_some(), "{}", d.name);
        }
        for d in reg::EVENT.fields {
            assert!(spec(Resource::Event, d.name).is_some(), "{}", d.name);
        }
    }
}
