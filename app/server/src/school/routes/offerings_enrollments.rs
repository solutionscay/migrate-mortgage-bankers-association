//! Phase-2 pane "offerings_enrollments": UI-18 Ofrecimientos, UI-19 Fecha del curso, UI-20 Matrícula, UI-21
//! Matrículas.
//!
//! * UI-18: `GET /offerings/{kind}` (`?o=` picks the offering), `POST /offerings/{kind}` (OP-CRS-01 create),
//!   `GET|PATCH|DELETE /offerings/{kind}/{id}` (Detalles autosave; AP-04 delete), `POST /offerings/{kind}/{id}/calendar`
//!   ("Abrir curso", OP-CRS-02, BR-CRS-02).
//! * UI-19: `GET /calendar/{kind}/{id}` (UI-18 with the dialog open), `PATCH /calendar/{kind}/{id}` (one field; the
//!   calendar row behind the dialog out of band), `POST /calendar/{kind}/{id}/dates`,
//!   `DELETE /calendar/{kind}/{id}/dates/{date_id}` (OP-CRS-03). AP-07 / T-05-07: `/calendar/{kind}/{id}` has GET and
//!   PATCH only, so DELETE answers 405.
//! * UI-20: `GET /students/{id}/enrollments/{kind}/new` and `POST /students/{id}/enrollments/{kind}` (OP-ENR-01..03 with
//!   the settle form), `GET /enrollments/{kind}/new?calendar=` and `POST /enrollments/{kind}` (the same dialog opened
//!   from a calendar row, with a student pick), `GET /calendar-options/{kind}` (T-05-11), `GET|POST|PATCH|DELETE
//!   /enrollments/{kind}/{id}` (OP-ENR-04, OP-ENR-08; OP-ENR-05 delete) and `POST /enrollments/{kind}/{id}/cancel`.
//! * UI-21: `GET /enrollments/{kind}` (`?calendar=`, `?status=`, `?q=`), `GET /calendar/{kind}/{id}/enrollments`
//!   (OP-CRS-05), `GET /exports/grades.xlsx` (OP-ENR-06).
//!
//! Plain forms everywhere a command writes (303 on success; a refusal re-renders the page with its status and the
//! dialog open). Record fields autosave through `school::registries::patch`. Money in cents; counts and revenue
//! from `v_calendar_*_summary`; course balances from `v_enrollments_course_lines`.

use crate::audit::RequestId;
use crate::auth::{CurrentUser, RequireStaff, Shell};
use crate::autosave::{self, Control, FieldView, OneField, Outcome};
use crate::convention::registries::{parse_date, parse_money, show_date};
use crate::error::AppError;
use crate::school::offerings::{self, CalendarRow, CALENDAR_STATUSES, DAY_TYPES, MAX_SHORT_DATES};
use crate::school::registries::{self as sreg, Patched, Resource};
use crate::school::{self, enrollments, grades, students, DomainError, Kind, Who};
use crate::web::{is_htmx, render_page_status, safe_next, urlencode, Page};
use crate::AppState;
use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{delete, get, post};
use axum::{Extension, Form, Router};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use std::collections::HashMap;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/offerings/{kind}", get(offerings_index).post(offering_create))
        .route("/offerings/{kind}/{id}", get(offering_page).patch(offering_patch).delete(offering_delete))
        .route("/offerings/{kind}/{id}/calendar", post(calendar_open))
        // AP-07 / T-05-07: GET and PATCH only; DELETE is 405.
        .route("/calendar/{kind}/{id}", get(calendar_dialog).patch(calendar_patch))
        .route("/calendar/{kind}/{id}/dates", post(date_add))
        .route("/calendar/{kind}/{id}/dates/{date_id}", delete(date_remove))
        .route("/calendar/{kind}/{id}/enrollments", get(calendar_enrollments))
        .route("/calendar-options/{kind}", get(calendar_options))
        .route("/students/{id}/enrollments/{kind}/new", get(enroll_new_for_student))
        .route("/students/{id}/enrollments/{kind}", post(enroll_create_for_student))
        .route("/enrollments/{kind}", get(enrollments_list).post(enroll_create_from_calendar))
        .route("/enrollments/{kind}/new", get(enroll_new_from_calendar))
        .route(
            "/enrollments/{kind}/{id}",
            get(enrollment_page).post(enrollment_save).patch(enrollment_patch).delete(enrollment_delete),
        )
        .route("/enrollments/{kind}/{id}/cancel", post(enrollment_cancel))
        .route("/exports/grades.xlsx", get(export_grades))
}

// ===============================================================================================================
// Shared helpers
// ===============================================================================================================

fn kind_of(slug: &str) -> Result<Kind, AppError> {
    Kind::from_slug(slug).ok_or(AppError::NotFound)
}

fn usd(c: i64) -> String {
    crate::seed::usd(c)
}

fn usd_opt(c: Option<i64>) -> String {
    c.map(usd).unwrap_or_else(|| "—".into())
}

fn date_txt(d: &Option<String>) -> String {
    d.as_ref().map(|s| show_date(&Value::Text(s.clone()))).unwrap_or_default()
}

fn num_txt(v: Option<f64>) -> String {
    match v {
        Some(f) if f.fract() == 0.0 => format!("{}", f as i64),
        Some(f) => format!("{f}"),
        None => String::new(),
    }
}

fn esc(s: &str) -> String {
    askama::filters::escape(s, askama::filters::Html).map(|e| e.to_string()).unwrap_or_default()
}

fn rid(r: &Option<Extension<RequestId>>) -> Option<&str> {
    r.as_ref().map(|r| r.0 .0.as_str())
}

/// 303 for a plain form; `HX-Redirect` for htmx.
fn go(headers: &HeaderMap, to: &str) -> Response {
    if is_htmx(headers) {
        let mut res = StatusCode::OK.into_response();
        if let Ok(v) = HeaderValue::from_str(to) {
            res.headers_mut().insert("hx-redirect", v);
        }
        res
    } else {
        Redirect::to(to).into_response()
    }
}

/// Singular kind label for titles ("curso especializado").
fn kind_singular(k: Kind) -> &'static str {
    match k {
        Kind::Course => "curso especializado",
        Kind::Seminar => "seminario",
        Kind::ShortCourse => "curso corto",
    }
}

/// The name label of an offering ("Curso", "Seminario", "Curso corto").
fn kind_name_label(k: Kind) -> &'static str {
    match k {
        Kind::Course => "Curso",
        Kind::Seminar => "Seminario",
        Kind::ShortCourse => "Curso corto",
    }
}

fn kind_new_label(k: Kind) -> &'static str {
    match k {
        Kind::Course => "Nuevo curso",
        Kind::Seminar => "Nuevo seminario",
        Kind::ShortCourse => "Nuevo curso corto",
    }
}

/// The kind switch of UI-18 and UI-21: (href, label, pressed).
fn kind_switch(base: &str, current: Kind) -> Vec<(String, &'static str, bool)> {
    Kind::ALL.iter().map(|k| (format!("{base}/{}", k.slug()), k.label(), *k == current)).collect()
}

/// Tag class of a calendar status (design/ofrecimiento.html).
fn cal_tag(status: &str) -> &'static str {
    match status {
        "Pendiente" => " warn",
        "Confirmado" => " ok",
        "Completado" => " info",
        "Cancelado" => " err",
        _ => "",
    }
}

/// Tag class of an enrollment status.
fn enr_tag(status: &str) -> &'static str {
    match status {
        "ACTIVO" | "Asistió" => " ok",
        "Canceló" => " err",
        "No Canceló" => " warn",
        _ => "",
    }
}

fn pairs(list: &[&str]) -> Vec<(String, String)> {
    list.iter().map(|v| ((*v).to_owned(), (*v).to_owned())).collect()
}

fn professors(conn: &Connection) -> rusqlite::Result<Vec<(String, String)>> {
    conn.prepare("SELECT uuid, name FROM professors ORDER BY name")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect()
}

fn stored(conn: &Connection, table: &str, column: &str, key: &str) -> Result<Value, AppError> {
    conn.query_row(&format!("SELECT {column} FROM {table} WHERE uuid = ?1"), [key], |r| r.get(0))
        .optional()?
        .ok_or(AppError::NotFound)
}

/// A page whose `content` block can be rendered alone, and whose pagebar goes out of band with it (htmx swaps of
/// `#main` from the kind switch and the pickers keep the title in step).
trait OobPage: Template {
    fn content(&self) -> askama::Result<String>;
    fn pagebar(&self) -> askama::Result<String>;
}

fn render_oob<P: OobPage>(status: StatusCode, headers: &HeaderMap, page: &P) -> Response {
    let html = if is_htmx(headers) {
        page.content().and_then(|c| {
            page.pagebar()
                .map(|p| c + &p.replacen("<div class=\"pagebar\"", "<div hx-swap-oob=\"true\" class=\"pagebar\"", 1))
        })
    } else {
        page.render()
    };
    match html {
        Ok(h) => (status, Html(h)).into_response(),
        Err(e) => AppError::from(e).into_response(),
    }
}

// ===============================================================================================================
// UI-18 Ofrecimientos
// ===============================================================================================================

/// One offering as UI-18 shows it.
struct OfferingInfo {
    uuid: String,
    name: String,
    credits: Option<f64>,
    hours: Option<f64>,
    member: Option<i64>,
    individual: Option<i64>,
}

fn offering_info(conn: &Connection, kind: Kind, id: &str) -> Result<OfferingInfo, AppError> {
    let (m, i) = kind.offering_prices();
    let credits = if kind == Kind::Course { "credits" } else { "NULL" };
    conn.query_row(
        &format!(
            "SELECT uuid, {}, {credits}, hours, {m}, {i} FROM {} WHERE uuid = ?1",
            kind.name_col(),
            kind.offering_table()
        ),
        [id],
        |r| {
            Ok(OfferingInfo {
                uuid: r.get(0)?,
                name: r.get(1)?,
                credits: r.get(2)?,
                hours: r.get(3)?,
                member: r.get(4)?,
                individual: r.get(5)?,
            })
        },
    )
    .optional()?
    .ok_or(AppError::NotFound)
}

/// (uuid, name) of every offering of a kind, by name.
fn offering_options(conn: &Connection, kind: Kind) -> rusqlite::Result<Vec<(String, String)>> {
    conn.prepare(&format!("SELECT uuid, {0} FROM {1} ORDER BY {0}", kind.name_col(), kind.offering_table()))?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect()
}

/// The offering UI-18 opens on: the one with the most calendar rows (then by name).
fn default_offering(conn: &Connection, kind: Kind) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        &format!(
            "SELECT o.uuid FROM {0} o ORDER BY (SELECT COUNT(*) FROM {1} c WHERE c.{2} = o.uuid) DESC, o.{3} LIMIT 1",
            kind.offering_table(),
            kind.calendar_table(),
            kind.calendar_offering_col(),
            kind.name_col()
        ),
        [],
        |r| r.get(0),
    )
    .optional()
}

/// The subtitle: "· curso especializado · 3 créditos · 45 horas" (short courses add the NMLS total, BR-CRS-04).
fn offering_sub(conn: &Connection, kind: Kind, o: &OfferingInfo) -> Result<String, AppError> {
    let mut s = format!("· {}", kind_singular(kind));
    if let Some(c) = o.credits {
        s.push_str(&format!(" · {} créditos", num_txt(Some(c))));
    }
    if let Some(h) = o.hours {
        s.push_str(&format!(" · {} horas", num_txt(Some(h))));
    }
    if kind == Kind::ShortCourse {
        if let Some(n) = offerings::nmls_total(conn, &o.uuid).map_err(dom)? {
            s.push_str(&format!(" · NMLS {}", usd(n)));
        }
    }
    Ok(s)
}

fn dom(e: DomainError) -> AppError {
    match e {
        DomainError::Forbidden => AppError::Forbidden,
        DomainError::NotFound => AppError::NotFound,
        e => AppError::Internal(e.message()),
    }
}

/// One calendar row of UI-18, with its cells as HTML (the same cells go out of band after a UI-19 edit).
pub struct CalRowView {
    pub uuid: String,
    pub date: String,
    pub cells: Vec<(String, &'static str, String)>,
    pub selected: bool,
    pub status: String,
}

/// The cells of a calendar row: (cell id suffix, td class, inner HTML). "Se necesitan" is clamped at 0 on screen
/// (the view gives quorum − enrollments, which can be negative).
fn cal_cells(kind: Kind, r: &CalendarRow) -> Vec<(String, &'static str, String)> {
    let date = r
        .first_date
        .as_ref()
        .map(|d| date_txt(&Some(d.clone())))
        .unwrap_or_else(|| "<span class=\"muted\">Sin fecha</span>".into());
    let semester = match (kind, &r.semester) {
        (Kind::Course, Some(s)) if !s.is_empty() => esc(s),
        _ => "<span class=\"muted\">—</span>".into(),
    };
    let needed = r.needed.map(|n| n.max(0));
    let needed_html = match needed {
        Some(n) => n.to_string(),
        None => "—".into(),
    };
    vec![
        ("fecha".into(), "", date),
        ("sem".into(), "", semester),
        ("st".into(), "", format!("<span class=\"tag{}\">{}</span>", cal_tag(&r.status), esc(&r.status))),
        ("pm".into(), "money", usd_opt(r.cost_member)),
        ("pi".into(), "money", usd_opt(r.cost_individual)),
        ("mat".into(), "num", r.enrollments.to_string()),
        ("quo".into(), "num", r.quorum.map(|q| q.to_string()).unwrap_or_else(|| "—".into())),
        ("nec".into(), if needed == Some(0) { "num muted" } else { "num" }, needed_html),
        ("ing".into(), "money", usd(r.revenue_total)),
    ]
}

fn cal_row_view(kind: Kind, r: &CalendarRow, open: Option<&str>) -> CalRowView {
    CalRowView {
        uuid: r.uuid.clone(),
        date: r.first_date.as_ref().map(|d| date_txt(&Some(d.clone()))).unwrap_or_else(|| "Sin fecha".into()),
        cells: cal_cells(kind, r),
        selected: open == Some(r.uuid.as_str()),
        status: r.status.clone(),
    }
}

/// The out-of-band cells of one calendar row (UI-18 behind the UI-19 dialog).
fn cal_row_oob(kind: Kind, r: &CalendarRow) -> String {
    let mut s = String::new();
    for (id, _, html) in cal_cells(kind, r) {
        s.push_str(&autosave::oob(&format!("cal-{}-{id}", r.uuid), &html));
    }
    s.push_str(&autosave::oob(&format!("fc-mat-{}", r.uuid), &enrolled_line(r)));
    s
}

/// "18 de quórum 12 · se necesitan 0".
fn enrolled_line(r: &CalendarRow) -> String {
    match r.quorum {
        Some(q) => format!("{} de quórum {q} · se necesitan {}", r.enrollments, r.needed.unwrap_or(0).max(0)),
        None => format!("{} matriculados · sin quórum", r.enrollments),
    }
}

/// One Detalles field of UI-18.
pub struct DetField {
    pub label: &'static str,
    pub id: String,
    pub req: bool,
    pub f: FieldView,
    pub gap: bool,
}

fn det_control(name: &str) -> Control {
    match name {
        "course_description" | "description" => Control::Textarea(4),
        "course_requirements" => Control::Textarea(3),
        "credits" | "hours" | "quorum" => Control::Number,
        "cost_member" | "cost_individual" | "cost_affiliate" => Control::Money,
        "nmls_fee" => Control::Select(vec![("0".into(), "No".into()), ("1".into(), "Sí".into())]),
        _ => Control::Text,
    }
}

fn det_field(kind: Kind, id: &str, name: &str, saved: String) -> FieldView {
    let f = FieldView::new(
        format!("/offerings/{}/{id}", kind.slug()),
        name,
        &format!("of-{name}"),
        det_control(name),
        saved,
    );
    match name {
        "cost_individual" => f.hint("Una fecha nueva copia estos precios una vez (BR-CRS-02)."),
        "nmls_fee" => f.hint("Con cargo NMLS: horas × tarifa NMLS (BR-CRS-04)."),
        _ => f,
    }
}

fn det_fields(conn: &Connection, kind: Kind, id: &str) -> Result<(Vec<DetField>, Vec<DetField>), AppError> {
    let reg = Resource::Offering(kind).registry();
    let (mut left, mut right) = (Vec::new(), Vec::new());
    for def in reg.fields {
        let v = stored(conn, reg.table, def.column, id)?;
        let shown = if def.name == "nmls_fee" { autosave::text_of(&v) } else { def.display(&v) };
        let field = DetField {
            label: def.label,
            id: format!("of-{}", def.name),
            req: def.name == kind.name_col(),
            f: det_field(kind, id, def.name, shown),
            gap: def.name.starts_with("cost_m") || def.name == "cost_affiliate",
        };
        match def.name {
            n if n == kind.name_col() || n.contains("description") || n.contains("requirements") => left.push(field),
            _ => right.push(field),
        }
    }
    Ok((left, right))
}

/// The UI-19 dialog of one calendar row.
pub struct FechaDialog {
    pub uuid: String,
    pub half: usize,
    pub title: String,
    pub fields: Vec<(String, &'static str, FieldView)>,
    pub enrolled: String,
    pub days: Vec<DayRow>,
    pub has_days: bool,
    pub day_count_note: String,
    pub can_add: bool,
    pub add_error: Option<String>,
    pub add_date: String,
    pub professors: Vec<(String, String)>,
    pub has_note: bool,
}

pub struct DayRow {
    pub uuid: String,
    pub date: String,
    pub kind: String,
    pub professor: String,
    pub note: String,
    pub n: usize,
}

impl DayRow {
    pub fn tag(&self) -> &'static str {
        if self.kind == "Día de Fiesta" {
            " warn"
        } else {
            " ok"
        }
    }
}

fn cal_field(kind: Kind, id: &str, name: &str, saved: String, profs: &[(String, String)]) -> FieldView {
    let control = match name {
        "status" => Control::Select(pairs(CALENDAR_STATUSES)),
        "cost_member" | "cost_individual" => Control::Money,
        "quorum" | "hours" => Control::Number,
        "professor_uuid" => {
            let mut o = vec![(String::new(), "Sin profesor".to_owned())];
            o.extend(profs.iter().cloned());
            Control::Select(o)
        }
        _ => Control::Text,
    };
    let f = FieldView::new(format!("/calendar/{}/{id}", kind.slug()), name, &format!("fc-{name}"), control, saved);
    if name == "date" {
        f.hint("m/d/aaaa")
    } else {
        f
    }
}

fn day_rows(conn: &Connection, kind: Kind, cal: &str) -> rusqlite::Result<Vec<DayRow>> {
    let sql = match kind {
        Kind::Course => {
            "SELECT d.uuid, d.date, d.status, p.name, d.note FROM calendar_course_dates d
               LEFT JOIN professors p ON p.uuid = d.professor_uuid WHERE d.calendar_course_uuid = ?1 ORDER BY d.date"
        }
        Kind::ShortCourse => {
            "SELECT d.uuid, d.date, COALESCE(d.status, 'Día de Clase'), p.name, NULL FROM calendar_short_course_dates d
               LEFT JOIN professors p ON p.uuid = d.professor_uuid WHERE d.calendar_short_course_uuid = ?1 ORDER BY d.date"
        }
        Kind::Seminar => return Ok(Vec::new()),
    };
    let mut n = 0;
    conn.prepare(sql)?
        .query_map([cal], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })?
        .map(|row| {
            row.map(|(uuid, date, k, p, note)| {
                n += 1;
                DayRow {
                    uuid,
                    date: date_txt(&Some(date)),
                    kind: k,
                    professor: p.unwrap_or_default(),
                    note: note.unwrap_or_default(),
                    n,
                }
            })
        })
        .collect()
}

fn fecha_dialog(
    conn: &Connection,
    kind: Kind,
    cal: &CalendarRow,
    add_error: Option<String>,
    add_date: String,
) -> Result<FechaDialog, AppError> {
    let profs = professors(conn)?;
    let reg = Resource::Calendar(kind).registry();
    let mut fields = Vec::new();
    for def in reg.fields {
        let v = stored(conn, reg.table, def.column, &cal.uuid)?;
        let shown = if def.name == "professor_uuid" { autosave::text_of(&v) } else { def.display(&v) };
        fields.push((format!("fc-{}", def.name), def.label, cal_field(kind, &cal.uuid, def.name, shown, &profs)));
    }
    let days = day_rows(conn, kind, &cal.uuid)?;
    let (can_add, note) = match kind {
        Kind::Course => (true, format!("{} fecha{}", days.len(), if days.len() == 1 { "" } else { "s" })),
        Kind::ShortCourse => {
            ((days.len() as i64) < MAX_SHORT_DATES, format!("{} de {MAX_SHORT_DATES} fechas", days.len()))
        }
        Kind::Seminar => (false, String::new()),
    };
    let date = cal.first_date.as_ref().map(|d| date_txt(&Some(d.clone()))).unwrap_or_else(|| "sin fecha".into());
    Ok(FechaDialog {
        uuid: cal.uuid.clone(),
        half: fields.len().div_ceil(2),
        title: format!("Fecha del {} · {date}", if kind == Kind::Seminar { "seminario" } else { "curso" }),
        fields,
        enrolled: enrolled_line(cal),
        has_days: kind != Kind::Seminar,
        days,
        day_count_note: note,
        can_add,
        add_error,
        add_date,
        professors: profs,
        has_note: kind == Kind::Course,
    })
}

#[derive(Template)]
#[template(path = "school/offerings_enrollments/offering.html", blocks = ["content", "pagebar"])]
pub struct OfferingPage {
    pub shell: Shell,
    pub kind: Kind,
    pub switch: Vec<(String, &'static str, bool)>,
    pub options: Vec<(String, String)>,
    pub uuid: String,
    pub name: String,
    pub sub: String,
    pub name_label: &'static str,
    pub new_label: &'static str,
    pub rows: Vec<CalRowView>,
    pub left: Vec<DetField>,
    pub right: Vec<DetField>,
    pub fecha: Option<FechaDialog>,
    pub open: Option<String>,
    pub tab_det: bool,
    pub new_error: Option<String>,
    pub new_name: String,
    pub delete_error: Option<String>,
    pub notice: Option<&'static str>,
}

impl OfferingPage {
    pub fn has_offering(&self) -> bool {
        !self.uuid.is_empty()
    }
    pub fn slug(&self) -> &'static str {
        self.kind.slug()
    }
    pub fn is_course(&self) -> bool {
        self.kind == Kind::Course
    }
    pub fn is_seminar(&self) -> bool {
        self.kind == Kind::Seminar
    }
    pub fn day_types(&self) -> &'static [&'static str] {
        DAY_TYPES
    }
    pub fn open_label(&self) -> &'static str {
        if self.kind == Kind::Seminar {
            "Abrir seminario"
        } else {
            "Abrir curso"
        }
    }
}

impl OobPage for OfferingPage {
    fn content(&self) -> askama::Result<String> {
        self.as_content().render()
    }
    fn pagebar(&self) -> askama::Result<String> {
        self.as_pagebar().render()
    }
}

impl Page for OfferingPage {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

fn offering_page_of(
    conn: &Connection,
    user: &CurrentUser,
    kind: Kind,
    id: Option<&str>,
    open_cal: Option<&str>,
) -> Result<OfferingPage, AppError> {
    let mut shell = Shell::new(user, "escuela", "ofrecimientos");
    shell.autosave = true;
    let options = offering_options(conn, kind)?;
    let id = match id {
        Some(i) => Some(i.to_owned()),
        None => default_offering(conn, kind)?,
    };
    let mut page = OfferingPage {
        shell,
        kind,
        switch: kind_switch("/offerings", kind),
        options,
        uuid: String::new(),
        name: kind.label().to_owned(),
        sub: String::new(),
        name_label: kind_name_label(kind),
        new_label: kind_new_label(kind),
        rows: Vec::new(),
        left: Vec::new(),
        right: Vec::new(),
        fecha: None,
        open: None,
        tab_det: false,
        new_error: None,
        new_name: String::new(),
        delete_error: None,
        notice: None,
    };
    let Some(id) = id else { return Ok(page) };
    let o = offering_info(conn, kind, &id)?;
    page.sub = offering_sub(conn, kind, &o)?;
    page.rows = offerings::calendar_of(conn, kind, &id)?.iter().map(|r| cal_row_view(kind, r, open_cal)).collect();
    let (left, right) = det_fields(conn, kind, &id)?;
    page.left = left;
    page.right = right;
    page.uuid = o.uuid;
    page.name = o.name;
    Ok(page)
}

#[derive(Deserialize, Default)]
struct OfferingQuery {
    o: Option<String>,
    tab: Option<String>,
    hecho: Option<String>,
}

async fn offerings_index(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path(kind): Path<String>,
    Query(q): Query<OfferingQuery>,
) -> Result<Response, AppError> {
    let kind = kind_of(&kind)?;
    let conn = state.db();
    let mut page = offering_page_of(&conn, &staff.0, kind, q.o.as_deref().filter(|s| !s.is_empty()), None)?;
    if q.hecho.as_deref() == Some("borrado") {
        page.notice = Some("Ofrecimiento borrado.");
    }
    Ok(render_oob(StatusCode::OK, &headers, &page))
}

async fn offering_page(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, String)>,
    Query(q): Query<OfferingQuery>,
) -> Result<Response, AppError> {
    let kind = kind_of(&kind)?;
    let mut page = offering_page_of(&state.db(), &staff.0, kind, Some(&id), None)?;
    page.tab_det = q.tab.as_deref() == Some("det");
    Ok(render_oob(StatusCode::OK, &headers, &page))
}

#[derive(Deserialize)]
struct NewOfferingBody {
    #[serde(default)]
    name: String,
    #[serde(default)]
    member_price: String,
    #[serde(default)]
    individual_price: String,
}

async fn offering_create(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path(kind): Path<String>,
    Form(f): Form<NewOfferingBody>,
) -> Result<Response, AppError> {
    let kind = kind_of(&kind)?;
    let who = Who::of(&staff.0);
    let o = offerings::NewOffering {
        name: f.name.clone(),
        member_price: f.member_price.clone(),
        individual_price: f.individual_price.clone(),
    };
    let res = {
        let mut conn = state.db();
        let tx = school::begin(&mut conn)?;
        let r = offerings::create_offering(&tx, &who, kind, &o);
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    match res {
        Ok(uuid) => Ok(go(&headers, &format!("/offerings/{}/{uuid}?tab=det", kind.slug()))),
        Err(e @ DomainError::Validation(..)) => {
            let mut page = offering_page_of(&state.db(), &staff.0, kind, None, None)?;
            page.new_error = Some(e.message());
            page.new_name = f.name;
            page.open = Some("dlg-nuevo".into());
            Ok(render_page_status(StatusCode::UNPROCESSABLE_ENTITY, &headers, &page))
        }
        Err(e) => Ok(e.into_response()),
    }
}

async fn offering_patch(
    State(state): State<AppState>,
    staff: RequireStaff,
    r: Option<Extension<RequestId>>,
    Path((kind, id)): Path<(String, String)>,
    field: OneField,
) -> Result<Response, AppError> {
    let kind = kind_of(&kind)?;
    let who = Who::of(&staff.0);
    let mut conn = state.db();
    let res = Resource::Offering(kind);
    let reg = res.registry();
    let Some(def) = reg.get(&field.name) else {
        return Ok(autosave::contract_error("Este campo no se puede cambiar aquí."));
    };
    let outcome = match sreg::patch(&mut conn, &who, rid(&r), res, &id, &field.name, &field.value)? {
        Patched::Unknown => return Ok(autosave::contract_error("Este campo no se puede cambiar aquí.")),
        Patched::Done(o) => o,
    };
    let show = |v: &Value| if def.name == "nmls_fee" { autosave::text_of(v) } else { def.display(v) };
    match outcome {
        Outcome::Saved { new, changed, .. } => {
            let mut extra = String::new();
            if changed {
                let o = offering_info(&conn, kind, &id)?;
                extra = autosave::oob(
                    "of-title",
                    &format!(
                        "<b>{}</b> <span class=\"sub\">{}</span>",
                        esc(&o.name),
                        esc(&offering_sub(&conn, kind, &o)?)
                    ),
                ) + &autosave::oob("of-crumb", &esc(&o.name));
            }
            Ok(autosave::respond(&det_field(kind, &id, def.name, show(&new)), &extra))
        }
        Outcome::Invalid(m) => {
            let old = stored(&conn, reg.table, def.column, &id)?;
            Ok(autosave::respond(&det_field(kind, &id, def.name, show(&old)).refused(&field.value, m), ""))
        }
    }
}

async fn offering_delete(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, String)>,
) -> Result<Response, AppError> {
    let kind = kind_of(&kind)?;
    let who = Who::of(&staff.0);
    let res = {
        let mut conn = state.db();
        let tx = school::begin(&mut conn)?;
        let r = offerings::delete_offering(&tx, &who, kind, &id);
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    match res {
        Ok(()) => Ok(go(&headers, &format!("/offerings/{}?hecho=borrado", kind.slug()))),
        Err(e @ (DomainError::HasCalendar(_) | DomainError::HasEnrollments(_) | DomainError::InUse(_))) => {
            let mut page = offering_page_of(&state.db(), &staff.0, kind, Some(&id), None)?;
            page.delete_error = Some(match e {
                DomainError::HasCalendar(n) => format!(
                    "Tiene {n} fecha{} en el calendario. Un ofrecimiento con fechas no se borra (AP-04).",
                    if n == 1 { "" } else { "s" }
                ),
                e => e.message(),
            });
            page.open = Some("dlg-borrar".into());
            Ok(render_page_status(StatusCode::CONFLICT, &headers, &page))
        }
        Err(e) => Ok(e.into_response()),
    }
}

async fn calendar_open(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, String)>,
) -> Result<Response, AppError> {
    let kind = kind_of(&kind)?;
    let who = Who::of(&staff.0);
    let res = {
        let mut conn = state.db();
        let tx = school::begin(&mut conn)?;
        let r = offerings::open_calendar(&tx, &who, kind, &id, &offerings::NewCalendar::default());
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    match res {
        Ok(cal) => Ok(go(&headers, &format!("/calendar/{}/{cal}", kind.slug()))),
        Err(e) => Ok(e.into_response()),
    }
}

// ===============================================================================================================
// UI-19 Fecha del curso
// ===============================================================================================================

fn calendar_offering(conn: &Connection, kind: Kind, cal: &str) -> Result<String, AppError> {
    conn.query_row(
        &format!("SELECT {} FROM {} WHERE uuid = ?1", kind.calendar_offering_col(), kind.calendar_table()),
        [cal],
        |r| r.get(0),
    )
    .optional()?
    .ok_or(AppError::NotFound)
}

fn fecha_page(
    conn: &Connection,
    user: &CurrentUser,
    kind: Kind,
    cal: &str,
    add_error: Option<String>,
    add_date: String,
) -> Result<OfferingPage, AppError> {
    let offering = calendar_offering(conn, kind, cal)?;
    let row = offerings::calendar_row(conn, kind, cal)?.ok_or(AppError::NotFound)?;
    let mut page = offering_page_of(conn, user, kind, Some(&offering), Some(cal))?;
    page.fecha = Some(fecha_dialog(conn, kind, &row, add_error, add_date)?);
    page.open = Some("dlg-fecha".into());
    Ok(page)
}

async fn calendar_dialog(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, String)>,
) -> Result<Response, AppError> {
    let kind = kind_of(&kind)?;
    let page = fecha_page(&state.db(), &staff.0, kind, &id, None, String::new())?;
    Ok(render_page_status(StatusCode::OK, &headers, &page))
}

async fn calendar_patch(
    State(state): State<AppState>,
    staff: RequireStaff,
    r: Option<Extension<RequestId>>,
    Path((kind, id)): Path<(String, String)>,
    field: OneField,
) -> Result<Response, AppError> {
    let kind = kind_of(&kind)?;
    let who = Who::of(&staff.0);
    let mut conn = state.db();
    let res = Resource::Calendar(kind);
    let reg = res.registry();
    let Some(def) = reg.get(&field.name) else {
        return Ok(autosave::contract_error("Este campo no se puede cambiar aquí."));
    };
    let outcome = match sreg::patch(&mut conn, &who, rid(&r), res, &id, &field.name, &field.value)? {
        Patched::Unknown => return Ok(autosave::contract_error("Este campo no se puede cambiar aquí.")),
        Patched::Done(o) => o,
    };
    let profs = professors(&conn)?;
    let show = |v: &Value| if def.name == "professor_uuid" { autosave::text_of(v) } else { def.display(v) };
    match outcome {
        Outcome::Saved { new, .. } => {
            let row = offerings::calendar_row(&conn, kind, &id)?.ok_or(AppError::NotFound)?;
            Ok(autosave::respond(&cal_field(kind, &id, def.name, show(&new), &profs), &cal_row_oob(kind, &row)))
        }
        Outcome::Invalid(m) => {
            let old = stored(&conn, reg.table, def.column, &id)?;
            Ok(autosave::respond(&cal_field(kind, &id, def.name, show(&old), &profs).refused(&field.value, m), ""))
        }
    }
}

#[derive(Deserialize)]
struct DateBody {
    #[serde(default)]
    date: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    professor: String,
    #[serde(default)]
    note: String,
}

async fn date_add(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, String)>,
    Form(f): Form<DateBody>,
) -> Result<Response, AppError> {
    let kind = kind_of(&kind)?;
    let who = Who::of(&staff.0);
    let iso = match parse_date(&f.date) {
        Ok(Value::Text(t)) => Ok(t),
        Ok(_) => Err(DomainError::Validation("date", "Escriba la fecha (m/d/aaaa).".into())),
        Err(_) => Err(DomainError::Validation("date", "Fecha no válida (m/d/aaaa).".into())),
    };
    let res = match iso {
        Err(e) => Err(e),
        Ok(date) => {
            let mut conn = state.db();
            let tx = school::begin(&mut conn)?;
            let prof = Some(f.professor.trim()).filter(|p| !p.is_empty());
            let note = Some(f.note.trim()).filter(|n| !n.is_empty());
            let r = offerings::add_class_date(&tx, &who, kind, &id, &date, prof).and_then(|d| {
                let t = if f.status.trim().is_empty() { "Día de Clase" } else { f.status.trim() };
                offerings::describe_class_date(&tx, kind, &d, t, note)
            });
            if r.is_ok() {
                tx.commit()?;
            }
            r
        }
    };
    match res {
        Ok(()) => Ok(go(&headers, &format!("/calendar/{}/{id}", kind.slug()))),
        Err(DomainError::NotFound) => Err(AppError::NotFound),
        Err(e @ (DomainError::Validation(..) | DomainError::LimitReached(_))) => {
            let (status, msg) = match &e {
                DomainError::LimitReached(n) => {
                    (StatusCode::CONFLICT, format!("Un curso corto tiene hasta {n} fechas."))
                }
                e => (StatusCode::UNPROCESSABLE_ENTITY, e.message()),
            };
            let page = fecha_page(&state.db(), &staff.0, kind, &id, Some(msg), f.date)?;
            Ok(render_page_status(status, &headers, &page))
        }
        Err(e) => Ok(e.into_response()),
    }
}

async fn date_remove(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path((kind, id, date_id)): Path<(String, String, String)>,
) -> Result<Response, AppError> {
    let kind = kind_of(&kind)?;
    let who = Who::of(&staff.0);
    let res = {
        let mut conn = state.db();
        let tx = school::begin(&mut conn)?;
        let r = offerings::remove_class_date(&tx, &who, kind, &date_id);
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    match res {
        Ok(()) => Ok(go(&headers, &format!("/calendar/{}/{id}", kind.slug()))),
        Err(e) => Ok(e.into_response()),
    }
}

// ===============================================================================================================
// UI-20 Matrícula
// ===============================================================================================================

/// The student line and price basis of an enrollment dialog.
struct StudentInfo {
    uuid: String,
    name: String,
    company: Option<String>,
    affiliate: Option<(String, Option<String>)>,
    member: bool,
}

fn student_info(conn: &Connection, id: &str) -> Result<StudentInfo, AppError> {
    let (name, company, socio, cust): (String, Option<String>, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT s.nombre, s.compania, a.socio, a.cust_type FROM students s LEFT JOIN affiliates a ON a.uuid = s.member_uuid WHERE s.uuid = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    let member = students::is_affiliate_student(conn, id).map_err(dom)?;
    Ok(StudentInfo { uuid: id.to_owned(), name, company, affiliate: socio.map(|s| (s, cust)), member })
}

impl StudentInfo {
    fn status_text(&self) -> String {
        match &self.affiliate {
            Some((_, Some(c))) if c == "ACTIVO" || c == "INACTIVO" => format!("Afiliado {c}"),
            Some(_) => "Afiliado no activo".into(),
            None => "Sin afiliado".into(),
        }
    }
    fn line(&self) -> String {
        let mut s = self.name.clone();
        if let Some(c) = self.company.as_ref().filter(|c| !c.trim().is_empty()) {
            s.push_str(" · ");
            s.push_str(c);
        }
        s.push_str(" · ");
        s.push_str(&self.status_text());
        s
    }
    /// "Afiliado ACTIVO: precio de socio" / "Afiliado INACTIVO: precio individual" / "Sin afiliado: precio individual".
    fn why(&self) -> String {
        format!("{}: precio {}", self.status_text(), if self.member { "de socio" } else { "individual" })
    }
    fn affiliate_name(&self) -> Option<String> {
        self.affiliate.as_ref().map(|(s, _)| s.clone())
    }
}

/// The price card of UI-20 (BR-ENR-01).
#[derive(Default, Template)]
#[template(path = "school/offerings_enrollments/price_card.html")]
pub struct PriceCard {
    pub oob: bool,
    pub class: &'static str,
    pub k: String,
    pub v: String,
    pub why: String,
    pub why_ok: bool,
    pub affiliate: Option<String>,
    pub alt: Option<String>,
    pub note: String,
}

/// The Fecha field of UI-20 with the price card (the `/calendar-options` fragment).
#[derive(Template)]
#[template(path = "school/offerings_enrollments/fecha_pick.html")]
pub struct FechaPick {
    pub kind: Kind,
    pub options: Vec<(String, String)>,
    pub selected: String,
    pub disabled: bool,
    pub empty: bool,
    pub error: Option<String>,
    pub price: PriceCard,
    pub oob: bool,
}

impl PriceCard {
    /// The card in the dialog.
    pub fn html(&self) -> String {
        self.render().unwrap_or_default()
    }
    /// The card out of band (the `/calendar-options` response).
    pub fn oob_html(&self) -> String {
        PriceCard { oob: true, ..self.clone_card() }.render().unwrap_or_default()
    }
    fn clone_card(&self) -> PriceCard {
        PriceCard {
            oob: self.oob,
            class: self.class,
            k: self.k.clone(),
            v: self.v.clone(),
            why: self.why.clone(),
            why_ok: self.why_ok,
            affiliate: self.affiliate.clone(),
            alt: self.alt.clone(),
            note: self.note.clone(),
        }
    }
}

impl FechaPick {
    pub fn is_sel(&self, v: &str) -> bool {
        self.selected == v
    }
}

fn option_label(kind: Kind, o: &offerings::CalendarOption, semester: Option<&str>) -> String {
    let d = o.first_date.as_ref().map(|d| date_txt(&Some(d.clone()))).unwrap_or_else(|| "Sin fecha".into());
    match (kind, semester) {
        (Kind::Course, Some(s)) if !s.is_empty() => format!("{d} · {} · {s}", o.status),
        _ => format!("{d} · {}", o.status),
    }
}

fn semester_of(conn: &Connection, kind: Kind, cal: &str) -> rusqlite::Result<Option<String>> {
    if kind != Kind::Course {
        return Ok(None);
    }
    conn.query_row("SELECT semester FROM calendar_course WHERE uuid = ?1", [cal], |r| r.get(0))
        .optional()
        .map(Option::flatten)
}

/// The Fecha select and price card for a new enrollment: active rows only (BR-CRS-01) and the price this student
/// pays (BR-ENR-01). Without a student, the card shows both prices.
fn new_pick(
    conn: &Connection,
    kind: Kind,
    offering: &str,
    student: Option<&StudentInfo>,
    selected: &str,
    error: Option<String>,
) -> Result<FechaPick, AppError> {
    let opts = match (offering.is_empty(), student) {
        (true, _) => Vec::new(),
        (false, Some(s)) => offerings::calendar_options(conn, kind, offering, &s.uuid).map_err(dom)?,
        (false, None) => {
            // No student yet: the rows are the same; the price is decided when the student is picked.
            let any: Option<String> =
                conn.query_row("SELECT uuid FROM students LIMIT 1", [], |r| r.get(0)).optional()?;
            match any {
                Some(a) => offerings::calendar_options(conn, kind, offering, &a).map_err(dom)?,
                None => Vec::new(),
            }
        }
    };
    let mut options = Vec::new();
    for o in &opts {
        options.push((o.uuid.clone(), option_label(kind, o, semester_of(conn, kind, &o.uuid)?.as_deref())));
    }
    let o_prices = if offering.is_empty() { None } else { Some(offering_info(conn, kind, offering)?) };
    let both = o_prices
        .as_ref()
        .map(|o| format!("Socio {} · individual {}", usd_opt(o.member), usd_opt(o.individual)))
        .unwrap_or_default();
    let chosen = opts.iter().find(|o| o.uuid == selected);
    let price = match (chosen, student) {
        (Some(_), Some(s)) => {
            let row = offerings::calendar_row(conn, kind, selected)?.ok_or(AppError::NotFound)?;
            let (pay, other, other_label) = if s.member {
                (row.cost_member, row.cost_individual, "Individual")
            } else {
                (row.cost_individual, row.cost_member, "Socio")
            };
            PriceCard {
                class: if s.member { "" } else { " ind" },
                k: format!("Costo · precio {}", if s.member { "de socio" } else { "individual" }),
                v: usd_opt(pay),
                why: s.why(),
                why_ok: s.member,
                affiliate: s.affiliate_name(),
                alt: Some(format!("{other_label} {} · no aplica", usd_opt(other))),
                note: "Se guarda al escoger la fecha (BR-ENR-01).".into(),
                oob: false,
            }
        }
        (Some(_), None) => PriceCard {
            class: " none",
            k: "Costo".into(),
            v: "—".into(),
            why: "Escoja el estudiante para ver su precio.".into(),
            note: both,
            ..Default::default()
        },
        _ => PriceCard {
            class: " none",
            k: "Costo".into(),
            v: "—".into(),
            why: "Escoja una fecha para ver el precio.".into(),
            note: both,
            ..Default::default()
        },
    };
    Ok(FechaPick {
        kind,
        empty: !offering.is_empty() && options.is_empty(),
        disabled: options.is_empty(),
        selected: if chosen.is_some() { selected.to_owned() } else { String::new() },
        options,
        error,
        price,
        oob: false,
    })
}

/// One grade input of the Notas section.
pub struct GradeInput {
    pub name: &'static str,
    pub label: &'static str,
    pub aria: Option<&'static str>,
    pub value: String,
    pub error: Option<String>,
}

/// The typed values of the dialog (create, settle, or a refusal re-render).
#[derive(Default, Clone)]
struct EnrValues {
    offering: String,
    calendar: String,
    student: String,
    status: String,
    costeado_por: String,
    pago: String,
    fecha_de_pago: String,
    check_number: String,
    expectations: String,
    notes: String,
    grades: Vec<String>,
    puntuacion: String,
    next: String,
}

impl EnrValues {
    fn from_pairs(p: &[(String, String)]) -> Self {
        let m: HashMap<&str, &str> = p.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let g = |k: &str| m.get(k).map(|s| (*s).to_owned()).unwrap_or_default();
        EnrValues {
            offering: g("offering"),
            calendar: g("calendar"),
            student: g("student"),
            status: g("status"),
            costeado_por: g("costeado_por"),
            pago: g("pago"),
            fecha_de_pago: g("fecha_de_pago"),
            check_number: g("check_number"),
            expectations: g("expectations"),
            notes: g("notes"),
            grades: enrollments::GRADE_COLUMNS.iter().map(|c| g(c)).collect(),
            puntuacion: g("puntuacion"),
            next: g("next"),
        }
    }
}

const GRADE_LABELS: [(&str, Option<&str>); 12] = [
    ("Examen", Some("Examen")),
    ("1", Some("Asignación 1")),
    ("2", Some("Asignación 2")),
    ("3", Some("Asignación 3")),
    ("4", Some("Asignación 4")),
    ("5", Some("Asignación 5")),
    ("6", Some("Asignación 6")),
    ("7", Some("Asignación 7")),
    ("8", Some("Asignación 8")),
    ("Promedio asignaciones", None),
    ("Asistencia y puntualidad", None),
    ("Promedio final", None),
];

/// An existing enrollment as stored: values, invoice text, created date, payment, stored price.
type StoredEnr = (EnrValues, String, String, i64, Option<i64>, String);
/// A student's enrollment line: (uuid, offering, status, cost, balance).
type StudentLine = (String, String, String, String, String);
/// A calendar pick: (uuid, label, first date, offering name, active, enrollments).
type CalChoice = (String, String, Option<String>, String, bool, i64);

/// What the form fields failed on: (field name, label, message).
type FieldErrors = Vec<(String, &'static str, String)>;

/// Parses and checks the settle form (the server owns the rules: BR-ENR-03 range, money, dates, value lists).
fn parse_settle(kind: Kind, v: &EnrValues) -> (enrollments::Settle, FieldErrors) {
    let mut errs: FieldErrors = Vec::new();
    let text = |s: &str| Some(s.trim().to_owned()).filter(|t| !t.is_empty());
    let pago = if v.pago.trim().starts_with('-') {
        errs.push(("pago".into(), "Pago", "El pago no puede ser negativo.".into()));
        0
    } else if v.pago.trim().is_empty() {
        0
    } else {
        match parse_money(&v.pago) {
            Ok(Value::Integer(c)) => c,
            _ => {
                errs.push(("pago".into(), "Pago", "Escriba una cantidad en dólares, por ejemplo 350.00.".into()));
                0
            }
        }
    };
    let fecha = match parse_date(&v.fecha_de_pago) {
        Ok(Value::Text(t)) => Some(t),
        Ok(_) => None,
        Err(_) => {
            errs.push(("fecha_de_pago".into(), "Fecha de pago", "Fecha no válida (m/d/aaaa).".into()));
            None
        }
    };
    let statuses = if kind == Kind::Course { enrollments::COURSE_STATUSES } else { enrollments::IRREGULAR_STATUSES };
    let status = text(&v.status);
    if status.as_deref().is_some_and(|s| !statuses.contains(&s)) || (kind == Kind::Course && status.is_none()) {
        errs.push(("status".into(), "Estatus", "Escoja un estatus de la lista.".into()));
    }
    let payer = text(&v.costeado_por);
    if payer.as_deref().is_some_and(|p| !enrollments::PAYERS.contains(&p)) {
        errs.push(("costeado_por".into(), "Costeado por", "Escoja COMPAÑIA o INDIVIDUAL.".into()));
    }
    let mut grades_out = Vec::new();
    let mut score = None;
    if kind == Kind::Course {
        for (i, raw) in v.grades.iter().enumerate() {
            match sreg::grade(raw) {
                Ok(Value::Real(f)) => grades_out.push(Some(f)),
                Ok(Value::Integer(n)) => grades_out.push(Some(n as f64)),
                Ok(_) => grades_out.push(None),
                Err(_) => {
                    let label = if i == 11 { "Promedio final" } else { GRADE_LABELS[i].1.unwrap_or(GRADE_LABELS[i].0) };
                    errs.push((enrollments::GRADE_COLUMNS[i].into(), label, "Va de 0 a 100.".into()));
                    grades_out.push(None);
                }
            }
        }
        match sreg::score(&v.puntuacion) {
            Ok(Value::Integer(n)) => score = Some(n),
            Ok(_) => {}
            Err(_) => errs.push(("puntuacion".into(), "Puntuación", "La puntuación va de 0 a 100.".into())),
        }
    }
    let s = enrollments::Settle {
        status,
        costeado_por: payer,
        pago,
        fecha_de_pago: fecha,
        check_number: if kind == Kind::Course { None } else { text(&v.check_number) },
        expectations: text(&v.expectations),
        notes: text(&v.notes),
        grades: grades_out,
        puntuacion: score,
    };
    (s, errs)
}

/// The UI-20 dialog.
#[derive(Template)]
#[template(path = "school/offerings_enrollments/matricula.html")]
pub struct EnrDialog {
    pub kind: Kind,
    pub csrf: String,
    pub is_admin: bool,
    pub title: String,
    pub action: String,
    pub is_new: bool,
    pub student_line: Option<String>,
    pub student_hidden: String,
    pub student_options: Vec<(String, String)>,
    pub offering_label: &'static str,
    pub offerings: Vec<(String, String)>,
    pub offering: String,
    pub pick: FechaPick,
    pub statuses: Vec<String>,
    pub v_status: String,
    pub v_payer: String,
    pub v_pago: String,
    pub v_fecha: String,
    pub v_check: String,
    pub invoice: String,
    pub balance: String,
    pub v_expect: String,
    pub v_notes: String,
    pub notes_label: &'static str,
    pub grades: Vec<GradeInput>,
    pub grades2: Vec<GradeInput>,
    pub v_score: String,
    pub score_error: Option<String>,
    pub letter: Option<&'static str>,
    pub passed: bool,
    pub errors: FieldErrors,
    pub paid: i64,
    pub uuid: String,
    pub delete_title: String,
    pub next: String,
    pub cancelled: bool,
}

impl EnrDialog {
    pub fn is_course(&self) -> bool {
        self.kind == Kind::Course
    }
    pub fn is_sel_off(&self, v: &str) -> bool {
        self.offering == v
    }
    pub fn is_sel_stu(&self, v: &str) -> bool {
        self.student_hidden == v
    }
    pub fn err(&self, name: &str) -> Option<String> {
        self.errors.iter().find(|(n, _, _)| n == name).map(|(_, _, m)| m.clone())
    }
    pub fn picker_url(&self) -> String {
        format!("/calendar-options/{}", self.kind.slug())
    }
    pub fn errors_len(&self) -> usize {
        self.errors.len()
    }
    pub fn usd(&self, c: &i64) -> String {
        usd(*c)
    }
}

fn status_choices(kind: Kind) -> Vec<String> {
    let l = if kind == Kind::Course { enrollments::COURSE_STATUSES } else { enrollments::IRREGULAR_STATUSES };
    l.iter().map(|s| (*s).to_owned()).collect()
}

fn grade_inputs(v: &EnrValues, errs: &FieldErrors) -> (Vec<GradeInput>, Vec<GradeInput>) {
    let mut a = Vec::new();
    let mut b = Vec::new();
    for (i, col) in enrollments::GRADE_COLUMNS.iter().enumerate() {
        let gi = GradeInput {
            name: col,
            label: GRADE_LABELS[i].0,
            aria: GRADE_LABELS[i].1,
            value: v.grades.get(i).cloned().unwrap_or_default(),
            error: errs.iter().find(|(n, _, _)| n == col).map(|(_, _, m)| m.clone()),
        };
        if i < 9 {
            a.push(gi);
        } else {
            b.push(gi);
        }
    }
    (a, b)
}

/// The live letter of the dialog: the final average decides the score when set (BR-ENR-03, -04, -05).
fn letter_of(v: &EnrValues) -> Option<i64> {
    let final_avg = v
        .grades
        .get(11)
        .and_then(|g| g.trim().replace(',', ".").parse::<f64>().ok())
        .filter(|f| (0.0..=100.0).contains(f));
    final_avg
        .map(|f| f.round() as i64)
        .or_else(|| v.puntuacion.trim().parse::<i64>().ok().filter(|s| (0..=100).contains(s)))
}

/// The balance a new enrollment would start with: the shown price minus the typed payment (BR-ENR-02).
fn new_balance(pick: &FechaPick, pago: &str) -> String {
    let Ok(price) = autosave::cents(&pick.price.v) else { return "—".into() };
    let paid = if pago.trim().is_empty() { Ok(0) } else { autosave::cents(pago) };
    match paid {
        Ok(p) => usd(price - p),
        Err(_) => "—".into(),
    }
}

/// A new-enrollment dialog (from UI-15 with the student fixed, or from a calendar row with a student pick).
fn new_dialog(
    conn: &Connection,
    user: &CurrentUser,
    kind: Kind,
    student: Option<&str>,
    v: &EnrValues,
    errs: FieldErrors,
    action: String,
) -> Result<EnrDialog, AppError> {
    let info = match student.filter(|s| !s.is_empty()) {
        Some(s) => Some(student_info(conn, s)?),
        None => None,
    };
    let offerings_list = offering_options(conn, kind)?;
    let offering =
        if v.offering.is_empty() { default_offering(conn, kind)?.unwrap_or_default() } else { v.offering.clone() };
    let cal_err = errs.iter().find(|(n, _, _)| n == "calendar").map(|(_, _, m)| m.clone());
    let mut pick = new_pick(conn, kind, &offering, info.as_ref(), &v.calendar, cal_err.clone())?;
    // A fresh dialog for a known student starts on the soonest active date, so the price shows at once.
    if errs.is_empty() && v.calendar.is_empty() && info.is_some() {
        if let Some(first) = pick.options.first().map(|o| o.0.clone()) {
            pick = new_pick(conn, kind, &offering, info.as_ref(), &first, None)?;
        }
    }
    let (grades, grades2) = grade_inputs(v, &errs);
    let score = letter_of(v);
    let score_error = errs.iter().find(|(n, _, _)| n == "puntuacion").map(|(_, _, m)| m.clone());
    let from_calendar = student.is_none();
    let student_options = if from_calendar {
        conn.prepare("SELECT uuid, nombre FROM students ORDER BY nombre")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?
    } else {
        Vec::new()
    };
    let default_payer = match &info {
        Some(i) if i.member => "COMPAÑIA",
        Some(_) => "INDIVIDUAL",
        None => "COMPAÑIA",
    };
    let balance = new_balance(&pick, &v.pago);
    Ok(EnrDialog {
        kind,
        csrf: user.csrf.clone(),
        is_admin: false,
        title: match &info {
            Some(i) => format!("Matricular en {} · {}", kind_singular(kind).trim_end_matches(" especializado"), i.name),
            None => format!("Matricular en {}", kind_singular(kind).trim_end_matches(" especializado")),
        },
        action,
        is_new: true,
        student_line: info.as_ref().map(StudentInfo::line),
        student_hidden: info.as_ref().map(|i| i.uuid.clone()).unwrap_or_else(|| v.student.clone()),
        student_options,
        offering_label: kind_name_label(kind),
        offerings: offerings_list,
        offering,
        pick,
        statuses: status_choices(kind),
        v_status: if v.status.is_empty() { status_choices(kind)[0].clone() } else { v.status.clone() },
        v_payer: if v.costeado_por.is_empty() { default_payer.into() } else { v.costeado_por.clone() },
        v_pago: v.pago.clone(),
        v_fecha: v.fecha_de_pago.clone(),
        v_check: v.check_number.clone(),
        invoice: "Se asigna al guardar".into(),
        balance,
        v_expect: v.expectations.clone(),
        v_notes: v.notes.clone(),
        notes_label: if kind == Kind::Course { "Indicar cambios" } else { "Notas" },
        grades,
        grades2,
        v_score: v.puntuacion.clone(),
        score_error,
        letter: school::letter(score),
        passed: school::passed(score),
        errors: errs,
        paid: 0,
        uuid: String::new(),
        delete_title: String::new(),
        next: v.next.clone(),
        cancelled: false,
    })
}

/// An existing enrollment's values, as stored.
fn stored_values(conn: &Connection, kind: Kind, id: &str) -> Result<StoredEnr, AppError> {
    let t = kind.enrollment_table();
    let off = kind.enrollment_offering_col();
    let cal = kind.enrollment_calendar_col();
    let price = kind.enrollment_price_col();
    let (fecha_col, check_col, notes_col, inv_col) = match kind {
        Kind::Course => ("fecha_de_pago", "NULL", "indicar_cambios", "invoice_num"),
        _ => ("date_of_payment", "check_number", "notas", "invoice_number"),
    };
    let grade_cols = if kind == Kind::Course {
        enrollments::GRADE_COLUMNS.join(", ") + ", puntuacion"
    } else {
        std::iter::repeat_n("NULL", 13).collect::<Vec<_>>().join(", ")
    };
    let sql = format!(
        "SELECT {off}, COALESCE({cal}, ''), student_uuid, COALESCE(status, ''), COALESCE(costeado_por, ''), pago, {fecha_col}, {check_col},
                expectations, {notes_col}, {inv_col}, {price}, created_at, {grade_cols} FROM {t} WHERE uuid = ?1"
    );
    conn.query_row(&sql, [id], |r| {
        let pago: i64 = r.get(5)?;
        let mut grades = Vec::new();
        for i in 0..12 {
            let g: Option<f64> = r.get(13 + i)?;
            grades.push(num_txt(g));
        }
        let score: Option<i64> = r.get(25)?;
        let inv: Option<i64> = r.get(10)?;
        let v = EnrValues {
            offering: r.get(0)?,
            calendar: r.get(1)?,
            student: r.get(2)?,
            status: r.get(3)?,
            costeado_por: r.get(4)?,
            pago: autosave::dollars(pago),
            fecha_de_pago: date_txt(&r.get(6)?),
            check_number: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
            expectations: r.get::<_, Option<String>>(8)?.unwrap_or_default(),
            notes: r.get::<_, Option<String>>(9)?.unwrap_or_default(),
            grades,
            puntuacion: score.map(|s| s.to_string()).unwrap_or_default(),
            next: String::new(),
        };
        let prefix = if kind == Kind::Course { "F" } else { "S" };
        Ok((
            v,
            inv.map(|n| format!("{prefix}-{n}")).unwrap_or_default(),
            date_txt(&r.get::<_, Option<String>>(12)?.map(|s| s[..10.min(s.len())].to_owned())),
            pago,
            r.get(11)?,
            String::new(),
        ))
    })
    .optional()?
    .ok_or(AppError::NotFound)
}

/// The dialog of an existing enrollment (Guardar, Dar de baja, Borrar).
fn edit_dialog(
    conn: &Connection,
    user: &CurrentUser,
    kind: Kind,
    id: &str,
    typed: Option<(&EnrValues, FieldErrors)>,
) -> Result<EnrDialog, AppError> {
    let (stored_v, invoice, created, paid, price, _) = stored_values(conn, kind, id)?;
    let (v, errs) = match typed {
        Some((t, e)) => {
            let mut t = t.clone();
            t.offering = stored_v.offering.clone();
            t.calendar = stored_v.calendar.clone();
            t.student = stored_v.student.clone();
            (t, e)
        }
        None => (stored_v.clone(), Vec::new()),
    };
    let info = student_info(conn, &stored_v.student)?;
    let o = offering_info(conn, kind, &stored_v.offering)?;
    let row =
        if stored_v.calendar.is_empty() { None } else { offerings::calendar_row(conn, kind, &stored_v.calendar)? };
    let label = row.as_ref().map(|r| {
        let d = r.first_date.as_ref().map(|d| date_txt(&Some(d.clone()))).unwrap_or_else(|| "Sin fecha".into());
        match &r.semester {
            Some(s) if !s.is_empty() => format!("{d} · {} · {s}", r.status),
            _ => format!("{d} · {}", r.status),
        }
    });
    let member_price = row
        .as_ref()
        .is_some_and(|r| r.cost_member.is_some() && r.cost_member == price && r.cost_member != r.cost_individual)
        || row.as_ref().is_some_and(|r| r.cost_member == r.cost_individual && info.member);
    let pick = FechaPick {
        kind,
        options: vec![(stored_v.calendar.clone(), label.unwrap_or_else(|| "Sin fecha".into()))],
        selected: stored_v.calendar.clone(),
        disabled: true,
        empty: false,
        error: None,
        price: PriceCard {
            class: if member_price { "" } else { " ind" },
            k: format!("Costo · precio {}", if member_price { "de socio" } else { "individual" }),
            v: usd_opt(price),
            why: format!(
                "Guardado el {created} con {}",
                if member_price { "precio de socio" } else { "precio individual" }
            ),
            why_ok: member_price,
            affiliate: None,
            alt: None,
            note: "No cambia si cambia el afiliado (BR-ENR-01).".into(),
            oob: false,
        },
        oob: false,
    };
    let balance = price.unwrap_or(0) - paid;
    let (grades, grades2) = grade_inputs(&v, &errs);
    let score = letter_of(&v);
    let score_error = errs.iter().find(|(n, _, _)| n == "puntuacion").map(|(_, _, m)| m.clone());
    let cancelled = matches!(stored_v.status.as_str(), "BAJA" | "Canceló");
    Ok(EnrDialog {
        kind,
        csrf: user.csrf.clone(),
        is_admin: user.role == crate::auth::Role::Admin,
        title: format!("Matrícula · {}", o.name),
        action: format!("/enrollments/{}/{id}", kind.slug()),
        is_new: false,
        student_line: Some(info.line()),
        student_hidden: info.uuid.clone(),
        student_options: Vec::new(),
        offering_label: kind_name_label(kind),
        offerings: vec![(o.uuid.clone(), o.name.clone())],
        offering: o.uuid.clone(),
        pick,
        statuses: status_choices(kind),
        v_status: v.status.clone(),
        v_payer: v.costeado_por.clone(),
        v_pago: v.pago.clone(),
        v_fecha: v.fecha_de_pago.clone(),
        v_check: v.check_number.clone(),
        invoice,
        balance: usd(balance),
        v_expect: v.expectations.clone(),
        v_notes: v.notes.clone(),
        notes_label: if kind == Kind::Course { "Indicar cambios" } else { "Notas" },
        grades,
        grades2,
        v_score: v.puntuacion.clone(),
        score_error,
        letter: school::letter(score),
        passed: school::passed(score),
        errors: errs,
        paid,
        uuid: id.to_owned(),
        delete_title: format!(
            "{} · {}",
            o.name,
            row.as_ref().and_then(|r| r.first_date.clone()).map(|d| date_txt(&Some(d))).unwrap_or_default()
        ),
        next: v.next.clone(),
        cancelled,
    })
}

#[derive(Deserialize, Default)]
struct OptionsQuery {
    offering: Option<String>,
    student: Option<String>,
    calendar: Option<String>,
}

/// `GET /calendar-options/{kind}?offering=&student=` (T-05-11): the Fecha field and, out of band, the price card.
async fn calendar_options(
    State(state): State<AppState>,
    _staff: RequireStaff,
    Path(kind): Path<String>,
    Query(q): Query<OptionsQuery>,
) -> Result<Response, AppError> {
    let kind = kind_of(&kind)?;
    let conn = state.db();
    let student = match q.student.as_deref().filter(|s| !s.is_empty()) {
        Some(s) => Some(student_info(&conn, s)?),
        None => None,
    };
    let offering = q.offering.unwrap_or_default();
    if !offering.is_empty() {
        offering_info(&conn, kind, &offering)?;
    }
    let mut pick = new_pick(&conn, kind, &offering, student.as_ref(), q.calendar.as_deref().unwrap_or_default(), None)?;
    if pick.selected.is_empty() && pick.options.len() == 1 && student.is_some() {
        let only = pick.options[0].0.clone();
        pick = new_pick(&conn, kind, &offering, student.as_ref(), &only, None)?;
    }
    pick.oob = true;
    Ok(Html(pick.render()?).into_response())
}

/// The parent page of a new enrollment opened from UI-15: the student's header and enrollments of this kind.
#[derive(Template)]
#[template(path = "school/offerings_enrollments/student_parent.html", blocks = ["content"])]
pub struct StudentParent {
    pub shell: Shell,
    pub student: String,
    pub name: String,
    pub company: String,
    pub status_text: String,
    pub kind: Kind,
    pub kind_label: &'static str,
    pub lines: Vec<StudentLine>,
    pub dialog: String,
}

impl StudentParent {
    pub fn kind_slug(&self) -> &'static str {
        self.kind.slug()
    }
}

impl Page for StudentParent {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

fn student_lines(conn: &Connection, kind: Kind, student: &str) -> rusqlite::Result<Vec<StudentLine>> {
    let t = kind.enrollment_table();
    let sql = format!(
        "SELECT e.uuid, o.{name}, COALESCE(e.status, ''), COALESCE(e.{price}, 0), e.pago FROM {t} e JOIN {ot} o ON o.uuid = e.{off}
          WHERE e.student_uuid = ?1 ORDER BY e.created_at DESC",
        name = kind.name_col(),
        price = kind.enrollment_price_col(),
        ot = kind.offering_table(),
        off = kind.enrollment_offering_col()
    );
    conn.prepare(&sql)?
        .query_map([student], |r| {
            let price: i64 = r.get(3)?;
            let paid: i64 = r.get(4)?;
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, usd(price), usd(price - paid)))
        })?
        .collect()
}

fn student_parent(
    conn: &Connection,
    user: &CurrentUser,
    kind: Kind,
    student: &str,
    dialog: &EnrDialog,
) -> Result<StudentParent, AppError> {
    let info = student_info(conn, student)?;
    Ok(StudentParent {
        shell: Shell::new(user, "escuela", "estudiantes"),
        student: student.to_owned(),
        name: info.name.clone(),
        company: info.company.clone().unwrap_or_default(),
        status_text: info.status_text(),
        kind,
        kind_label: kind.label(),
        lines: student_lines(conn, kind, student)?,
        dialog: dialog.render()?,
    })
}

fn with_open<P: Page>(status: StatusCode, headers: &HeaderMap, page: &P) -> Response {
    render_page_status(status, headers, page)
}

#[derive(Deserialize, Default)]
struct NewQuery {
    calendar: Option<String>,
    offering: Option<String>,
    next: Option<String>,
}

async fn enroll_new_for_student(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path((student, kind)): Path<(String, String)>,
    Query(q): Query<NewQuery>,
) -> Result<Response, AppError> {
    let kind = kind_of(&kind)?;
    let conn = state.db();
    let v = EnrValues {
        offering: q.offering.unwrap_or_default(),
        calendar: q.calendar.unwrap_or_default(),
        next: q.next.unwrap_or_default(),
        ..Default::default()
    };
    let dlg = new_dialog(
        &conn,
        &staff.0,
        kind,
        Some(&student),
        &v,
        Vec::new(),
        format!("/students/{student}/enrollments/{}", kind.slug()),
    )?;
    let page = student_parent(&conn, &staff.0, kind, &student, &dlg)?;
    Ok(with_open(StatusCode::OK, &headers, &page))
}

/// Creates the enrollment and settles it in one transaction. Errors: the field list (422).
fn create_and_settle(
    state: &AppState,
    who: &Who,
    kind: Kind,
    student: &str,
    v: &EnrValues,
) -> Result<Result<String, FieldErrors>, AppError> {
    let (settle, mut errs) = parse_settle(kind, v);
    let conn = state.db();
    if student.is_empty() || students::exists(&conn, student).is_err() {
        errs.insert(0, ("student".into(), "Estudiante", "Escoja un estudiante.".into()));
    }
    let active = !v.calendar.is_empty()
        && conn
            .query_row(
                &format!(
                    "SELECT status FROM {} WHERE uuid = ?1 AND {} = ?2",
                    kind.calendar_table(),
                    kind.calendar_offering_col()
                ),
                [&v.calendar, &v.offering],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .is_some_and(|s| offerings::is_active_status(&s));
    if !active {
        errs.insert(0, ("calendar".into(), "Fecha", "Escoja una fecha activa.".into()));
    }
    drop(conn);
    if !errs.is_empty() {
        return Ok(Err(errs));
    }
    let mut conn = state.db();
    let tx = school::begin(&mut conn)?;
    let r = enrollments::enroll(&tx, who, kind, student, &v.calendar).and_then(|e| {
        let mut s = settle.clone();
        if s.costeado_por.is_none() {
            s.costeado_por = Some(if e.member_price { "COMPAÑIA" } else { "INDIVIDUAL" }.into());
        }
        enrollments::settle(&tx, who, kind, &e.uuid, &s).map(|_| e.uuid)
    });
    match r {
        Ok(uuid) => {
            tx.commit()?;
            Ok(Ok(uuid))
        }
        Err(DomainError::CalendarNotActive) => {
            Ok(Err(vec![("calendar".into(), "Fecha", "Escoja una fecha activa.".into())]))
        }
        Err(DomainError::Validation(f, m)) => Ok(Err(vec![(f.to_owned(), "Campo", m)])),
        Err(e) => Err(dom(e)),
    }
}

async fn enroll_create_for_student(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path((student, kind)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Result<Response, AppError> {
    let kind = kind_of(&kind)?;
    let who = Who::of(&staff.0);
    let v = EnrValues::from_pairs(&crate::web::form_pairs(&body));
    student_info(&state.db(), &student)?;
    match create_and_settle(&state, &who, kind, &student, &v)? {
        Ok(_) => {
            let next =
                if v.next.is_empty() { format!("/students/{student}") } else { safe_next(Some(&v.next)).to_owned() };
            Ok(go(&headers, &next))
        }
        Err(errs) => {
            let conn = state.db();
            let dlg = new_dialog(
                &conn,
                &staff.0,
                kind,
                Some(&student),
                &v,
                errs,
                format!("/students/{student}/enrollments/{}", kind.slug()),
            )?;
            let page = student_parent(&conn, &staff.0, kind, &student, &dlg)?;
            Ok(with_open(StatusCode::UNPROCESSABLE_ENTITY, &headers, &page))
        }
    }
}

async fn enroll_new_from_calendar(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path(kind): Path<String>,
    Query(q): Query<NewQuery>,
) -> Result<Response, AppError> {
    let kind = kind_of(&kind)?;
    let conn = state.db();
    let cal = q.calendar.unwrap_or_default();
    let offering = if cal.is_empty() { String::new() } else { calendar_offering(&conn, kind, &cal)? };
    let v = EnrValues { offering, calendar: cal.clone(), ..Default::default() };
    let dlg = new_dialog(&conn, &staff.0, kind, None, &v, Vec::new(), format!("/enrollments/{}", kind.slug()))?;
    let mut page = list_page(&conn, &staff.0, kind, &ListQuery { calendar: Some(cal), ..Default::default() })?;
    page.dialog = dlg.render()?;
    page.open = Some("dlg-mat".into());
    Ok(render_oob(StatusCode::OK, &headers, &page))
}

async fn enroll_create_from_calendar(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path(kind): Path<String>,
    body: axum::body::Bytes,
) -> Result<Response, AppError> {
    let kind = kind_of(&kind)?;
    let who = Who::of(&staff.0);
    let v = EnrValues::from_pairs(&crate::web::form_pairs(&body));
    match create_and_settle(&state, &who, kind, &v.student, &v)? {
        Ok(_) => Ok(go(&headers, &format!("/calendar/{}/{}/enrollments?hecho=matriculado", kind.slug(), v.calendar))),
        Err(errs) => {
            let conn = state.db();
            let dlg = new_dialog(&conn, &staff.0, kind, None, &v, errs, format!("/enrollments/{}", kind.slug()))?;
            let mut page = list_page(
                &conn,
                &staff.0,
                kind,
                &ListQuery { calendar: Some(v.calendar.clone()), ..Default::default() },
            )?;
            page.dialog = dlg.render()?;
            page.open = Some("dlg-mat".into());
            Ok(render_oob(StatusCode::UNPROCESSABLE_ENTITY, &headers, &page))
        }
    }
}

fn enrollment_calendar(conn: &Connection, kind: Kind, id: &str) -> Result<Option<String>, AppError> {
    conn.query_row(
        &format!("SELECT {} FROM {} WHERE uuid = ?1", kind.enrollment_calendar_col(), kind.enrollment_table()),
        [id],
        |r| r.get(0),
    )
    .optional()?
    .ok_or(AppError::NotFound)
}

/// UI-21 of the enrollment's calendar row with the enrollment dialog open.
fn enrollment_page_of(
    conn: &Connection,
    user: &CurrentUser,
    kind: Kind,
    id: &str,
    dlg: EnrDialog,
    open: &str,
) -> Result<ListPage, AppError> {
    let cal = enrollment_calendar(conn, kind, id)?;
    let mut page = list_page(conn, user, kind, &ListQuery { calendar: cal, ..Default::default() })?;
    page.dialog = dlg.render()?;
    page.open = Some(open.to_owned());
    Ok(page)
}

#[derive(Deserialize, Default)]
struct EnrPageQuery {
    hecho: Option<String>,
}

async fn enrollment_page(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, String)>,
    Query(q): Query<EnrPageQuery>,
) -> Result<Response, AppError> {
    let kind = kind_of(&kind)?;
    let conn = state.db();
    let dlg = edit_dialog(&conn, &staff.0, kind, &id, None)?;
    let mut page = enrollment_page_of(&conn, &staff.0, kind, &id, dlg, "dlg-mat")?;
    page.notice = match q.hecho.as_deref() {
        Some("guardado") => Some("Matrícula guardada."),
        Some("baja") => Some(if kind == Kind::Course {
            "Matrícula dada de baja (BAJA)."
        } else {
            "Matrícula cancelada (Canceló)."
        }),
        _ => None,
    };
    Ok(render_oob(StatusCode::OK, &headers, &page))
}

async fn enrollment_save(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Result<Response, AppError> {
    let kind = kind_of(&kind)?;
    let who = Who::of(&staff.0);
    let v = EnrValues::from_pairs(&crate::web::form_pairs(&body));
    enrollments::pago(&state.db(), kind, &id).map_err(dom)?;
    let (settle, mut errs) = parse_settle(kind, &v);
    if errs.is_empty() {
        let mut conn = state.db();
        let tx = school::begin(&mut conn)?;
        match enrollments::settle(&tx, &who, kind, &id, &settle) {
            Ok(()) => {
                tx.commit()?;
                let cal = enrollment_calendar(&conn, kind, &id)?;
                let to = match (v.next.is_empty(), cal) {
                    (false, _) => safe_next(Some(&v.next)).to_owned(),
                    (true, Some(c)) => format!("/calendar/{}/{c}/enrollments?hecho=guardado", kind.slug()),
                    (true, None) => format!("/enrollments/{}", kind.slug()),
                };
                return Ok(go(&headers, &to));
            }
            Err(DomainError::Validation(f, m)) => errs.push((f.to_owned(), "Campo", m)),
            Err(e) => return Ok(e.into_response()),
        }
    }
    let conn = state.db();
    let dlg = edit_dialog(&conn, &staff.0, kind, &id, Some((&v, errs)))?;
    let page = enrollment_page_of(&conn, &staff.0, kind, &id, dlg, "dlg-mat")?;
    Ok(render_oob(StatusCode::UNPROCESSABLE_ENTITY, &headers, &page))
}

/// `PATCH /enrollments/{kind}/{id}`: one line field (UI-15, UI-21 lists); the field fragment.
async fn enrollment_patch(
    State(state): State<AppState>,
    staff: RequireStaff,
    r: Option<Extension<RequestId>>,
    Path((kind, id)): Path<(String, String)>,
    field: OneField,
) -> Result<Response, AppError> {
    let kind = kind_of(&kind)?;
    let who = Who::of(&staff.0);
    let mut conn = state.db();
    let res = Resource::Enrollment(kind);
    let reg = res.registry();
    let Some(def) = reg.get(&field.name) else {
        return Ok(autosave::contract_error("Este campo no se puede cambiar aquí."));
    };
    let outcome = match sreg::patch(&mut conn, &who, rid(&r), res, &id, &field.name, &field.value)? {
        Patched::Unknown => return Ok(autosave::contract_error("Este campo no se puede cambiar aquí.")),
        Patched::Done(o) => o,
    };
    let control = match def.name {
        "status" => Control::Select(pairs(if kind == Kind::Course {
            enrollments::COURSE_STATUSES
        } else {
            enrollments::IRREGULAR_STATUSES
        })),
        "costeado_por" => Control::Select(pairs(enrollments::PAYERS)),
        "pago" => Control::Money,
        "indicar_cambios" | "expectations" | "notas" => Control::Textarea(3),
        n if n == "puntuacion" || enrollments::GRADE_COLUMNS.contains(&n) => Control::Number,
        _ => Control::Text,
    };
    let url = format!("/enrollments/{}/{id}", kind.slug());
    let dom_id = format!("en-{}-{}", def.name, &id[..8.min(id.len())]);
    let view = |saved: String| FieldView::new(url.clone(), def.name, &dom_id, control.clone(), saved).aria(def.label);
    match outcome {
        Outcome::Saved { new, .. } => {
            let mut extra = String::new();
            if def.name == "promedio_final" {
                let s = stored(&conn, reg.table, "puntuacion", &id)?;
                extra = autosave::oob(&format!("en-score-{}", &id[..8.min(id.len())]), &esc(&autosave::text_of(&s)));
            }
            Ok(autosave::respond(&view(def.display(&new)), &extra))
        }
        Outcome::Invalid(m) => {
            let old = stored(&conn, reg.table, def.column, &id)?;
            Ok(autosave::respond(&view(def.display(&old)).refused(&field.value, m), ""))
        }
    }
}

async fn enrollment_cancel(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, String)>,
) -> Result<Response, AppError> {
    let kind = kind_of(&kind)?;
    let who = Who::of(&staff.0);
    let res = {
        let mut conn = state.db();
        let tx = school::begin(&mut conn)?;
        let r = enrollments::cancel_enrollment(&tx, &who, kind, &id);
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    match res {
        Ok(()) => Ok(go(&headers, &format!("/enrollments/{}/{id}?hecho=baja", kind.slug()))),
        Err(e) => Ok(e.into_response()),
    }
}

#[derive(Deserialize)]
struct DeleteBody {
    #[serde(default)]
    confirm: String,
}

async fn enrollment_delete(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, String)>,
    Form(f): Form<DeleteBody>,
) -> Result<Response, AppError> {
    let kind = kind_of(&kind)?;
    let who = Who::of(&staff.0);
    let cal = enrollment_calendar(&state.db(), kind, &id)?;
    let res = {
        let mut conn = state.db();
        let tx = school::begin(&mut conn)?;
        let r = enrollments::delete_enrollment(&tx, &who, kind, &id, f.confirm == "1");
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    match res {
        Ok(()) => Ok(go(
            &headers,
            &match cal {
                Some(c) => format!("/calendar/{}/{c}/enrollments?hecho=quitada", kind.slug()),
                None => format!("/enrollments/{}?hecho=quitada", kind.slug()),
            },
        )),
        Err(DomainError::ConfirmRequired) => {
            let conn = state.db();
            let dlg = edit_dialog(&conn, &staff.0, kind, &id, None)?;
            let page = enrollment_page_of(&conn, &staff.0, kind, &id, dlg, "dlg-mat-borrar")?;
            Ok(render_oob(StatusCode::OK, &headers, &page))
        }
        Err(DomainError::EnrollmentPaid) => {
            let conn = state.db();
            let dlg = edit_dialog(&conn, &staff.0, kind, &id, None)?;
            let mut page = enrollment_page_of(&conn, &staff.0, kind, &id, dlg, "dlg-mat")?;
            page.refusal = Some("Tiene un pago: no se borra. Use Dar de baja (BR-ENR-08).".into());
            Ok(render_oob(StatusCode::CONFLICT, &headers, &page))
        }
        Err(e) => Ok(e.into_response()),
    }
}

// ===============================================================================================================
// UI-21 Matrículas
// ===============================================================================================================

/// One enrollment line of UI-21.
pub struct EnrLine {
    pub uuid: String,
    pub name: String,
    pub invoice: String,
    pub payer: String,
    pub cost: i64,
    pub paid: i64,
    pub balance: i64,
    pub grades: [String; 4],
    pub status: String,
    pub n: usize,
}

impl EnrLine {
    pub fn tag(&self) -> &'static str {
        enr_tag(&self.status)
    }
}

#[derive(Deserialize, Default)]
pub struct ListQuery {
    calendar: Option<String>,
    status: Option<String>,
    q: Option<String>,
    hecho: Option<String>,
}

#[derive(Template)]
#[template(path = "school/offerings_enrollments/matriculas.html", blocks = ["content", "pagebar"])]
pub struct ListPage {
    pub shell: Shell,
    pub kind: Kind,
    pub switch: Vec<(String, &'static str, bool)>,
    pub calendars: Vec<(String, String)>,
    pub calendar: String,
    pub sub: String,
    pub statuses: Vec<(String, String)>,
    pub status: String,
    pub q: String,
    pub lines: Vec<EnrLine>,
    pub total_lines: i64,
    pub summary: String,
    pub sum_cost: i64,
    pub sum_paid: i64,
    pub sum_balance: i64,
    pub dialog: String,
    pub open: Option<String>,
    pub notice: Option<&'static str>,
    pub refusal: Option<String>,
}

impl ListPage {
    pub fn is_course(&self) -> bool {
        self.kind == Kind::Course
    }
    pub fn is_short(&self) -> bool {
        self.kind == Kind::ShortCourse
    }
    pub fn slug(&self) -> &'static str {
        self.kind.slug()
    }
    pub fn usd(&self, c: &i64) -> String {
        usd(*c)
    }
    pub fn is_sel_cal(&self, v: &str) -> bool {
        self.calendar == v
    }
    pub fn is_sel_st(&self, v: &str) -> bool {
        self.status == v
    }
    pub fn cal_q(&self) -> String {
        urlencode(&self.calendar)
    }
}

impl OobPage for ListPage {
    fn content(&self) -> askama::Result<String> {
        self.as_content().render()
    }
    fn pagebar(&self) -> askama::Result<String> {
        self.as_pagebar().render()
    }
}

impl Page for ListPage {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

/// Every calendar row of a kind for the picker: (uuid, "Offering · date · status"), newest first.
fn calendar_choices(conn: &Connection, kind: Kind) -> Result<Vec<CalChoice>, AppError> {
    let sql = format!(
        "SELECT c.uuid, o.{name}, c.status FROM {ct} c JOIN {ot} o ON o.uuid = c.{off}",
        name = kind.name_col(),
        ct = kind.calendar_table(),
        ot = kind.offering_table(),
        off = kind.calendar_offering_col()
    );
    let base: Vec<(String, String, String)> =
        conn.prepare(&sql)?.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
    let mut out = Vec::new();
    for (uuid, name, status) in base {
        let row = offerings::calendar_row(conn, kind, &uuid)?.ok_or(AppError::NotFound)?;
        let d = date_txt(&row.first_date);
        let label = format!("{name} · {} · {status}", if d.is_empty() { "sin fecha".into() } else { d });
        out.push((uuid, label, row.first_date.clone(), name, row.is_active, row.enrollments));
    }
    out.sort_by(|a, b| b.2.cmp(&a.2).then(a.3.cmp(&b.3)));
    Ok(out)
}

fn list_page(conn: &Connection, user: &CurrentUser, kind: Kind, q: &ListQuery) -> Result<ListPage, AppError> {
    let choices = calendar_choices(conn, kind)?;
    let today = school_today();
    // Default: the next active row with enrollments that already started or starts soonest; else the newest with any.
    let calendar = match q.calendar.as_deref().filter(|c| !c.is_empty()) {
        Some(c) => {
            if !choices.iter().any(|x| x.0 == c) {
                return Err(AppError::NotFound);
            }
            c.to_owned()
        }
        None => choices
            .iter()
            .filter(|c| c.4 && c.5 > 0)
            .min_by_key(|c| {
                c.2.as_deref()
                    .map(|d| if d >= today.as_str() { (0, d.to_owned()) } else { (1, String::new()) })
                    .unwrap_or((2, String::new()))
            })
            .or_else(|| choices.iter().find(|c| c.5 > 0))
            .map(|c| c.0.clone())
            .unwrap_or_default(),
    };
    let status = q.status.clone().unwrap_or_default();
    let term = q.q.clone().unwrap_or_default().trim().to_owned();
    let lines = if calendar.is_empty() { Vec::new() } else { enrollment_lines(conn, kind, &calendar, &status, &term)? };
    let total_lines = if calendar.is_empty() { 0 } else { enrollments::calendar_count(conn, kind, &calendar)? };
    let (sub, summary) = match choices.iter().find(|c| c.0 == calendar) {
        Some(c) => {
            let parts: Vec<&str> = c.1.splitn(3, " · ").collect();
            (format!("· {} · {}", parts[0], parts.get(1).copied().unwrap_or_default()), line_summary(kind, &lines))
        }
        None => (String::new(), String::new()),
    };
    let statuses = if kind == Kind::Course {
        vec![
            (String::new(), "ACTIVO y BAJA".into()),
            ("ACTIVO".into(), "ACTIVO".into()),
            ("BAJA".into(), "BAJA".into()),
        ]
    } else {
        let mut v = vec![(String::new(), "Todos".to_owned())];
        v.extend(pairs(enrollments::IRREGULAR_STATUSES));
        v
    };
    Ok(ListPage {
        shell: Shell::new(user, "escuela", "matriculas"),
        kind,
        switch: kind_switch("/enrollments", kind),
        calendars: choices.iter().map(|c| (c.0.clone(), c.1.clone())).collect(),
        calendar,
        sub,
        statuses,
        status,
        q: term,
        sum_cost: lines.iter().map(|l| l.cost).sum(),
        sum_paid: lines.iter().map(|l| l.paid).sum(),
        sum_balance: lines.iter().map(|l| l.balance).sum(),
        lines,
        total_lines,
        summary,
        dialog: String::new(),
        open: None,
        notice: match q.hecho.as_deref() {
            Some("matriculado") => Some("Estudiante matriculado."),
            Some("guardado") => Some("Matrícula guardada."),
            Some("quitada") => Some("Matrícula quitada."),
            _ => None,
        },
        refusal: None,
    })
}

fn school_today() -> String {
    crate::clock::now_text()[..10].to_owned()
}

/// "7 matrículas · 6 ACTIVO · 1 BAJA".
fn line_summary(kind: Kind, lines: &[EnrLine]) -> String {
    let n = lines.len();
    let mut s = format!("{n} matrícula{}", if n == 1 { "" } else { "s" });
    let list = if kind == Kind::Course { enrollments::COURSE_STATUSES } else { enrollments::IRREGULAR_STATUSES };
    for st in list {
        let c = lines.iter().filter(|l| l.status == *st).count();
        if c > 0 {
            s.push_str(&format!(" · {c} {st}"));
        }
    }
    s
}

/// The lines of one calendar row: course rows from `v_enrollments_course_lines` (balance BR-ENR-02).
fn enrollment_lines(
    conn: &Connection,
    kind: Kind,
    calendar: &str,
    status: &str,
    term: &str,
) -> Result<Vec<EnrLine>, AppError> {
    let like = format!("%{}%", term.to_lowercase());
    let sql = match kind {
        Kind::Course => {
            "SELECT l.uuid, s.nombre, l.invoice_num, COALESCE(l.costeado_por, ''), COALESCE(l.costo, 0), l.pago, l.balance,
                    l.examen, l.promedio_asignaciones, l.asistencia_y_puntualidad, l.promedio_final, l.status
               FROM v_enrollments_course_lines l JOIN students s ON s.uuid = l.student_uuid
              WHERE l.course_calendar_uuid = ?1 AND (?2 = '' OR l.status = ?2)
                AND (?3 = '%%' OR lower(s.nombre) LIKE ?3 OR CAST(l.invoice_num AS TEXT) LIKE ?3)
              ORDER BY s.nombre"
                .to_owned()
        }
        _ => format!(
            "SELECT e.uuid, s.nombre, e.invoice_number, COALESCE(e.costeado_por, ''), COALESCE(e.cargo, 0), e.pago,
                    COALESCE(e.cargo, 0) - e.pago, NULL, NULL, NULL, NULL, COALESCE(e.status, '')
               FROM {t} e JOIN students s ON s.uuid = e.student_uuid
              WHERE e.{cal} = ?1 AND (?2 = '' OR e.status = ?2)
                AND (?3 = '%%' OR lower(s.nombre) LIKE ?3 OR CAST(e.invoice_number AS TEXT) LIKE ?3)
              ORDER BY s.nombre",
            t = kind.enrollment_table(),
            cal = kind.enrollment_calendar_col()
        ),
    };
    let prefix = if kind == Kind::Course { "F" } else { "S" };
    let mut n = 0;
    let rows = conn
        .prepare(&sql)?
        .query_map(rusqlite::params![calendar, status, like], |r| {
            let inv: Option<i64> = r.get(2)?;
            let g = |i: usize| -> rusqlite::Result<String> { Ok(num_txt(r.get::<_, Option<f64>>(i)?)) };
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                inv,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                [g(7)?, g(8)?, g(9)?, g(10)?],
                r.get::<_, String>(11)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .map(|(uuid, name, inv, payer, cost, paid, balance, grades, status)| {
            n += 1;
            EnrLine {
                uuid,
                name,
                invoice: inv.map(|i| format!("{prefix}-{i}")).unwrap_or_default(),
                payer,
                cost,
                paid,
                balance,
                grades,
                status,
                n,
            }
        })
        .collect())
}

async fn enrollments_list(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path(kind): Path<String>,
    Query(q): Query<ListQuery>,
) -> Result<Response, AppError> {
    let kind = kind_of(&kind)?;
    let page = list_page(&state.db(), &staff.0, kind, &q)?;
    Ok(render_oob(StatusCode::OK, &headers, &page))
}

/// OP-CRS-05: the enrollments of one calendar row (UI-21 with the row picked; empty: the message).
async fn calendar_enrollments(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, String)>,
    Query(mut q): Query<ListQuery>,
) -> Result<Response, AppError> {
    let kind = kind_of(&kind)?;
    q.calendar = Some(id);
    let page = list_page(&state.db(), &staff.0, kind, &q)?;
    Ok(render_oob(StatusCode::OK, &headers, &page))
}

#[derive(Deserialize, Default)]
struct ExportQuery {
    calendar: Option<String>,
}

/// OP-ENR-06 (AP-10): the grades workbook of one course calendar row (or all course enrollments).
async fn export_grades(
    State(state): State<AppState>,
    _staff: RequireStaff,
    Query(q): Query<ExportQuery>,
) -> Result<Response, AppError> {
    let conn = state.db();
    let cal = q.calendar.filter(|c| !c.is_empty());
    let name = match &cal {
        Some(c) => {
            let row = offerings::calendar_row(&conn, Kind::Course, c)?.ok_or(AppError::NotFound)?;
            format!("notas-{}.xlsx", row.first_date.unwrap_or_else(|| "sin-fecha".into()))
        }
        None => "notas.xlsx".into(),
    };
    let bytes = grades::export_xlsx(&conn, cal.as_deref()).map_err(dom)?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_owned()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
        ],
        bytes,
    )
        .into_response())
}
