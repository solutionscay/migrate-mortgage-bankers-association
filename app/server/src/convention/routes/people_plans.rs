//! Phase-2 pane "people_plans": UI-03 Convencionistas, UI-04 Convencionista, UI-12 Planes, UI-13 Plan.
//!
//! * `GET /people` (`?q=`, `?company=`, `?page=`); with `HX-Target: people-results` only the
//!   results block. `GET /people/new` opens the create dialog; `POST /people` creates (OP-PPL-01).
//! * `GET /people/{id}`, `PATCH /people/{id}` (one field), `DELETE /people/{id}` (OP-PPL-02, AP-04: staff 403,
//!   registrations 409), `POST /people/{id}/registrations` ("Añadir registro", OP-REG-01, redirect to UI-06).
//! * `GET /plans` (`?event=`, `?q=`), `POST /plans` (OP-EVT-05 create, then OP-EVT-04 build).
//! * `GET /plans/{id}`, `PATCH /plans/{id}` (name, price override with the price out of band, event: the after-rule
//!   builds the items and the page reloads), `DELETE /plans/{id}` (AP-04), `POST /plans/{id}/build` (OP-EVT-04).
//! * `PATCH /plan-items/{id}` (`active` or `ticket_qty`: the row and the price out of band), `DELETE
//!   /plan-items/{id}` (AP-06).
//!
//! Deletes and the create dialogs are plain forms (`_method=DELETE`): success is 303, a refusal re-renders the page
//! with its status (403 shell page, 409 with the refusal dialog open, 422 with the dialog and its messages).

use crate::audit::RequestId;
use crate::auth::{CurrentUser, RequireStaff, Shell};
use crate::autosave::{self, Control, FieldView, OneField, Outcome};
use crate::convention::registries::{self, show_date, show_time, Patched, Resource, Target};
use crate::convention::{self as conv, events, people, plans, registrations, DomainError, Who};
use crate::error::AppError;
use crate::web::{is_htmx, render_page, render_page_status, urlencode, Page};
use crate::AppState;
use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, patch, post};
use axum::{Extension, Form, Router};
use rusqlite::types::Value;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/people", get(people_list).post(person_create))
        .route("/people/new", get(person_new))
        .route("/people/{id}", get(person_page).patch(person_patch).delete(person_delete))
        .route("/people/{id}/registrations", post(person_add_registration))
        .route("/plans", get(plans_list).post(plan_create))
        .route("/plans/{id}", get(plan_page).patch(plan_patch).delete(plan_delete))
        .route("/plans/{id}/build", post(plan_build))
        .route("/plan-items/{id}", patch(plan_item_patch).delete(plan_item_delete))
}

/// People per page (UI-03 pager).
const PAGE_SIZE: i64 = 50;

fn usd(c: i64) -> String {
    crate::seed::usd(c)
}

fn text(v: Option<String>) -> String {
    v.unwrap_or_default()
}

/// "2,318" (title counts).
fn grouped(n: i64) -> String {
    usd(n * 100).trim_start_matches('$').trim_end_matches(".00").to_owned()
}

fn date_of(v: &Option<String>) -> String {
    v.as_ref().map(|s| show_date(&Value::Text(s.clone()))).unwrap_or_default()
}

fn time_of(v: &Option<String>) -> String {
    v.as_ref().map(|s| show_time(&Value::Text(s.clone()))).unwrap_or_default()
}

fn rid(r: &Option<Extension<RequestId>>) -> Option<&str> {
    r.as_ref().map(|r| r.0 .0.as_str())
}

/// The stored value of one column, for the field fragment after a refusal.
fn stored(conn: &Connection, table: &str, column: &str, key: &str) -> Result<Value, AppError> {
    Ok(conn.query_row(&format!("SELECT {column} FROM {table} WHERE uuid = ?1"), [key], |r| r.get(0))?)
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

/// Status tag class of a registration status (design/registros.html).
pub fn status_class(status: &str) -> &'static str {
    match status {
        "Registrado" => " info",
        "Asistió" => " ok",
        "Canceló" => " err",
        "No Canceló" | "No Asistió" => " warn",
        _ => "",
    }
}

// ===============================================================================================================
// UI-03 Convencionistas
// ===============================================================================================================

pub struct PersonRow {
    pub uuid: String,
    pub name: String,
    pub company: String,
    pub phone: String,
    pub email: String,
    pub city: String,
    pub registrations: i64,
}

/// The create dialog: typed values and the message.
#[derive(Default)]
pub struct NewPersonForm {
    pub name_first: String,
    pub name_last: String,
    pub company: String,
    pub error: Option<String>,
}

#[derive(Template)]
#[template(path = "convention/people_plans/people_results.html")]
pub struct PeopleResults {
    pub rows: Vec<PersonRow>,
    /// All people (empty state) vs. the filtered count (no results).
    pub total_all: i64,
    pub total: i64,
    pub page: i64,
    pub pages: i64,
    pub q: String,
    pub company: String,
    pub is_admin: bool,
    pub csrf: String,
}

impl PeopleResults {
    pub fn first(&self) -> i64 {
        if self.total == 0 {
            0
        } else {
            (self.page - 1) * PAGE_SIZE + 1
        }
    }
    pub fn last(&self) -> i64 {
        (self.page * PAGE_SIZE).min(self.total)
    }
    pub fn n(&self, v: &i64) -> String {
        grouped(*v)
    }
    /// The query string of another page, keeping the filters.
    pub fn link(&self, page: i64) -> String {
        let mut s = format!("/people?page={page}");
        if !self.q.is_empty() {
            s.push_str(&format!("&q={}", urlencode(&self.q)));
        }
        if !self.company.is_empty() {
            s.push_str(&format!("&company={}", urlencode(&self.company)));
        }
        s
    }
    pub fn page_list(&self) -> Vec<i64> {
        let lo = (self.page - 2).max(1);
        let hi = (lo + 4).min(self.pages);
        ((hi - 4).max(1)..=hi).collect()
    }
}

#[derive(Template)]
#[template(path = "convention/people_plans/people_list.html", blocks = ["content"])]
pub struct PeoplePage {
    pub shell: Shell,
    pub results: PeopleResults,
    pub companies: Vec<String>,
    pub new_person: NewPersonForm,
    pub notice: Option<&'static str>,
    pub open: Option<&'static str>,
}

impl Page for PeoplePage {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

#[derive(Deserialize, Default)]
pub struct PeopleQuery {
    q: Option<String>,
    company: Option<String>,
    page: Option<i64>,
    hecho: Option<String>,
}

/// Query `people_search` (05): name, company or e-mail; exact company filter; name order; one page.
fn people_search(conn: &Connection, q: &PeopleQuery, user: &CurrentUser) -> Result<PeopleResults, AppError> {
    let term = q.q.clone().unwrap_or_default().trim().to_owned();
    let company = q.company.clone().unwrap_or_default().trim().to_owned();
    let filter = "(?1 = '' OR (COALESCE(name_first,'') || ' ' || COALESCE(name_last,'')) LIKE '%' || ?1 || '%'
                   OR company LIKE '%' || ?1 || '%' OR email LIKE '%' || ?1 || '%')
                  AND (?2 = '' OR company = ?2)";
    let total_all: i64 = conn.query_row("SELECT COUNT(*) FROM people", [], |r| r.get(0))?;
    let total: i64 =
        conn.query_row(&format!("SELECT COUNT(*) FROM people WHERE {filter}"), params![term, company], |r| r.get(0))?;
    let pages = ((total + PAGE_SIZE - 1) / PAGE_SIZE).max(1);
    let page = q.page.unwrap_or(1).clamp(1, pages);
    let rows = conn
        .prepare(&format!(
            "SELECT uuid, name_first, name_last, company, phone1, email, city,
                    (SELECT COUNT(*) FROM registration r WHERE r.person_uuid = people.uuid)
               FROM people WHERE {filter}
              ORDER BY COALESCE(name_first,'') || ' ' || COALESCE(name_last,'') COLLATE NOCASE, uuid
              LIMIT ?3 OFFSET ?4"
        ))?
        .query_map(params![term, company, PAGE_SIZE, (page - 1) * PAGE_SIZE], |r| {
            Ok(PersonRow {
                uuid: r.get(0)?,
                name: people::full_name(
                    r.get::<_, Option<String>>(1)?.as_deref(),
                    r.get::<_, Option<String>>(2)?.as_deref(),
                ),
                company: text(r.get(3)?),
                phone: text(r.get(4)?),
                email: text(r.get(5)?),
                city: text(r.get(6)?),
                registrations: r.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PeopleResults {
        rows,
        total_all,
        total,
        page,
        pages,
        q: term,
        company,
        is_admin: user.role == crate::auth::Role::Admin,
        csrf: user.csrf.clone(),
    })
}

/// Value list `people.company`: the companies people have, for the filter and the datalist.
fn companies(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    conn.prepare("SELECT DISTINCT company FROM people WHERE company IS NOT NULL AND company <> '' ORDER BY company COLLATE NOCASE")?
        .query_map([], |r| r.get(0))?
        .collect()
}

fn people_page(conn: &Connection, user: &CurrentUser, q: &PeopleQuery) -> Result<PeoplePage, AppError> {
    Ok(PeoplePage {
        shell: Shell::new(user, "convencion", "convencionistas"),
        results: people_search(conn, q, user)?,
        companies: companies(conn)?,
        new_person: NewPersonForm::default(),
        notice: match q.hecho.as_deref() {
            Some("borrado") => Some("Convencionista borrado."),
            _ => None,
        },
        open: None,
    })
}

async fn people_list(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Query(q): Query<PeopleQuery>,
) -> Result<Response, AppError> {
    let conn = state.db();
    if headers.get("hx-target").is_some_and(|v| v == "people-results") {
        return Ok(Html(people_search(&conn, &q, &staff.0)?.render()?).into_response());
    }
    let page = people_page(&conn, &staff.0, &q)?;
    Ok(render_page(&headers, &page)?.into_response())
}

async fn person_new(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let mut page = people_page(&state.db(), &staff.0, &PeopleQuery::default())?;
    page.open = Some("dlg-nuevo");
    Ok(render_page(&headers, &page)?.into_response())
}

#[derive(Deserialize)]
struct NewPersonBody {
    #[serde(default)]
    name_first: String,
    #[serde(default)]
    name_last: String,
    #[serde(default)]
    company: String,
}

async fn person_create(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Form(f): Form<NewPersonBody>,
) -> Result<Response, AppError> {
    let who = Who::of(&staff.0);
    let np = people::NewPerson {
        name_first: f.name_first.clone(),
        name_last: f.name_last.clone(),
        company: f.company.clone(),
    };
    let result = {
        let mut conn = state.db();
        let tx = conv::begin(&mut conn)?;
        match people::create_person(&tx, &who, &np) {
            Ok(uuid) => {
                tx.commit()?;
                Ok(uuid)
            }
            Err(e) => Err(e),
        }
    };
    match result {
        Ok(uuid) => Ok(go(&headers, &format!("/people/{uuid}"))),
        Err(e @ DomainError::Validation(..)) => {
            let mut page = people_page(&state.db(), &staff.0, &PeopleQuery::default())?;
            page.new_person = NewPersonForm {
                name_first: f.name_first,
                name_last: f.name_last,
                company: f.company,
                error: Some(e.message()),
            };
            page.open = Some("dlg-nuevo");
            Ok(render_page_status(StatusCode::UNPROCESSABLE_ENTITY, &headers, &page))
        }
        Err(e) => Ok(e.into_response()),
    }
}

// ===============================================================================================================
// UI-04 Convencionista
// ===============================================================================================================

pub struct PersonRegRow {
    pub uuid: String,
    pub number: Option<i64>,
    pub event: Option<String>,
    pub location: String,
    pub start: String,
    pub end: String,
    pub status: String,
    pub total: i64,
    pub paid: i64,
    pub balance: i64,
    pub payments: i64,
}

impl PersonRegRow {
    pub fn tag(&self) -> &'static str {
        status_class(&self.status)
    }
}

/// The company field with its datalist (value list `people.company`, 06 UI-04).
#[derive(Template)]
#[template(path = "convention/people_plans/company_field.html")]
pub struct CompanyField {
    pub f: FieldView,
    pub options: Vec<String>,
}

#[derive(Template)]
#[template(path = "convention/people_plans/person.html", blocks = ["content"])]
pub struct PersonPage {
    pub shell: Shell,
    pub uuid: String,
    pub name: String,
    pub first: FieldView,
    pub last: FieldView,
    pub company: CompanyField,
    pub phone1: FieldView,
    pub phone2: FieldView,
    pub fax: FieldView,
    pub address: FieldView,
    pub city: FieldView,
    pub state: FieldView,
    pub zip: FieldView,
    pub email: FieldView,
    pub badge: FieldView,
    pub regs: Vec<PersonRegRow>,
    pub total: i64,
    pub paid: i64,
    pub balance: i64,
    pub open: Option<&'static str>,
}

impl Page for PersonPage {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

impl PersonPage {
    pub fn usd(&self, c: &i64) -> String {
        usd(*c)
    }
    /// The person's name as a query value ("Ver registros").
    pub fn name_q(&self) -> String {
        urlencode(&self.name)
    }
}

fn person_url(id: &str) -> String {
    format!("/people/{id}")
}

/// The autosave field of one `people` column, as the page and the PATCH response render it.
fn person_field(id: &str, name: &str, saved: String) -> FieldView {
    let (dom, aria) = match name {
        "name_first" => ("p-nombre", None),
        "name_last" => ("p-apellido", None),
        "company" => ("p-comp", None),
        "phone1" => ("p-tel", None),
        "phone2" => ("p-tel2", None),
        "fax" => ("p-fax", None),
        "address" => ("p-dir", None),
        "city" => ("p-ciudad", Some("Ciudad")),
        "state" => ("p-estado", Some("Estado")),
        "zip" => ("p-zip", Some("Zip")),
        "email" => ("p-email", None),
        _ => ("p-badge", None),
    };
    let f = FieldView::new(person_url(id), name, dom, Control::Text, saved);
    match aria {
        Some(a) => f.aria(a),
        None => f,
    }
}

struct PersonRec {
    cols: [Option<String>; 12],
}

const PERSON_COLS: [&str; 12] = [
    "name_first",
    "name_last",
    "company",
    "phone1",
    "phone2",
    "fax",
    "address",
    "city",
    "state",
    "zip",
    "email",
    "name_badge",
];

fn person_rec(conn: &Connection, id: &str) -> Result<PersonRec, AppError> {
    conn.query_row(&format!("SELECT {} FROM people WHERE uuid = ?1", PERSON_COLS.join(", ")), [id], |r| {
        let mut cols: [Option<String>; 12] = Default::default();
        for (i, c) in cols.iter_mut().enumerate() {
            *c = r.get(i)?;
        }
        Ok(PersonRec { cols })
    })
    .optional()?
    .ok_or(AppError::NotFound)
}

/// Query `person_get` registrations portal: event, place, dates, status and the `v_registration_totals` figures.
fn person_regs(conn: &Connection, id: &str) -> rusqlite::Result<Vec<PersonRegRow>> {
    conn.prepare(
        "SELECT r.uuid, r.registration_number, e.name, e.location, e.date_start, e.date_end, r.status,
                t.total, t.payment_total, t.balance,
                (SELECT COUNT(*) FROM payments p WHERE p.registration_uuid = r.uuid)
           FROM registration r
           JOIN v_registration_totals t ON t.registration_uuid = r.uuid
           LEFT JOIN events e ON e.uuid = r.event_uuid
          WHERE r.person_uuid = ?1
          ORDER BY e.date_start IS NULL DESC, e.date_start DESC, r.registration_number DESC",
    )?
    .query_map([id], |r| {
        Ok(PersonRegRow {
            uuid: r.get(0)?,
            number: r.get(1)?,
            event: r.get(2)?,
            location: text(r.get(3)?),
            start: date_of(&r.get(4)?),
            end: date_of(&r.get(5)?),
            status: r.get(6)?,
            total: r.get(7)?,
            paid: r.get(8)?,
            balance: r.get(9)?,
            payments: r.get(10)?,
        })
    })?
    .collect()
}

fn person_page_of(conn: &Connection, user: &CurrentUser, id: &str) -> Result<PersonPage, AppError> {
    let p = person_rec(conn, id)?;
    let c = |i: usize| p.cols[i].clone().unwrap_or_default();
    let regs = person_regs(conn, id)?;
    let mut shell = Shell::new(user, "convencion", "convencionistas");
    shell.autosave = true;
    Ok(PersonPage {
        shell,
        uuid: id.to_owned(),
        name: people::full_name(p.cols[0].as_deref(), p.cols[1].as_deref()),
        first: person_field(id, "name_first", c(0)),
        last: person_field(id, "name_last", c(1)),
        company: CompanyField { f: person_field(id, "company", c(2)), options: companies(conn)? },
        phone1: person_field(id, "phone1", c(3)),
        phone2: person_field(id, "phone2", c(4)),
        fax: person_field(id, "fax", c(5)),
        address: person_field(id, "address", c(6)),
        city: person_field(id, "city", c(7)),
        state: person_field(id, "state", c(8)),
        zip: person_field(id, "zip", c(9)),
        email: person_field(id, "email", c(10)),
        badge: person_field(id, "name_badge", c(11)),
        total: regs.iter().map(|r| r.total).sum(),
        paid: regs.iter().map(|r| r.paid).sum(),
        balance: regs.iter().map(|r| r.balance).sum(),
        regs,
        open: None,
    })
}

async fn person_page(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let page = person_page_of(&state.db(), &staff.0, &id)?;
    Ok(render_page(&headers, &page)?.into_response())
}

/// The title and crumb after a name edit (out of band).
fn person_title_oob(conn: &Connection, id: &str) -> Result<String, AppError> {
    let (f, l): (Option<String>, Option<String>) =
        conn.query_row("SELECT name_first, name_last FROM people WHERE uuid = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
    let name = askama::filters::escape(people::full_name(f.as_deref(), l.as_deref()), askama::filters::Html)
        .map(|s| s.to_string())
        .unwrap_or_default();
    Ok(autosave::oob("person-title", &format!("<b>Convencionista</b> · {name}"))
        + &autosave::oob("person-crumb", &name))
}

async fn person_patch(
    State(state): State<AppState>,
    staff: RequireStaff,
    r: Option<Extension<RequestId>>,
    Path(id): Path<String>,
    field: OneField,
) -> Result<Response, AppError> {
    let who = Who::of(&staff.0);
    let mut conn = state.db();
    let Some(def) = registries::PERSON.get(&field.name) else {
        return Ok(autosave::contract_error("Este campo no se puede cambiar aquí."));
    };
    // OP-PPL-01: a person keeps a first or a last name (the create rule, kept on edit).
    let other = match def.name {
        "name_first" => Some("name_last"),
        "name_last" => Some("name_first"),
        _ => None,
    };
    let mut refused: Option<String> = None;
    if let Some(o) = other {
        let other_val = autosave::text_of(&stored(&conn, "people", o, &id).map_err(|_| AppError::NotFound)?);
        if field.value.trim().is_empty() && other_val.trim().is_empty() {
            refused = Some("Escriba el nombre o el apellido.".into());
        }
    }
    let outcome = match refused {
        Some(m) => Outcome::Invalid(m),
        None => match registries::patch(
            &mut conn,
            &who,
            rid(&r),
            Target::new(Resource::Person, &id),
            &field.name,
            &field.value,
        )? {
            Patched::Unknown => return Ok(autosave::contract_error("Este campo no se puede cambiar aquí.")),
            Patched::Done(o) => o,
        },
    };
    let (view, extra) = match outcome {
        Outcome::Saved { new, changed, .. } => {
            let extra = if changed && other.is_some() { person_title_oob(&conn, &id)? } else { String::new() };
            (person_field(&id, def.name, def.display(&new)), extra)
        }
        Outcome::Invalid(m) => {
            let old = stored(&conn, "people", def.column, &id)?;
            (person_field(&id, def.name, def.display(&old)).refused(&field.value, m), String::new())
        }
    };
    if def.name == "company" {
        let status = if view.error.is_some() { StatusCode::UNPROCESSABLE_ENTITY } else { StatusCode::OK };
        let html = CompanyField { f: view, options: companies(&conn)? }.render()?;
        return Ok((status, Html(html + &extra)).into_response());
    }
    Ok(autosave::respond(&view, &extra))
}

async fn person_delete(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let who = Who::of(&staff.0);
    let res = {
        let mut conn = state.db();
        let tx = conv::begin(&mut conn)?;
        let r = people::delete_person(&tx, &who, &id);
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    match res {
        Ok(()) => Ok(go(&headers, "/people?hecho=borrado")),
        Err(DomainError::HasRegistrations(_)) => {
            let mut page = person_page_of(&state.db(), &staff.0, &id)?;
            page.open = Some("dlg-borrar");
            Ok(render_page_status(StatusCode::CONFLICT, &headers, &page))
        }
        Err(e) => Ok(e.into_response()),
    }
}

async fn person_add_registration(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let who = Who::of(&staff.0);
    let res = {
        let mut conn = state.db();
        let tx = conv::begin(&mut conn)?;
        let r = registrations::create_registration(&tx, &who, &id);
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    match res {
        Ok(uuid) => Ok(go(&headers, &format!("/registrations/{uuid}"))),
        Err(e) => Ok(e.into_response()),
    }
}

// ===============================================================================================================
// UI-12 Planes
// ===============================================================================================================

pub struct PlanRow {
    pub uuid: String,
    pub name: String,
    pub event: String,
    pub included: i64,
    pub price: i64,
    pub fixed: bool,
    pub registrations: i64,
}

#[derive(Default)]
pub struct NewPlanForm {
    pub event_uuid: String,
    pub name: String,
    pub error: Option<String>,
}

#[derive(Template)]
#[template(path = "convention/people_plans/plans_list.html", blocks = ["content"])]
pub struct PlansPage {
    pub shell: Shell,
    pub rows: Vec<PlanRow>,
    pub total_all: i64,
    /// (uuid, label) of the events that have plans, and the active ones.
    pub events: Vec<(String, String)>,
    pub active: Vec<(String, String)>,
    pub event: String,
    pub event_name: Option<String>,
    pub q: String,
    pub new_plan: NewPlanForm,
    pub notice: Option<String>,
    pub error: Option<String>,
    pub open: Option<&'static str>,
}

impl Page for PlansPage {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

impl PlansPage {
    pub fn usd(&self, c: &i64) -> String {
        usd(*c)
    }
}

#[derive(Deserialize, Default)]
pub struct PlansQuery {
    event: Option<String>,
    q: Option<String>,
    hecho: Option<String>,
}

/// "Convención Demo 2027 · 5/6/2027" (plan event options).
fn event_label(name: &str, start: &Option<String>) -> String {
    match start {
        Some(_) => format!("{name} · {}", date_of(start)),
        None => name.to_owned(),
    }
}

fn active_options(conn: &Connection) -> rusqlite::Result<Vec<(String, String)>> {
    Ok(events::active_events(conn)?.into_iter().map(|(u, n, d)| (u, event_label(&n, &d))).collect())
}

/// Query `plan_list` (05): `?event=` (absent = the first active event, empty = all) and the plan name.
fn plans_page(conn: &Connection, user: &CurrentUser, q: &PlansQuery) -> Result<PlansPage, AppError> {
    let active = active_options(conn)?;
    let event = match &q.event {
        Some(e) => e.trim().to_owned(),
        None => active.first().map(|(u, _)| u.clone()).unwrap_or_default(),
    };
    let term = q.q.clone().unwrap_or_default().trim().to_owned();
    let rows = conn
        .prepare(
            "SELECT p.uuid, p.name, e.name, v.price, p.price_override IS NOT NULL,
                    (SELECT COUNT(*) FROM package_items pi WHERE pi.package_uuid = p.uuid AND pi.active = 1),
                    (SELECT COUNT(*) FROM registration r WHERE r.package_uuid = p.uuid)
               FROM packages p
               JOIN events e ON e.uuid = p.event_uuid
               JOIN v_package_price v ON v.package_uuid = p.uuid
              WHERE (?1 = '' OR p.event_uuid = ?1) AND (?2 = '' OR p.name LIKE '%' || ?2 || '%')
              ORDER BY e.date_start DESC, e.name, p.name COLLATE NOCASE",
        )?
        .query_map(params![event, term], |r| {
            Ok(PlanRow {
                uuid: r.get(0)?,
                name: r.get(1)?,
                event: r.get(2)?,
                price: r.get(3)?,
                fixed: r.get(4)?,
                included: r.get(5)?,
                registrations: r.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let events: Vec<(String, String)> = conn
        .prepare(
            "SELECT uuid, name FROM events e WHERE status = 'Activo' OR EXISTS (SELECT 1 FROM packages p WHERE p.event_uuid = e.uuid)
              ORDER BY date_start DESC, name",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let event_name = events.iter().find(|(u, _)| *u == event).map(|(_, n)| n.clone());
    Ok(PlansPage {
        shell: Shell::new(user, "convencion", "planes"),
        total_all: conn.query_row("SELECT COUNT(*) FROM packages", [], |r| r.get(0))?,
        rows,
        new_plan: NewPlanForm { event_uuid: event.clone(), ..Default::default() },
        events,
        active,
        event,
        event_name,
        q: term,
        notice: match q.hecho.as_deref() {
            Some("borrado") => Some("Plan borrado.".into()),
            _ => None,
        },
        error: None,
        open: None,
    })
}

async fn plans_list(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Query(q): Query<PlansQuery>,
) -> Result<Response, AppError> {
    let page = plans_page(&state.db(), &staff.0, &q)?;
    Ok(render_page(&headers, &page)?.into_response())
}

#[derive(Deserialize)]
struct NewPlanBody {
    #[serde(default)]
    event_uuid: String,
    #[serde(default)]
    name: String,
}

async fn plan_create(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Form(f): Form<NewPlanBody>,
) -> Result<Response, AppError> {
    let who = Who::of(&staff.0);
    let res = {
        let mut conn = state.db();
        let tx = conv::begin(&mut conn)?;
        let r = plans::create_plan(&tx, &who, f.event_uuid.trim(), &f.name);
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    match res {
        Ok((uuid, _)) => Ok(go(&headers, &format!("/plans/{uuid}"))),
        Err(e @ (DomainError::Validation(..) | DomainError::NoEvent)) => {
            let q = PlansQuery { event: Some(f.event_uuid.clone()), ..Default::default() };
            let mut page = plans_page(&state.db(), &staff.0, &q)?;
            page.new_plan = NewPlanForm { event_uuid: f.event_uuid, name: f.name, error: Some(e.message()) };
            page.open = Some("dlg-nuevo-plan");
            Ok(render_page_status(StatusCode::UNPROCESSABLE_ENTITY, &headers, &page))
        }
        Err(e) => Ok(e.into_response()),
    }
}

// ===============================================================================================================
// UI-13 Plan
// ===============================================================================================================

#[derive(Clone)]
pub struct PlanItemRow {
    pub uuid: String,
    pub active: bool,
    pub kind: String,
    pub name: String,
    pub room: String,
    pub start: String,
    pub time_start: String,
    pub end: String,
    pub time_end: String,
    pub qty: i64,
    pub price: i64,
    /// After a refused qty: the typed value and the message (422).
    pub typed: Option<String>,
    pub error: Option<String>,
}

#[derive(Template)]
#[template(path = "convention/people_plans/plan_row.html")]
pub struct PlanRowView {
    pub it: PlanItemRow,
}

impl PlanRowView {
    pub fn usd(&self, c: &i64) -> String {
        usd(*c)
    }
    pub fn qty_value(&self) -> String {
        self.it.typed.clone().unwrap_or_else(|| self.it.qty.to_string())
    }
}

/// The price summary (`#plan-price`) and the "Detalles" count (`#plan-count`), page and out of band.
#[derive(Template)]
#[template(path = "convention/people_plans/plan_price.html")]
pub struct PlanPriceView {
    pub price: i64,
    pub fixed: bool,
    pub included: i64,
    /// Activities of the plan's event.
    pub items: i64,
}

impl PlanPriceView {
    pub fn usd(&self, c: &i64) -> String {
        usd(*c)
    }
    fn load(conn: &Connection, plan: &str) -> Result<Self, AppError> {
        let (fixed, included, items): (bool, i64, i64) = conn.query_row(
            "SELECT p.price_override IS NOT NULL,
                    (SELECT COUNT(*) FROM package_items pi WHERE pi.package_uuid = p.uuid AND pi.active = 1),
                    (SELECT COUNT(*) FROM event_items ei WHERE ei.event_uuid = p.event_uuid)
               FROM packages p WHERE p.uuid = ?1",
            [plan],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        Ok(Self { price: plans::plan_price(conn, plan).map_err(dom)?, fixed, included, items })
    }
    fn count_text(&self) -> String {
        format!(
            "{} actividad{} del evento · {} incluida{}",
            self.items,
            if self.items == 1 { "" } else { "es" },
            self.included,
            if self.included == 1 { "" } else { "s" }
        )
    }
    fn oob(&self) -> Result<String, AppError> {
        Ok(autosave::oob("plan-price", &self.render()?) + &autosave::oob("plan-count", &self.count_text()))
    }
}

fn dom(e: DomainError) -> AppError {
    match e {
        DomainError::NotFound => AppError::NotFound,
        DomainError::Forbidden => AppError::Forbidden,
        e => AppError::Internal(format!("convention: {e:?}")),
    }
}

#[derive(Template)]
#[template(path = "convention/people_plans/plan.html", blocks = ["content"])]
pub struct PlanPage {
    pub shell: Shell,
    pub uuid: String,
    pub name: String,
    pub event_uuid: String,
    /// None: the plan's event is missing (state "sin evento").
    pub event_name: Option<String>,
    /// Activities of the plan's event (0: state "sin detalles").
    pub event_items: i64,
    /// Event activities the plan does not have yet (the rebuild dialog).
    pub missing: i64,
    pub registrations: i64,
    pub f_event: FieldView,
    pub f_name: FieldView,
    pub f_fixed: FieldView,
    pub price: PlanPriceView,
    pub count_text: String,
    pub rows: Vec<PlanItemRow>,
    pub notice: Option<String>,
    pub open: Option<&'static str>,
}

impl Page for PlanPage {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

impl PlanPage {
    pub fn row_html(&self, it: &PlanItemRow) -> String {
        PlanRowView { it: it.clone() }.render().unwrap_or_default()
    }
}

fn plan_url(id: &str) -> String {
    format!("/plans/{id}")
}

fn plan_field(conn: &Connection, id: &str, name: &str, saved: String, event_uuid: &str) -> Result<FieldView, AppError> {
    Ok(match name {
        "event_uuid" => {
            let mut opts = active_options(conn)?;
            if !opts.iter().any(|(u, _)| u == event_uuid) {
                // The plan's own event stays visible when it is no longer active (BR-EVT-02 limits new choices only).
                let cur: Option<(String, Option<String>)> = conn
                    .query_row("SELECT name, date_start FROM events WHERE uuid = ?1", [event_uuid], |r| {
                        Ok((r.get(0)?, r.get(1)?))
                    })
                    .optional()?;
                match cur {
                    Some((n, d)) => opts.insert(0, (event_uuid.to_owned(), event_label(&n, &d))),
                    None => opts.insert(0, (String::new(), "Escoja un evento…".into())),
                }
            }
            FieldView::new(plan_url(id), "event_uuid", "pl-ev", Control::Select(opts), saved)
        }
        "name" => FieldView::new(plan_url(id), "name", "pl-nombre", Control::Text, saved),
        _ => {
            FieldView::new(plan_url(id), "price_override", "pl-fijo", Control::Money, saved).hint("Vacío = calculado.")
        }
    })
}

fn plan_items(conn: &Connection, plan: &str) -> rusqlite::Result<Vec<PlanItemRow>> {
    conn.prepare(
        "SELECT pi.uuid, pi.active, ei.type, ei.name, ei.location_room, ei.date_start, ei.time_start, ei.date_end,
                ei.time_end, pi.ticket_qty, pi.price
           FROM package_items pi JOIN event_items ei ON ei.uuid = pi.event_item_uuid
          WHERE pi.package_uuid = ?1
          ORDER BY ei.sort_order, ei.date_start, ei.time_start, ei.uuid",
    )?
    .query_map([plan], |r| {
        Ok(PlanItemRow {
            uuid: r.get(0)?,
            active: r.get::<_, i64>(1)? == 1,
            kind: r.get(2)?,
            name: r.get(3)?,
            room: text(r.get(4)?),
            start: date_of(&r.get(5)?),
            time_start: time_of(&r.get(6)?),
            end: date_of(&r.get(7)?),
            time_end: time_of(&r.get(8)?),
            qty: r.get(9)?,
            price: r.get(10)?,
            typed: None,
            error: None,
        })
    })?
    .collect()
}

fn plan_page_of(conn: &Connection, user: &CurrentUser, id: &str) -> Result<PlanPage, AppError> {
    let (name, event_uuid, fixed): (String, String, Option<i64>) = conn
        .query_row("SELECT name, event_uuid, price_override FROM packages WHERE uuid = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?
        .ok_or(AppError::NotFound)?;
    let event_name: Option<String> =
        conn.query_row("SELECT name FROM events WHERE uuid = ?1", [&event_uuid], |r| r.get(0)).optional()?;
    let event_items: i64 =
        conn.query_row("SELECT COUNT(*) FROM event_items WHERE event_uuid = ?1", [&event_uuid], |r| r.get(0))?;
    let missing: i64 = conn.query_row(
        "SELECT COUNT(*) FROM event_items ei WHERE ei.event_uuid = ?1
            AND NOT EXISTS (SELECT 1 FROM package_items pi WHERE pi.package_uuid = ?2 AND pi.event_item_uuid = ei.uuid)",
        params![event_uuid, id],
        |r| r.get(0),
    )?;
    let price = PlanPriceView::load(conn, id)?;
    let mut shell = Shell::new(user, "convencion", "planes");
    shell.autosave = true;
    Ok(PlanPage {
        shell,
        uuid: id.to_owned(),
        f_event: plan_field(conn, id, "event_uuid", event_uuid.clone(), &event_uuid)?,
        f_name: plan_field(conn, id, "name", name.clone(), &event_uuid)?,
        f_fixed: plan_field(
            conn,
            id,
            "price_override",
            registries::show_money(&fixed.map(Value::Integer).unwrap_or(Value::Null)),
            &event_uuid,
        )?,
        name,
        event_name,
        event_items,
        missing,
        registrations: plans::registration_count(conn, id)?,
        count_text: price.count_text(),
        price,
        rows: plan_items(conn, id)?,
        event_uuid,
        notice: None,
        open: None,
    })
}

#[derive(Deserialize, Default)]
struct PlanQuery {
    hecho: Option<String>,
    n: Option<i64>,
}

async fn plan_page(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<PlanQuery>,
) -> Result<Response, AppError> {
    let mut page = plan_page_of(&state.db(), &staff.0, &id)?;
    page.notice = match q.hecho.as_deref() {
        Some("reconstruido") => {
            let n = q.n.unwrap_or(0);
            Some(format!("{n} detalle{} añadido{}.", if n == 1 { "" } else { "s" }, if n == 1 { "" } else { "s" }))
        }
        Some("quitado") => Some("Detalle quitado del plan.".into()),
        _ => None,
    };
    Ok(render_page(&headers, &page)?.into_response())
}

fn plan_title_oob(conn: &Connection, id: &str) -> Result<String, AppError> {
    let (name, ev): (String, Option<String>) = conn.query_row(
        "SELECT p.name, e.name FROM packages p LEFT JOIN events e ON e.uuid = p.event_uuid WHERE p.uuid = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let esc = |s: &str| askama::filters::escape(s, askama::filters::Html).map(|s| s.to_string()).unwrap_or_default();
    let title = match ev {
        Some(e) => format!("<b>Plan</b> · {} · {}", esc(&e), esc(&name)),
        None => format!("<b>Plan</b> · {}", esc(&name)),
    };
    Ok(autosave::oob("plan-title", &title) + &autosave::oob("plan-crumb", &esc(&name)))
}

async fn plan_patch(
    State(state): State<AppState>,
    staff: RequireStaff,
    r: Option<Extension<RequestId>>,
    Path(id): Path<String>,
    field: OneField,
) -> Result<Response, AppError> {
    let who = Who::of(&staff.0);
    let mut conn = state.db();
    let outcome =
        match registries::patch(&mut conn, &who, rid(&r), Target::new(Resource::Plan, &id), &field.name, &field.value)?
        {
            Patched::Unknown => return Ok(autosave::contract_error("Este campo no se puede cambiar aquí.")),
            Patched::Done(o) => o,
        };
    let def = registries::PLAN.get(&field.name).ok_or(AppError::NotFound)?;
    let event_uuid = autosave::text_of(&stored(&conn, "packages", "event_uuid", &id)?);
    match outcome {
        Outcome::Saved { new, changed, .. } => {
            if changed && def.name == "event_uuid" {
                // OP-EVT-04 ran in the after-rule: the items table changed, so the page reloads.
                let view = plan_field(&conn, &id, def.name, def.display(&new), &event_uuid)?;
                let mut res = autosave::respond(&view, "");
                res.headers_mut().insert("hx-refresh", HeaderValue::from_static("true"));
                return Ok(res);
            }
            let extra = match (changed, def.name) {
                (true, "name") => plan_title_oob(&conn, &id)?,
                (_, "price_override") => PlanPriceView::load(&conn, &id)?.oob()?,
                _ => String::new(),
            };
            let view = plan_field(&conn, &id, def.name, def.display(&new), &event_uuid)?;
            Ok(autosave::respond(&view, &extra))
        }
        Outcome::Invalid(m) => {
            let old = stored(&conn, "packages", def.column, &id)?;
            let view = plan_field(&conn, &id, def.name, def.display(&old), &event_uuid)?.refused(&field.value, m);
            Ok(autosave::respond(&view, ""))
        }
    }
}

async fn plan_delete(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let who = Who::of(&staff.0);
    let res = {
        let mut conn = state.db();
        let tx = conv::begin(&mut conn)?;
        let r = plans::delete_plan(&tx, &who, &id);
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    match res {
        Ok(()) => Ok(go(&headers, "/plans?hecho=borrado")),
        Err(e @ DomainError::HasRegistrations(_)) => {
            // From the list (Referer /plans) the list shows the refusal; from the plan page its dialog opens.
            let from_list = headers
                .get("referer")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|r| r.contains("/plans?") || r.ends_with("/plans"));
            if from_list {
                let mut page = plans_page(&state.db(), &staff.0, &PlansQuery::default())?;
                page.error = Some(e.message());
                return Ok(render_page_status(StatusCode::CONFLICT, &headers, &page));
            }
            let mut page = plan_page_of(&state.db(), &staff.0, &id)?;
            page.open = Some("dlg-borrar-plan");
            Ok(render_page_status(StatusCode::CONFLICT, &headers, &page))
        }
        Err(e) => Ok(e.into_response()),
    }
}

async fn plan_build(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let who = Who::of(&staff.0);
    let res = {
        let mut conn = state.db();
        let tx = conv::begin(&mut conn)?;
        let r = plans::build_plan_items(&tx, &who, &id);
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    match res {
        Ok(n) => Ok(go(&headers, &format!("/plans/{id}?hecho=reconstruido&n={n}"))),
        Err(DomainError::NoEventItems | DomainError::NoEvent) => {
            // The page already shows the state banner; the refusal keeps it (409).
            let page = plan_page_of(&state.db(), &staff.0, &id)?;
            Ok(render_page_status(StatusCode::CONFLICT, &headers, &page))
        }
        Err(e) => Ok(e.into_response()),
    }
}

/// One plan item row with its plan uuid.
fn plan_item_row(conn: &Connection, item: &str) -> Result<(String, PlanItemRow), AppError> {
    let plan: String = conn
        .query_row("SELECT package_uuid FROM package_items WHERE uuid = ?1", [item], |r| r.get(0))
        .optional()?
        .ok_or(AppError::NotFound)?;
    let row = plan_items(conn, &plan)?.into_iter().find(|r| r.uuid == item).ok_or(AppError::NotFound)?;
    Ok((plan, row))
}

async fn plan_item_patch(
    State(state): State<AppState>,
    staff: RequireStaff,
    r: Option<Extension<RequestId>>,
    Path(id): Path<String>,
    field: OneField,
) -> Result<Response, AppError> {
    let who = Who::of(&staff.0);
    let mut conn = state.db();
    let outcome = match registries::patch(
        &mut conn,
        &who,
        rid(&r),
        Target::new(Resource::PlanItem, &id),
        &field.name,
        &field.value,
    )? {
        Patched::Unknown => return Ok(autosave::contract_error("Este campo no se puede cambiar aquí.")),
        Patched::Done(o) => o,
    };
    let (plan, mut row) = plan_item_row(&conn, &id)?;
    let status = match outcome {
        Outcome::Saved { .. } => StatusCode::OK,
        Outcome::Invalid(m) => {
            if field.name == "ticket_qty" {
                row.typed = Some(field.value.clone());
            }
            row.error = Some(m);
            StatusCode::UNPROCESSABLE_ENTITY
        }
    };
    let mut html = PlanRowView { it: row }.render()?;
    if status == StatusCode::OK {
        html.push_str(&PlanPriceView::load(&conn, &plan)?.oob()?);
    }
    Ok((status, Html(html)).into_response())
}

async fn plan_item_delete(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let who = Who::of(&staff.0);
    let res = {
        let mut conn = state.db();
        let plan: Option<String> =
            conn.query_row("SELECT package_uuid FROM package_items WHERE uuid = ?1", [&id], |r| r.get(0)).optional()?;
        let Some(plan) = plan else { return Err(AppError::NotFound) };
        let tx = conv::begin(&mut conn)?;
        let r = plans::delete_plan_item(&tx, &who, &id).map(|_| plan);
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    match res {
        Ok(plan) => Ok(go(&headers, &format!("/plans/{plan}?hecho=quitado"))),
        Err(e) => Ok(e.into_response()),
    }
}
