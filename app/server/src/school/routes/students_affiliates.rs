//! Phase-2 pane "students_affiliates": UI-14 Estudiantes, UI-15 Estudiante, UI-16 Afiliados, UI-17 Afiliado, EXT-02.
//!
//! * `GET /students` (`?q=`, `?status=`, `?affiliate=` (uuid or `none`), `?page=`); with `HX-Target:
//!   stu-results` only the results block. A plain search (Enter, no htmx) with exactly one Quick Find hit opens the
//!   student (OP-STU-05). `GET /students/new` opens the create dialog; `POST /students` creates (OP-STU-01).
//! * `GET /students/{id}` (`?tab=certifications|courses|seminars|short-courses|details`; the tabs are links with
//!   `hx-get` + `hx-push-url`), `PATCH /students/{id}` (one field; `member_uuid` takes the affiliate picked from the
//!   typeahead, "Name (A-1001)", and copies its name into `compania`, BR-STU-02), `DELETE /students/{id}` (OP-STU-04,
//!   AP-04: staff 403, enrollments 409), `POST /students/{id}/duplicate` (OP-STU-02),
//!   `POST /students/{id}/interests` and `DELETE /students/{id}/interests/{interest}` (OP-STU-03, BR-STU-05).
//! * `PATCH /student-certifications/{id}` (graduation date; the whole row comes back: "Certificado" shows only when
//!   BR-CRT-04 allows it).
//! * `GET /affiliates/options?q=` (typeahead `<option>`s), `GET /affiliates` (`?q=`, `?tipo=`, `?miembro=`),
//!   `GET /affiliates/new`, `POST /affiliates`, `GET|PATCH|DELETE /affiliates/{id}` (OP-AFF-01; delete unlinks the
//!   students, BR-AFF-02).
//!
//! The enrollment dialog (UI-20) belongs to the offerings pane: "Matricular" links to
//! `/students/{id}/enrollments/{kind}/new`. The transcript and certificate prints are I5 (their links may 404).

use crate::audit::RequestId;
use crate::auth::{CurrentUser, RequireStaff, Role, Shell};
use crate::autosave::{self, Control, FieldView, OneField, Outcome};
use crate::convention::registries::show_date;
use crate::error::AppError;
use crate::school::registries::{self as reg, Patched, Resource};
use crate::school::{self, affiliates, certifications, enrollments, letter, students, DomainError, Kind, Who};
use crate::web::{is_htmx, render_page, render_page_status, urlencode, Page};
use crate::AppState;
use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Extension, Form, Router};
use rusqlite::types::Value;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/students", get(students_list).post(student_create))
        .route("/students/new", get(student_new))
        .route("/students/{id}", get(student_page).patch(student_patch).delete(student_delete))
        .route("/students/{id}/duplicate", post(student_duplicate))
        .route("/students/{id}/interests", post(interest_add))
        .route("/students/{id}/interests/{interest}", delete(interest_remove))
        .route("/student-certifications/{id}", patch(student_cert_patch))
        .route("/affiliates", get(affiliates_list).post(affiliate_create))
        .route("/affiliates/new", get(affiliate_new))
        .route("/affiliates/options", get(affiliate_options))
        .route("/affiliates/{id}", get(affiliate_page).patch(affiliate_patch).delete(affiliate_delete))
}

/// Students per page (UI-14 pager).
const PAGE_SIZE: i64 = 50;
/// Students shown on UI-17.
const AFF_STUDENTS: i64 = 10;

fn usd(c: i64) -> String {
    crate::seed::usd(c)
}

fn text(v: Option<String>) -> String {
    v.unwrap_or_default()
}

/// "1,248" (title counts).
fn grouped(n: i64) -> String {
    usd(n * 100).trim_start_matches('$').trim_end_matches(".00").to_owned()
}

fn date_of(v: &Option<String>) -> String {
    v.as_ref().map(|s| show_date(&Value::Text(s.clone()))).unwrap_or_default()
}

/// "12" or "4.5" (credits and hours).
fn num(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

fn rid(r: &Option<Extension<RequestId>>) -> Option<&str> {
    r.as_ref().map(|r| r.0 .0.as_str())
}

fn esc(s: &str) -> String {
    askama::filters::escape(s, askama::filters::Html).map(|s| s.to_string()).unwrap_or_default()
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

fn status_tag(status: &str) -> &'static str {
    match status {
        "ACTIVO" | "Confirmado" | "Asistió" => " ok",
        "Pendiente" | "No Canceló" => " warn",
        "Completado" => " info",
        "Canceló" | "Cancelado" => " err",
        _ => "",
    }
}

/// E-mail check of the create dialog (same rule as the autosave field).
fn email_ok(s: &str) -> bool {
    let t = s.trim();
    t.is_empty()
        || (!t.contains(' ')
            && t.split_once('@').is_some_and(|(a, d)| !a.is_empty() && d.contains('.') && !d.ends_with('.')))
}

// ===============================================================================================================
// Affiliate picker (BR-STU-02): the typeahead shows "Name (A-1001)"; the server resolves it to one key.
// ===============================================================================================================

/// "Banco Ejemplo (A-1001)".
fn affiliate_choice(name: &str, account: Option<i64>) -> String {
    match account {
        Some(n) => format!("{name} ({})", affiliates::account_label(Some(n))),
        None => name.to_owned(),
    }
}

/// The affiliate uuid of a typeahead value: "Name (A-1001)" by account number, else a unique exact name, else a
/// uuid. `Ok(Ok(None))` for empty (unlink); `Ok(Err(()))` when no single affiliate matches.
fn resolve_affiliate(conn: &Connection, raw: &str) -> Result<Result<Option<String>, ()>, AppError> {
    let t = raw.trim();
    if t.is_empty() {
        return Ok(Ok(None));
    }
    if let Some(open) = t.rfind("(A-") {
        if let Some(n) = t[open + 3..].strip_suffix(')').and_then(|n| n.trim().parse::<i64>().ok()) {
            if let Some(u) = conn
                .query_row("SELECT uuid FROM affiliates WHERE account_num = ?1", [n], |r| r.get::<_, String>(0))
                .optional()?
            {
                return Ok(Ok(Some(u)));
            }
        }
    }
    let by_name: Vec<String> = conn
        .prepare("SELECT uuid FROM affiliates WHERE socio = ?1 OR uuid = ?1")?
        .query_map([t], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    if by_name.len() == 1 {
        return Ok(Ok(by_name.into_iter().next()));
    }
    Ok(Err(()))
}

fn affiliate_choices(conn: &Connection, q: &str) -> rusqlite::Result<Vec<String>> {
    let q = q.trim();
    let q = q.rfind(" (A-").map(|i| &q[..i]).unwrap_or(q);
    conn.prepare(
        "SELECT socio, account_num FROM affiliates
          WHERE ?1 = '' OR socio LIKE '%' || ?1 || '%' OR other_name LIKE '%' || ?1 || '%'
          ORDER BY socio COLLATE NOCASE LIMIT 20",
    )?
    .query_map([q], |r| Ok(affiliate_choice(&r.get::<_, String>(0)?, r.get(1)?)))?
    .collect()
}

#[derive(Deserialize, Default)]
struct OptionsQuery {
    q: Option<String>,
    member_uuid: Option<String>,
    afiliado: Option<String>,
}

/// `GET /affiliates/options`: the `<option>`s of the affiliate datalist (typeahead).
async fn affiliate_options(
    State(state): State<AppState>,
    _staff: RequireStaff,
    Query(q): Query<OptionsQuery>,
) -> Result<Response, AppError> {
    let term = q.q.or(q.member_uuid).or(q.afiliado).unwrap_or_default();
    let opts = affiliate_choices(&state.db(), &term)?;
    let html: String = opts.iter().map(|o| format!("<option value=\"{}\"></option>", esc(o))).collect();
    Ok(Html(html).into_response())
}

/// UI-15 Afiliado: the autosave text field with the typeahead datalist. The control sends `member_uuid`.
#[derive(Template)]
#[template(path = "school/students_affiliates/affiliate_field.html")]
pub struct AffiliateField {
    pub f: FieldView,
    pub options: Vec<String>,
}

// ===============================================================================================================
// UI-14 Estudiantes
// ===============================================================================================================

pub struct StudentRow {
    pub uuid: String,
    pub nombre: String,
    pub compania: String,
    pub email: String,
    pub phone: String,
    pub status: String,
    pub enrollments: i64,
}

impl StudentRow {
    pub fn tag(&self) -> &'static str {
        status_tag(&self.status)
    }
}

#[derive(Default)]
pub struct NewStudentForm {
    pub nombre: String,
    pub afiliado: String,
    pub email: String,
    pub telefono: String,
    pub errors: Vec<(&'static str, &'static str, String)>,
}

impl NewStudentForm {
    pub fn err(&self, field: &str) -> Option<&str> {
        self.errors.iter().find(|e| e.0 == field).map(|e| e.2.as_str())
    }
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

#[derive(Template)]
#[template(path = "school/students_affiliates/students_results.html")]
pub struct StudentResults {
    pub rows: Vec<StudentRow>,
    pub total_all: i64,
    pub total: i64,
    pub page: i64,
    pub pages: i64,
    pub q: String,
    pub status: String,
    pub affiliate: String,
    pub is_admin: bool,
    pub notice: Option<String>,
}

impl StudentResults {
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
    pub fn link(&self, page: i64) -> String {
        let mut s = format!("/students?page={page}");
        for (k, v) in [("q", &self.q), ("status", &self.status), ("affiliate", &self.affiliate)] {
            if !v.is_empty() {
                s.push_str(&format!("&{k}={}", urlencode(v)));
            }
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
#[template(path = "school/students_affiliates/students_list.html", blocks = ["content"])]
pub struct StudentsPage {
    pub shell: Shell,
    pub results: StudentResults,
    pub active: i64,
    /// (uuid, name) of every affiliate, for the filter.
    pub affiliates: Vec<(String, String)>,
    pub new_student: NewStudentForm,
    pub choices: Vec<String>,
    pub notice: Option<&'static str>,
    pub open: Option<&'static str>,
}

impl Page for StudentsPage {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

impl StudentsPage {
    pub fn n(&self, v: &i64) -> String {
        grouped(*v)
    }
    pub fn statuses(&self) -> [&'static str; 2] {
        ["ACTIVO", "INACTIVO"]
    }
}

#[derive(Deserialize, Default)]
pub struct StudentsQuery {
    q: Option<String>,
    status: Option<String>,
    affiliate: Option<String>,
    page: Option<i64>,
    hecho: Option<String>,
}

/// UI-14 results: name, company, e-mail or student number (as `students::quick_find`); status (BR-STU-03, from
/// `v_student_academic`) and affiliate filters; name order; one page.
fn students_search(conn: &Connection, q: &StudentsQuery, user: &CurrentUser) -> Result<StudentResults, AppError> {
    let term = q.q.clone().unwrap_or_default().trim().to_owned();
    let status = q.status.clone().unwrap_or_default().trim().to_owned();
    let affiliate = q.affiliate.clone().unwrap_or_default().trim().to_owned();
    let filter = "(?1 = '' OR s.nombre LIKE '%' || ?1 || '%' OR s.compania LIKE '%' || ?1 || '%'
                   OR s.email LIKE '%' || ?1 || '%' OR s.email_work LIKE '%' || ?1 || '%' OR s.student_id = ?1)
                  AND (?2 = '' OR a.status = ?2)
                  AND (?3 = '' OR (?3 = 'none' AND s.member_uuid IS NULL) OR s.member_uuid = ?3)";
    let from = "FROM students s JOIN v_student_academic a ON a.student_uuid = s.uuid";
    let total_all: i64 = conn.query_row("SELECT COUNT(*) FROM students", [], |r| r.get(0))?;
    let total: i64 =
        conn.query_row(&format!("SELECT COUNT(*) {from} WHERE {filter}"), params![term, status, affiliate], |r| {
            r.get(0)
        })?;
    let pages = ((total + PAGE_SIZE - 1) / PAGE_SIZE).max(1);
    let page = q.page.unwrap_or(1).clamp(1, pages);
    let rows = conn
        .prepare(&format!(
            "SELECT s.uuid, s.nombre, s.compania, COALESCE(s.email, s.email_work),
                    COALESCE(s.telefono_r, s.phone_mobile, s.telefono_t), a.status,
                    (SELECT COUNT(*) FROM enrollments_course e WHERE e.student_uuid = s.uuid)
                  + (SELECT COUNT(*) FROM enrollments_seminars e WHERE e.student_uuid = s.uuid)
                  + (SELECT COUNT(*) FROM enrollments_short_course e WHERE e.student_uuid = s.uuid)
             {from} WHERE {filter}
             ORDER BY s.nombre COLLATE NOCASE, s.uuid LIMIT ?4 OFFSET ?5"
        ))?
        .query_map(params![term, status, affiliate, PAGE_SIZE, (page - 1) * PAGE_SIZE], |r| {
            Ok(StudentRow {
                uuid: r.get(0)?,
                nombre: r.get(1)?,
                compania: text(r.get(2)?),
                email: text(r.get(3)?),
                phone: text(r.get(4)?),
                status: r.get(5)?,
                enrollments: r.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(StudentResults {
        rows,
        total_all,
        total,
        page,
        pages,
        q: term,
        status,
        affiliate,
        is_admin: user.role == Role::Admin,
        notice: None,
    })
}

fn affiliate_names(conn: &Connection) -> rusqlite::Result<Vec<(String, String)>> {
    conn.prepare("SELECT uuid, socio FROM affiliates ORDER BY socio COLLATE NOCASE")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect()
}

fn students_page(conn: &Connection, user: &CurrentUser, q: &StudentsQuery) -> Result<StudentsPage, AppError> {
    let active: i64 =
        conn.query_row("SELECT COUNT(*) FROM v_student_academic WHERE status = 'ACTIVO'", [], |r| r.get(0))?;
    Ok(StudentsPage {
        shell: Shell::new(user, "escuela", "estudiantes"),
        results: students_search(conn, q, user)?,
        active,
        affiliates: affiliate_names(conn)?,
        new_student: NewStudentForm::default(),
        choices: affiliate_choices(conn, "")?,
        notice: match q.hecho.as_deref() {
            Some("borrado") => Some("Estudiante borrado."),
            _ => None,
        },
        open: None,
    })
}

async fn students_list(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Query(q): Query<StudentsQuery>,
) -> Result<Response, AppError> {
    let conn = state.db();
    let term = q.q.clone().unwrap_or_default();
    // OP-STU-05: a plain search (Enter) with one hit opens the student; 0 or many show the list.
    if !is_htmx(&headers) && !term.trim().is_empty() && q.status.as_deref().unwrap_or("").is_empty() {
        if let students::Found::One(uuid) = students::quick_find(&conn, &term)? {
            if q.affiliate.as_deref().unwrap_or("").is_empty() {
                return Ok(Redirect::to(&format!("/students/{uuid}")).into_response());
            }
        }
    }
    if headers.get("hx-target").is_some_and(|v| v == "stu-results") {
        return Ok(Html(students_search(&conn, &q, &staff.0)?.render()?).into_response());
    }
    let page = students_page(&conn, &staff.0, &q)?;
    Ok(render_page(&headers, &page)?.into_response())
}

async fn student_new(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let mut page = students_page(&state.db(), &staff.0, &StudentsQuery::default())?;
    page.open = Some("dlg-nuevo");
    Ok(render_page(&headers, &page)?.into_response())
}

#[derive(Deserialize)]
struct NewStudentBody {
    #[serde(default)]
    nombre: String,
    #[serde(default)]
    afiliado: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    telefono: String,
}

/// OP-STU-01 "Nuevo estudiante" (Guardar): name, affiliate by key (its name goes to `compania`), e-mail, phone;
/// BR-STU-01 adds the certification rows; then the student opens.
async fn student_create(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Form(f): Form<NewStudentBody>,
) -> Result<Response, AppError> {
    let who = Who::of(&staff.0);
    let mut errors: Vec<(&'static str, &'static str, String)> = Vec::new();
    if f.nombre.trim().is_empty() {
        errors.push(("nombre", "Nombre", "Escriba el nombre.".into()));
    }
    let member = match resolve_affiliate(&state.db(), &f.afiliado)? {
        Ok(m) => m,
        Err(_) => {
            errors.push(("afiliado", "Afiliado", "Escoja un afiliado de la lista.".into()));
            None
        }
    };
    if !email_ok(&f.email) {
        errors.push(("email", "E-mail", "Escriba un e-mail válido (nombre@ejemplo.com).".into()));
    }
    let result = if errors.is_empty() {
        let mut conn = state.db();
        let tx = school::begin(&mut conn)?;
        let ns = students::NewStudent { nombre: f.nombre.clone(), member_uuid: member, email: f.email.clone() };
        match students::create_student(&tx, &who, &ns) {
            Ok(uuid) => {
                let tel = f.telefono.trim();
                if !tel.is_empty() {
                    tx.execute("UPDATE students SET telefono_r = ?1 WHERE uuid = ?2", params![tel, uuid])?;
                }
                tx.commit()?;
                Ok(uuid)
            }
            Err(DomainError::Validation(field, m)) => {
                errors.push((field, if field == "email" { "E-mail" } else { "Nombre" }, m));
                Err(None)
            }
            Err(e) => Err(Some(e)),
        }
    } else {
        Err(None)
    };
    match result {
        Ok(uuid) => Ok(go(&headers, &format!("/students/{uuid}"))),
        Err(Some(e)) => Ok(e.into_response()),
        Err(None) => {
            let mut page = students_page(&state.db(), &staff.0, &StudentsQuery::default())?;
            page.new_student =
                NewStudentForm { nombre: f.nombre, afiliado: f.afiliado, email: f.email, telefono: f.telefono, errors };
            page.open = Some("dlg-nuevo");
            Ok(render_page_status(StatusCode::UNPROCESSABLE_ENTITY, &headers, &page))
        }
    }
}

// ===============================================================================================================
// UI-15 Estudiante
// ===============================================================================================================

/// The tabs of UI-15 (`?tab=`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Certs,
    Courses,
    Seminars,
    Shorts,
    Details,
}

impl Tab {
    fn from_query(s: Option<&str>) -> Tab {
        match s {
            Some("courses") => Tab::Courses,
            Some("seminars") => Tab::Seminars,
            Some("short-courses") => Tab::Shorts,
            Some("details") => Tab::Details,
            _ => Tab::Certs,
        }
    }
    pub fn slug(&self) -> &'static str {
        match self {
            Tab::Certs => "certifications",
            Tab::Courses => "courses",
            Tab::Seminars => "seminars",
            Tab::Shorts => "short-courses",
            Tab::Details => "details",
        }
    }
}

/// One row of "Progreso por certificación", as the page and the graduation-date PATCH render it.
#[derive(Template)]
#[template(path = "school/students_affiliates/cert_row.html")]
pub struct CertRow {
    pub p: students::Progress,
    pub date: FieldView,
    pub certificate: bool,
    pub student: String,
}

impl CertRow {
    pub fn pct(&self) -> i64 {
        if self.p.total <= 0 {
            0
        } else {
            (self.p.completed.min(self.p.total) * 100 + self.p.total / 2) / self.p.total
        }
    }
    /// "Faltan" never shows below zero.
    pub fn left(&self) -> i64 {
        self.p.left_count.max(0)
    }
    pub fn avg(&self) -> String {
        match self.p.average_points {
            Some(a) => format!("{a} {}", letter(Some(a)).unwrap_or("")),
            None => "—".into(),
        }
    }
    pub fn credits(&self) -> String {
        num(self.p.total_credits)
    }
    pub fn hours(&self) -> String {
        num(self.p.total_hours)
    }
    pub fn missing(&self) -> String {
        if self.p.courses_left.is_empty() {
            "—".into()
        } else {
            self.p.courses_left.join(", ")
        }
    }
    pub fn name(&self) -> String {
        match &self.p.description {
            Some(d) if !d.trim().is_empty() => format!("{d} ({})", self.p.certification),
            _ => self.p.certification.clone(),
        }
    }
}

fn cert_row(conn: &Connection, student: &str, p: students::Progress) -> Result<CertRow, AppError> {
    let certificate = certifications::certificate_allowed(conn, &p.uuid).unwrap_or(false);
    let date = FieldView::new(
        format!("/student-certifications/{}", p.uuid),
        "graduation_date",
        &format!("gd-{}", p.uuid),
        Control::Text,
        date_of(&p.graduation_date),
    )
    .aria("Fecha de graduación");
    Ok(CertRow { p, date, certificate, student: student.to_owned() })
}

pub struct SemLine {
    pub uuid: String,
    pub name: String,
    pub date: String,
    pub hours: String,
    pub status: String,
    pub cargo: i64,
    pub balance: i64,
}

impl SemLine {
    pub fn tag(&self) -> &'static str {
        status_tag(&self.status)
    }
}

/// Seminar or short-course enrollments of a student (stored price and payment of each enrollment, BR-ENR-01).
fn sem_lines(conn: &Connection, kind: Kind, student: &str) -> rusqlite::Result<Vec<SemLine>> {
    let sql = match kind {
        Kind::Seminar => {
            "SELECT e.uuid, s.seminar, c.date, s.hours, e.status, COALESCE(e.cargo, 0), COALESCE(e.cargo, 0) - e.pago
               FROM enrollments_seminars e JOIN seminars s ON s.uuid = e.seminar_uuid
               LEFT JOIN calendar_seminar c ON c.uuid = e.seminar_calendar_uuid
              WHERE e.student_uuid = ?1 ORDER BY c.date DESC, e.serial DESC"
        }
        _ => {
            "SELECT e.uuid, s.short_course,
                    (SELECT MIN(d.date) FROM calendar_short_course_dates d WHERE d.calendar_short_course_uuid = c.uuid),
                    COALESCE(c.hours, s.hours), e.status, COALESCE(e.cargo, 0), COALESCE(e.cargo, 0) - e.pago
               FROM enrollments_short_course e JOIN short_courses s ON s.uuid = e.short_course_uuid
               LEFT JOIN calendar_short_course c ON c.uuid = e.short_course_calendar_uuid
              WHERE e.student_uuid = ?1 ORDER BY 3 DESC, e.serial DESC"
        }
    };
    conn.prepare(sql)?
        .query_map([student], |r| {
            Ok(SemLine {
                uuid: r.get(0)?,
                name: r.get(1)?,
                date: date_of(&r.get(2)?),
                hours: r.get::<_, Option<f64>>(3)?.map(num).unwrap_or_default(),
                status: text(r.get(4)?),
                cargo: r.get(5)?,
                balance: r.get(6)?,
            })
        })?
        .collect()
}

pub struct Interest {
    pub uuid: String,
    pub name: String,
    pub date: String,
}

/// "Cert. de interés" (OP-STU-03, BR-STU-05): the chips, the picker and the message of the last refusal.
#[derive(Template)]
#[template(path = "school/students_affiliates/interests.html")]
pub struct Interests {
    pub student: String,
    pub csrf: String,
    pub items: Vec<Interest>,
    /// (uuid, name) of the certifications not in the list.
    pub options: Vec<(String, String)>,
    pub error: Option<String>,
}

impl Interests {
    pub fn full(&self) -> bool {
        self.items.len() as i64 >= students::MAX_INTERESTS
    }
}

fn cert_label(certification: &str, description: Option<String>) -> String {
    match description {
        Some(d) if !d.trim().is_empty() => format!("{d} ({certification})"),
        _ => certification.to_owned(),
    }
}

fn interests_of(conn: &Connection, student: &str, csrf: &str) -> Result<Interests, AppError> {
    let items = conn
        .prepare(
            "SELECT i.uuid, c.certification, c.description, i.date
               FROM student_certification_interests i JOIN certifications c ON c.uuid = i.certification_uuid
              WHERE i.student_uuid = ?1 ORDER BY i.repetition",
        )?
        .query_map([student], |r| {
            Ok(Interest {
                uuid: r.get(0)?,
                name: cert_label(&r.get::<_, String>(1)?, r.get(2)?),
                date: date_of(&r.get(3)?),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let options = conn
        .prepare(
            "SELECT c.uuid, c.certification, c.description FROM certifications c
              WHERE NOT EXISTS (SELECT 1 FROM student_certification_interests i WHERE i.student_uuid = ?1 AND i.certification_uuid = c.uuid)
              ORDER BY c.certification",
        )?
        .query_map([student], |r| Ok((r.get(0)?, cert_label(&r.get::<_, String>(1)?, r.get(2)?))))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Interests { student: student.to_owned(), csrf: csrf.to_owned(), items, options, error: None })
}

/// The "Ver en el mapa" link (EXT-02), or nothing without an address.
fn map_link(url: Option<String>) -> String {
    match url {
        Some(u) => format!(
            "<a class=\"field-hint\" href=\"{}\" target=\"_blank\" rel=\"noopener\">Ver en el mapa ↗</a>",
            esc(&u)
        ),
        None => "<span class=\"field-hint\">Sin dirección para el mapa.</span>".into(),
    }
}

/// The columns of the "Datos personales" tab (registry `STUDENT` names).
const STU_FIELDS: [&str; 21] = [
    "nombre",
    "member_uuid",
    "compania",
    "company_position",
    "company_department",
    "nmls",
    "email",
    "email_work",
    "telefono_r",
    "telefono_t",
    "phone_mobile",
    "dir_residencial",
    "residencial_2",
    "home_city",
    "home_state",
    "home_zip",
    "emergency_contact",
    "emergency_phone",
    "emergency_contact_relationship",
    "notes",
    "student_id",
];

/// The autosave field of one `students` column.
fn student_field(id: &str, name: &str, saved: String) -> FieldView {
    let url = format!("/students/{id}");
    let (dom, aria, control): (&str, Option<&str>, Control) = match name {
        "nombre" => ("es-nom", None, Control::Text),
        "compania" => ("es-comp", None, Control::Text),
        "company_position" => ("es-puesto", Some("Puesto"), Control::Text),
        "company_department" => ("es-depto", Some("Departamento"), Control::Text),
        "nmls" => ("es-nmls", None, Control::Text),
        "student_id" => ("es-num", None, Control::Text),
        "email" => ("es-mail", None, Control::Text),
        "email_work" => ("es-mailt", None, Control::Text),
        "telefono_r" => ("es-telr", Some("Teléfono residencial"), Control::Text),
        "telefono_t" => ("es-telt", Some("Teléfono trabajo"), Control::Text),
        "phone_mobile" => ("es-movil", Some("Móvil"), Control::Text),
        "dir_residencial" => ("es-d1", None, Control::Text),
        "residencial_2" => ("es-d2", None, Control::Text),
        "home_city" => ("es-pueblo", Some("Pueblo-Ciudad"), Control::Text),
        "home_state" => ("es-estado", Some("Estado"), Control::Text),
        "home_zip" => ("es-zip", Some("Código postal"), Control::Text),
        "emergency_contact" => ("es-emc", None, Control::Text),
        "emergency_phone" => ("es-emt", Some("Teléfono de emergencia"), Control::Text),
        "emergency_contact_relationship" => ("es-emp", Some("Parentesco"), Control::Text),
        "notes" => ("es-not", None, Control::Textarea(2)),
        _ => ("es-x", None, Control::Text),
    };
    let f = FieldView::new(url, name, dom, control, saved);
    match aria {
        Some(a) => f.aria(a),
        None => f,
    }
}

fn affiliate_field(conn: &Connection, id: &str, saved: String) -> Result<AffiliateField, AppError> {
    let f = FieldView::new(format!("/students/{id}"), "member_uuid", "es-afi", Control::Text, saved)
        .hint("Escoger un afiliado copia su nombre en Compañía (BR-STU-02).");
    Ok(AffiliateField { f, options: affiliate_choices(conn, "")? })
}

/// The affiliate typeahead value of a student ("Name (A-1001)"), or empty.
fn student_affiliate_choice(conn: &Connection, id: &str) -> Result<String, AppError> {
    Ok(conn
        .query_row(
            "SELECT a.socio, a.account_num FROM students s JOIN affiliates a ON a.uuid = s.member_uuid WHERE s.uuid = ?1",
            [id],
            |r| Ok(affiliate_choice(&r.get::<_, String>(0)?, r.get(1)?)),
        )
        .optional()?
        .unwrap_or_default())
}

#[derive(Template)]
#[template(path = "school/students_affiliates/student.html", blocks = ["content"])]
pub struct StudentPage {
    pub shell: Shell,
    pub uuid: String,
    pub nombre: String,
    pub compania: String,
    pub number: String,
    pub email: String,
    pub academic: students::Academic,
    pub tab: Tab,
    pub cert_count: usize,
    pub graduated: usize,
    pub certs: Vec<CertRow>,
    pub courses: Vec<enrollments::CourseLine>,
    pub seminars: Vec<SemLine>,
    pub shorts: Vec<SemLine>,
    pub enrollments: i64,
    /// Details tab fields (`STU_FIELDS` order, without `member_uuid`).
    pub fields: Vec<FieldView>,
    pub affiliate: AffiliateField,
    pub interests: Interests,
    pub map: String,
    pub new_student: NewStudentForm,
    pub choices: Vec<String>,
    pub open: Option<&'static str>,
}

impl Page for StudentPage {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

impl StudentPage {
    pub fn usd(&self, c: &i64) -> String {
        usd(*c)
    }
    pub fn f(&self, name: &str) -> String {
        STU_FIELDS
            .iter()
            .filter(|n| **n != "member_uuid")
            .position(|n| *n == name)
            .and_then(|i| self.fields.get(i))
            .map(|f| f.html())
            .unwrap_or_default()
    }
    pub fn tab_link(&self, t: &str) -> String {
        format!("/students/{}?tab={t}", self.uuid)
    }
    pub fn avg(&self) -> String {
        self.academic.average_points.map(|a| a.to_string()).unwrap_or_else(|| "—".into())
    }
    pub fn avg_letter(&self) -> &'static str {
        letter(self.academic.average_points).unwrap_or("")
    }
    pub fn credits(&self) -> String {
        num(self.academic.total_credits)
    }
    pub fn hours(&self) -> String {
        num(self.academic.total_hours)
    }
    pub fn last_date(&self) -> String {
        date_of(&self.academic.last_course_date)
    }
    pub fn status_tag(&self) -> &'static str {
        status_tag(&self.academic.status)
    }
    pub fn balance_usd(&self) -> String {
        usd(self.courses.iter().map(|c| c.balance).sum())
    }
    pub fn course_tag(&self, s: &Option<String>) -> &'static str {
        status_tag(s.as_deref().unwrap_or(""))
    }
    pub fn letter_of(&self, p: &Option<i64>) -> &'static str {
        letter(*p).unwrap_or("—")
    }
    pub fn first_name(&self) -> &str {
        self.nombre.split_whitespace().next().unwrap_or(&self.nombre)
    }
    pub fn enroll_link(&self, kind: &str) -> String {
        format!("/students/{}/enrollments/{kind}/new", self.uuid)
    }
    pub fn tabs(&self) -> Vec<(&'static str, &'static str, &'static str, usize)> {
        vec![
            ("certifications", "cert", "Certificaciones", self.cert_count),
            ("courses", "cur", "Cursos especializados", self.courses.len()),
            ("seminars", "sem", "Seminarios", self.seminars.len()),
            ("short-courses", "cc", "Cursos cortos", self.shorts.len()),
        ]
    }
    pub fn cdate(&self, d: &Option<String>) -> String {
        date_of(d)
    }
    pub fn mailto(&self) -> String {
        format!("mailto:{}", self.email)
    }
}

struct StudentRec {
    cols: Vec<Option<String>>,
}

fn student_rec(conn: &Connection, id: &str) -> Result<StudentRec, AppError> {
    let cols: Vec<&str> = STU_FIELDS.to_vec();
    conn.query_row(&format!("SELECT {} FROM students WHERE uuid = ?1", cols.join(", ")), [id], |r| {
        let mut v = Vec::with_capacity(cols.len());
        for i in 0..cols.len() {
            v.push(r.get::<_, Option<String>>(i)?);
        }
        Ok(StudentRec { cols: v })
    })
    .optional()?
    .ok_or(AppError::NotFound)
}

fn student_page_of(conn: &Connection, user: &CurrentUser, id: &str, tab: Tab) -> Result<StudentPage, AppError> {
    let rec = student_rec(conn, id)?;
    let col =
        |name: &str| rec.cols[STU_FIELDS.iter().position(|n| *n == name).unwrap_or(0)].clone().unwrap_or_default();
    let academic = students::academic(conn, id).map_err(|_| AppError::NotFound)?;
    let progress = students::certification_progress(conn, id)?;
    let graduated = progress.iter().filter(|p| p.is_graduated).count();
    let cert_count = progress.len();
    let certs = progress.into_iter().map(|p| cert_row(conn, id, p)).collect::<Result<Vec<_>, _>>()?;
    let courses = enrollments::course_lines(conn, id)?;
    let seminars = sem_lines(conn, Kind::Seminar, id)?;
    let shorts = sem_lines(conn, Kind::ShortCourse, id)?;
    let fields = STU_FIELDS.iter().filter(|n| **n != "member_uuid").map(|n| student_field(id, n, col(n))).collect();
    let mut shell = Shell::new(user, "escuela", "estudiantes");
    shell.autosave = true;
    Ok(StudentPage {
        shell,
        uuid: id.to_owned(),
        nombre: col("nombre"),
        compania: col("compania"),
        number: col("student_id"),
        email: col("email"),
        academic,
        tab,
        cert_count,
        graduated,
        certs,
        enrollments: students::enrollment_count(conn, id)?,
        courses,
        seminars,
        shorts,
        fields,
        affiliate: affiliate_field(conn, id, student_affiliate_choice(conn, id)?)?,
        interests: interests_of(conn, id, &user.csrf)?,
        map: map_link(students::maps_url(conn, id).map_err(|_| AppError::NotFound)?),
        new_student: NewStudentForm::default(),
        choices: affiliate_choices(conn, "")?,
        open: None,
    })
}

#[derive(Deserialize, Default)]
pub struct StudentQuery {
    tab: Option<String>,
}

async fn student_page(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<StudentQuery>,
) -> Result<Response, AppError> {
    let page = student_page_of(&state.db(), &staff.0, &id, Tab::from_query(q.tab.as_deref()))?;
    Ok(render_page(&headers, &page)?.into_response())
}

/// The title out of band after a name or company edit.
fn student_title_oob(conn: &Connection, id: &str) -> Result<String, AppError> {
    let (n, c, num): (String, Option<String>, Option<String>) =
        conn.query_row("SELECT nombre, compania, student_id FROM students WHERE uuid = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
    let mut sub = String::new();
    if let Some(c) = c.filter(|c| !c.trim().is_empty()) {
        sub.push_str(&format!(" · {}", esc(&c)));
    }
    if let Some(n) = num.filter(|n| !n.trim().is_empty()) {
        sub.push_str(&format!(" · #{}", esc(&n)));
    }
    Ok(autosave::oob("stu-title", &format!("<b>{}</b> <span class=\"sub\">{sub}</span>", esc(&n)))
        + &autosave::oob("stu-crumb", &esc(&n)))
}

/// The tags of the page bar (BR-STU-03 status, BR-AFF-01 affiliate price), out of band after an affiliate edit.
fn student_tags_oob(conn: &Connection, id: &str) -> Result<String, AppError> {
    let a = students::academic(conn, id).map_err(|_| AppError::NotFound)?;
    let mut html =
        format!("<span class=\"tag lg{}\" title=\"BR-STU-03\">{}</span>", status_tag(&a.status), esc(&a.status));
    if a.is_affiliate {
        html.push_str("<span class=\"tag lg info\" title=\"BR-AFF-01\">Afiliado</span>");
    }
    Ok(autosave::oob("stu-tags", &html))
}

async fn student_patch(
    State(state): State<AppState>,
    staff: RequireStaff,
    r: Option<Extension<RequestId>>,
    Path(id): Path<String>,
    field: OneField,
) -> Result<Response, AppError> {
    let who = Who::of(&staff.0);
    let mut conn = state.db();
    let Some(def) = reg::STUDENT.get(&field.name) else {
        return Ok(autosave::contract_error("Este campo no se puede cambiar aquí."));
    };
    if def.name == "member_uuid" {
        return affiliate_pick(&mut conn, &who, rid(&r), &id, &field.value);
    }
    let outcome = match reg::patch(&mut conn, &who, rid(&r), Resource::Student, &id, &field.name, &field.value)? {
        Patched::Unknown => return Ok(autosave::contract_error("Este campo no se puede cambiar aquí.")),
        Patched::Done(o) => o,
    };
    let (view, extra) = match outcome {
        Outcome::Saved { new, changed, .. } => {
            let mut extra = String::new();
            if changed && matches!(def.name, "nombre" | "compania" | "student_id") {
                extra.push_str(&student_title_oob(&conn, &id)?);
            }
            if changed
                && matches!(def.name, "dir_residencial" | "residencial_2" | "home_city" | "home_state" | "home_zip")
            {
                extra.push_str(&autosave::oob(
                    "stu-map",
                    &map_link(students::maps_url(&conn, &id).map_err(|_| AppError::NotFound)?),
                ));
            }
            (student_field(&id, def.name, def.display(&new)), extra)
        }
        Outcome::Invalid(m) => {
            let old = stored(&conn, "students", def.column, &id)?;
            (student_field(&id, def.name, def.display(&old)).refused(&field.value, m), String::new())
        }
    };
    Ok(autosave::respond(&view, &extra))
}

/// BR-STU-02: the typeahead value resolves to one affiliate key; the registry saves `member_uuid` and copies the
/// name into `compania` in the same transaction. The response is the affiliate field, the company field and the
/// tags out of band.
fn affiliate_pick(
    conn: &mut Connection,
    who: &Who,
    request_id: Option<&str>,
    id: &str,
    raw: &str,
) -> Result<Response, AppError> {
    students::exists(conn, id).map_err(|_| AppError::NotFound)?;
    let key = match resolve_affiliate(conn, raw)? {
        Ok(k) => k,
        Err(_) => {
            let saved = student_affiliate_choice(conn, id)?;
            let mut f = affiliate_field(conn, id, saved)?;
            f.f = f.f.refused(raw, "Escoja un afiliado de la lista.");
            return Ok((StatusCode::UNPROCESSABLE_ENTITY, Html(f.render()?)).into_response());
        }
    };
    let outcome =
        match reg::patch(conn, who, request_id, Resource::Student, id, "member_uuid", key.as_deref().unwrap_or(""))? {
            Patched::Unknown => return Ok(autosave::contract_error("Este campo no se puede cambiar aquí.")),
            Patched::Done(o) => o,
        };
    match outcome {
        Outcome::Saved { .. } => {
            let f = affiliate_field(conn, id, student_affiliate_choice(conn, id)?)?;
            let comp = text(conn.query_row("SELECT compania FROM students WHERE uuid = ?1", [id], |r| r.get(0))?);
            let extra = autosave::oob("stu-comp", &student_field(id, "compania", comp).html())
                + &student_title_oob(conn, id)?
                + &student_tags_oob(conn, id)?;
            Ok((StatusCode::OK, Html(f.render()? + &extra)).into_response())
        }
        Outcome::Invalid(m) => {
            let mut f = affiliate_field(conn, id, student_affiliate_choice(conn, id)?)?;
            f.f = f.f.refused(raw, m);
            Ok((StatusCode::UNPROCESSABLE_ENTITY, Html(f.render()?)).into_response())
        }
    }
}

/// OP-STU-04: admin only (403 for staff), 409 with the refusal dialog when the student has enrollments.
async fn student_delete(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let who = Who::of(&staff.0);
    let res = {
        let mut conn = state.db();
        let tx = school::begin(&mut conn)?;
        let r = students::delete_student(&tx, &who, &id);
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    match res {
        Ok(()) => Ok(go(&headers, "/students?hecho=borrado")),
        Err(DomainError::HasEnrollments(_)) | Err(DomainError::InUse(_)) => {
            let mut page = student_page_of(&state.db(), &staff.0, &id, Tab::Certs)?;
            page.open = Some("dlg-borrar-stu");
            Ok(render_page_status(StatusCode::CONFLICT, &headers, &page))
        }
        Err(e) => Ok(e.into_response()),
    }
}

/// OP-STU-02: a copy of the profile with its certification rows; the copy opens.
async fn student_duplicate(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let who = Who::of(&staff.0);
    let res = {
        let mut conn = state.db();
        let tx = school::begin(&mut conn)?;
        let r = students::duplicate_student(&tx, &who, &id);
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    match res {
        Ok(uuid) => Ok(go(&headers, &format!("/students/{uuid}?tab=details"))),
        Err(e) => Ok(e.into_response()),
    }
}

#[derive(Deserialize)]
struct InterestBody {
    #[serde(default)]
    certification_uuid: String,
}

/// The interests block after an add or remove (htmx), or the details tab (plain form).
fn interests_response(
    state: &AppState,
    headers: &HeaderMap,
    user: &CurrentUser,
    id: &str,
    error: Option<(StatusCode, String)>,
) -> Result<Response, AppError> {
    let conn = state.db();
    let mut block = interests_of(&conn, id, &user.csrf)?;
    if !is_htmx(headers) {
        return match error {
            None => Ok(Redirect::to(&format!("/students/{id}?tab=details")).into_response()),
            Some((status, m)) => {
                let mut page = student_page_of(&conn, user, id, Tab::Details)?;
                page.interests.error = Some(m);
                Ok(render_page_status(status, headers, &page))
            }
        };
    }
    let status = match error {
        None => StatusCode::OK,
        Some((s, m)) => {
            block.error = Some(m);
            s
        }
    };
    Ok((status, Html(block.render()?)).into_response())
}

/// OP-STU-03 add (BR-STU-05: at most 6, each with today's date).
async fn interest_add(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(f): Form<InterestBody>,
) -> Result<Response, AppError> {
    let who = Who::of(&staff.0);
    let res = {
        let mut conn = state.db();
        let tx = school::begin(&mut conn)?;
        let r = students::add_interest(&tx, &who, &id, &f.certification_uuid);
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    match res {
        Ok(_) => interests_response(&state, &headers, &staff.0, &id, None),
        Err(DomainError::NotFound) => Err(AppError::NotFound),
        Err(e @ (DomainError::Validation(..) | DomainError::LimitReached(_))) => {
            let status = if matches!(e, DomainError::LimitReached(_)) {
                StatusCode::CONFLICT
            } else {
                StatusCode::UNPROCESSABLE_ENTITY
            };
            interests_response(&state, &headers, &staff.0, &id, Some((status, e.message())))
        }
        Err(e) => Ok(e.into_response()),
    }
}

/// OP-STU-03 remove (AP-06).
async fn interest_remove(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path((id, interest)): Path<(String, String)>,
) -> Result<Response, AppError> {
    let who = Who::of(&staff.0);
    let res = {
        let mut conn = state.db();
        let tx = school::begin(&mut conn)?;
        let r = students::remove_interest(&tx, &who, &id, &interest);
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    match res {
        Ok(()) => interests_response(&state, &headers, &staff.0, &id, None),
        Err(e) => Ok(e.into_response()),
    }
}

/// `PATCH /student-certifications/{id}` (graduation date): the whole row comes back, so "Graduado" and
/// "Certificado" follow BR-CRT-04 at once; 422 keeps the typed date in the row.
async fn student_cert_patch(
    State(state): State<AppState>,
    staff: RequireStaff,
    r: Option<Extension<RequestId>>,
    Path(id): Path<String>,
    field: OneField,
) -> Result<Response, AppError> {
    let who = Who::of(&staff.0);
    let mut conn = state.db();
    if reg::STUDENT_CERT.get(&field.name).is_none() {
        return Ok(autosave::contract_error("Este campo no se puede cambiar aquí."));
    }
    let outcome = match reg::patch(&mut conn, &who, rid(&r), Resource::StudentCert, &id, &field.name, &field.value)? {
        Patched::Unknown => return Ok(autosave::contract_error("Este campo no se puede cambiar aquí.")),
        Patched::Done(o) => o,
    };
    let student: String =
        conn.query_row("SELECT student_uuid FROM student_certifications WHERE uuid = ?1", [&id], |r| r.get(0))?;
    let progress = students::certification_progress(&conn, &student)?;
    let Some(p) = progress.into_iter().find(|p| p.uuid == id) else { return Err(AppError::NotFound) };
    let mut row = cert_row(&conn, &student, p)?;
    let status = match outcome {
        Outcome::Saved { .. } => StatusCode::OK,
        Outcome::Invalid(m) => {
            row.date = row.date.refused(&field.value, m);
            StatusCode::UNPROCESSABLE_ENTITY
        }
    };
    Ok((status, Html(row.render()?)).into_response())
}

// ===============================================================================================================
// UI-16 Afiliados
// ===============================================================================================================

pub struct AffiliateRow {
    pub uuid: String,
    pub account: String,
    pub socio: String,
    pub negocio: String,
    pub miembro: String,
    pub city: String,
    pub phone: String,
    pub students: i64,
    pub cust_type: String,
}

impl AffiliateRow {
    pub fn tag(&self) -> &'static str {
        status_tag(&self.cust_type)
    }
}

/// "Regular" from `REGULAR` (value list `affiliates_membresia`).
fn membership_label(v: &str) -> String {
    let mut c = v.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
        None => String::new(),
    }
}

#[derive(Default)]
pub struct NewAffiliateForm {
    pub socio: String,
    pub error: Option<String>,
}

#[derive(Template)]
#[template(path = "school/students_affiliates/affiliates_list.html", blocks = ["content"])]
pub struct AffiliatesPage {
    pub shell: Shell,
    pub rows: Vec<AffiliateRow>,
    pub total_all: i64,
    pub active: i64,
    pub q: String,
    pub tipo: String,
    pub miembro: String,
    pub new_affiliate: NewAffiliateForm,
    pub notice: Option<String>,
    pub open: Option<&'static str>,
}

impl Page for AffiliatesPage {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

impl AffiliatesPage {
    pub fn types(&self) -> &'static [&'static str] {
        affiliates::CUST_TYPES
    }
    pub fn memberships(&self) -> Vec<(String, String)> {
        affiliates::MEMBERSHIPS.iter().map(|m| (m.to_string(), membership_label(m))).collect()
    }
}

#[derive(Deserialize, Default)]
pub struct AffiliatesQuery {
    q: Option<String>,
    tipo: Option<String>,
    miembro: Option<String>,
    hecho: Option<String>,
    n: Option<i64>,
}

fn affiliates_page(conn: &Connection, user: &CurrentUser, q: &AffiliatesQuery) -> Result<AffiliatesPage, AppError> {
    let term = q.q.clone().unwrap_or_default().trim().to_owned();
    let tipo = q.tipo.clone().unwrap_or_default();
    let miembro = q.miembro.clone().unwrap_or_default();
    let rows = conn
        .prepare(
            "SELECT a.uuid, a.account_num, a.socio, a.tipo_de_negocio, a.miembro, a.city, a.telephone,
                    (SELECT COUNT(*) FROM students s WHERE s.member_uuid = a.uuid), a.cust_type
               FROM affiliates a
              WHERE (?1 = '' OR a.socio LIKE '%' || ?1 || '%' OR a.other_name LIKE '%' || ?1 || '%'
                     OR a.city LIKE '%' || ?1 || '%' OR ('A-' || a.account_num) = ?1 OR CAST(a.account_num AS TEXT) = ?1)
                AND (?2 = '' OR a.cust_type = ?2) AND (?3 = '' OR a.miembro = ?3)
              ORDER BY a.socio COLLATE NOCASE",
        )?
        .query_map(params![term, tipo, miembro], |r| {
            Ok(AffiliateRow {
                uuid: r.get(0)?,
                account: affiliates::account_label(r.get(1)?),
                socio: r.get(2)?,
                negocio: text(r.get(3)?),
                miembro: membership_label(&text(r.get(4)?)),
                city: text(r.get(5)?),
                phone: text(r.get(6)?),
                students: r.get(7)?,
                cust_type: text(r.get(8)?),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let total_all: i64 = conn.query_row("SELECT COUNT(*) FROM affiliates", [], |r| r.get(0))?;
    let active: i64 = conn.query_row("SELECT COUNT(*) FROM affiliates WHERE cust_type = 'ACTIVO'", [], |r| r.get(0))?;
    Ok(AffiliatesPage {
        shell: Shell::new(user, "escuela", "afiliados"),
        rows,
        total_all,
        active,
        q: term,
        tipo,
        miembro,
        new_affiliate: NewAffiliateForm::default(),
        notice: match q.hecho.as_deref() {
            Some("borrado") => Some(match q.n.unwrap_or(0) {
                0 => "Afiliado borrado.".to_owned(),
                1 => "Afiliado borrado; 1 estudiante quedó sin afiliado.".to_owned(),
                n => format!("Afiliado borrado; {n} estudiantes quedaron sin afiliado."),
            }),
            _ => None,
        },
        open: None,
    })
}

async fn affiliates_list(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Query(q): Query<AffiliatesQuery>,
) -> Result<Response, AppError> {
    let page = affiliates_page(&state.db(), &staff.0, &q)?;
    Ok(render_page(&headers, &page)?.into_response())
}

async fn affiliate_new(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let mut page = affiliates_page(&state.db(), &staff.0, &AffiliatesQuery::default())?;
    page.open = Some("dlg-nuevo-afi");
    Ok(render_page(&headers, &page)?.into_response())
}

#[derive(Deserialize)]
struct NewAffiliateBody {
    #[serde(default)]
    socio: String,
}

/// OP-AFF-01 create (Guardar): the name; the next account number; the record opens for the other fields.
async fn affiliate_create(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Form(f): Form<NewAffiliateBody>,
) -> Result<Response, AppError> {
    let who = Who::of(&staff.0);
    let res = {
        let mut conn = state.db();
        let tx = school::begin(&mut conn)?;
        let r = affiliates::create_affiliate(&tx, &who, &f.socio);
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    match res {
        Ok(uuid) => Ok(go(&headers, &format!("/affiliates/{uuid}"))),
        Err(e @ DomainError::Validation(..)) => {
            let mut page = affiliates_page(&state.db(), &staff.0, &AffiliatesQuery::default())?;
            page.new_affiliate = NewAffiliateForm { socio: f.socio, error: Some(e.message()) };
            page.open = Some("dlg-nuevo-afi");
            Ok(render_page_status(StatusCode::UNPROCESSABLE_ENTITY, &headers, &page))
        }
        Err(e) => Ok(e.into_response()),
    }
}

// ===============================================================================================================
// UI-17 Afiliado
// ===============================================================================================================

const AFF_FIELDS: [&str; 15] = [
    "socio",
    "other_name",
    "tipo_de_negocio",
    "miembro",
    "cust_type",
    "e_mail",
    "customer_e_mail",
    "telephone",
    "telephone_1",
    "fax",
    "address",
    "city",
    "state_zip",
    "zip",
    "address1",
];

fn affiliate_record_field(id: &str, name: &str, saved: String) -> FieldView {
    let url = format!("/affiliates/{id}");
    let blank = || (String::new(), "—".to_owned());
    let (dom, aria, control): (&str, Option<&str>, Control) = match name {
        "socio" => ("af-socio", None, Control::Text),
        "other_name" => ("af-otro", None, Control::Text),
        "tipo_de_negocio" => ("af-neg", None, Control::Text),
        "miembro" => (
            "af-mem",
            None,
            Control::Select(
                std::iter::once(blank())
                    .chain(affiliates::MEMBERSHIPS.iter().map(|m| (m.to_string(), membership_label(m))))
                    .collect(),
            ),
        ),
        "cust_type" => (
            "af-tipo",
            None,
            Control::Select(affiliates::CUST_TYPES.iter().map(|t| (t.to_string(), t.to_string())).collect()),
        ),
        "e_mail" => ("af-mail", None, Control::Text),
        "customer_e_mail" => ("af-mailf", None, Control::Text),
        "telephone" => ("af-t1", None, Control::Text),
        "telephone_1" => ("af-t2", None, Control::Text),
        "fax" => ("af-fax", None, Control::Text),
        "address" => ("af-dir", None, Control::Text),
        "city" => ("af-pue", None, Control::Text),
        "state_zip" => ("af-estado", Some("Estado"), Control::Text),
        "zip" => ("af-zip", Some("Código Postal"), Control::Text),
        "address1" => ("af-fac", None, Control::Textarea(3)),
        _ => ("af-x", None, Control::Text),
    };
    let f = FieldView::new(url, name, dom, control, saved);
    let f = if name == "cust_type" { f.hint("ACTIVO: sus estudiantes pagan precio de socio (BR-AFF-01).") } else { f };
    match aria {
        Some(a) => f.aria(a),
        None => f,
    }
}

pub struct AffStudent {
    pub uuid: String,
    pub nombre: String,
    pub email: String,
    pub phone: String,
    pub status: String,
}

impl AffStudent {
    pub fn tag(&self) -> &'static str {
        status_tag(&self.status)
    }
}

#[derive(Template)]
#[template(path = "school/students_affiliates/affiliate.html", blocks = ["content"])]
pub struct AffiliatePage {
    pub shell: Shell,
    pub uuid: String,
    pub socio: String,
    pub account: String,
    pub negocio: String,
    pub cust_type: String,
    pub fields: Vec<FieldView>,
    pub students: Vec<AffStudent>,
    pub student_count: i64,
    pub map: String,
    pub open: Option<&'static str>,
}

impl Page for AffiliatePage {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

impl AffiliatePage {
    pub fn f(&self, name: &str) -> String {
        AFF_FIELDS
            .iter()
            .position(|n| *n == name)
            .and_then(|i| self.fields.get(i))
            .map(|f| f.html())
            .unwrap_or_default()
    }
    pub fn tag(&self) -> &'static str {
        status_tag(&self.cust_type)
    }
}

fn affiliate_map(conn: &Connection, id: &str) -> Result<String, AppError> {
    let (a, c, s, z): (Option<String>, Option<String>, Option<String>, Option<String>) = conn
        .query_row("SELECT address, city, state_zip, zip FROM affiliates WHERE uuid = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .optional()?
        .ok_or(AppError::NotFound)?;
    let tail = [s.as_deref(), z.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(" ");
    Ok(map_link(affiliates::maps_url(&[a.as_deref(), c.as_deref(), Some(tail.as_str())])))
}

fn affiliate_page_of(conn: &Connection, user: &CurrentUser, id: &str) -> Result<AffiliatePage, AppError> {
    let (account, cols): (Option<i64>, Vec<Option<String>>) = conn
        .query_row(
            &format!("SELECT account_num, {} FROM affiliates WHERE uuid = ?1", AFF_FIELDS.join(", ")),
            [id],
            |r| {
                let mut v = Vec::with_capacity(AFF_FIELDS.len());
                for i in 0..AFF_FIELDS.len() {
                    v.push(r.get::<_, Option<String>>(i + 1)?);
                }
                Ok((r.get(0)?, v))
            },
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    let col = |n: &str| cols[AFF_FIELDS.iter().position(|x| *x == n).unwrap_or(0)].clone().unwrap_or_default();
    let fields = AFF_FIELDS.iter().map(|n| affiliate_record_field(id, n, col(n))).collect();
    let students = conn
        .prepare(
            "SELECT s.uuid, s.nombre, COALESCE(s.email, s.email_work), COALESCE(s.telefono_r, s.phone_mobile), a.status
               FROM students s JOIN v_student_academic a ON a.student_uuid = s.uuid
              WHERE s.member_uuid = ?1 ORDER BY s.nombre COLLATE NOCASE LIMIT ?2",
        )?
        .query_map(params![id, AFF_STUDENTS], |r| {
            Ok(AffStudent {
                uuid: r.get(0)?,
                nombre: r.get(1)?,
                email: text(r.get(2)?),
                phone: text(r.get(3)?),
                status: r.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let student_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM students WHERE member_uuid = ?1", [id], |r| r.get(0))?;
    let mut shell = Shell::new(user, "escuela", "afiliados");
    shell.autosave = true;
    Ok(AffiliatePage {
        shell,
        uuid: id.to_owned(),
        socio: col("socio"),
        account: affiliates::account_label(account),
        negocio: col("tipo_de_negocio"),
        cust_type: col("cust_type"),
        fields,
        students,
        student_count,
        map: affiliate_map(conn, id)?,
        open: None,
    })
}

async fn affiliate_page(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let page = affiliate_page_of(&state.db(), &staff.0, &id)?;
    Ok(render_page(&headers, &page)?.into_response())
}

fn affiliate_head_oob(conn: &Connection, id: &str) -> Result<String, AppError> {
    let (s, acc, neg, t): (String, Option<i64>, Option<String>, Option<String>) = conn.query_row(
        "SELECT socio, account_num, tipo_de_negocio, cust_type FROM affiliates WHERE uuid = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let mut sub = format!(" · {}", affiliates::account_label(acc));
    if let Some(n) = neg.filter(|n| !n.trim().is_empty()) {
        sub.push_str(&format!(" · {}", esc(&n)));
    }
    let t = t.unwrap_or_default();
    Ok(autosave::oob("afi-title", &format!("<b>{}</b> <span class=\"sub\">{sub}</span>", esc(&s)))
        + &autosave::oob("afi-crumb", &esc(&s))
        + &autosave::oob("afi-tag", &format!("<span class=\"tag lg{}\">{}</span>", status_tag(&t), esc(&t))))
}

async fn affiliate_patch(
    State(state): State<AppState>,
    staff: RequireStaff,
    r: Option<Extension<RequestId>>,
    Path(id): Path<String>,
    field: OneField,
) -> Result<Response, AppError> {
    let who = Who::of(&staff.0);
    let mut conn = state.db();
    let Some(def) = reg::AFFILIATE.get(&field.name) else {
        return Ok(autosave::contract_error("Este campo no se puede cambiar aquí."));
    };
    let outcome = match reg::patch(&mut conn, &who, rid(&r), Resource::Affiliate, &id, &field.name, &field.value)? {
        Patched::Unknown => return Ok(autosave::contract_error("Este campo no se puede cambiar aquí.")),
        Patched::Done(o) => o,
    };
    let (view, extra) = match outcome {
        Outcome::Saved { new, changed, .. } => {
            let mut extra = String::new();
            if changed && matches!(def.name, "socio" | "tipo_de_negocio" | "cust_type") {
                extra.push_str(&affiliate_head_oob(&conn, &id)?);
            }
            if changed && matches!(def.name, "address" | "city" | "state_zip" | "zip") {
                extra.push_str(&autosave::oob("afi-map", &affiliate_map(&conn, &id)?));
            }
            (affiliate_record_field(&id, def.name, def.display(&new)), extra)
        }
        Outcome::Invalid(m) => {
            let old = stored(&conn, "affiliates", def.column, &id)?;
            (affiliate_record_field(&id, def.name, def.display(&old)).refused(&field.value, m), String::new())
        }
    };
    Ok(autosave::respond(&view, &extra))
}

/// OP-AFF-01 delete: admin only (403 for staff); the students are unlinked and keep their company (BR-AFF-02).
async fn affiliate_delete(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let who = Who::of(&staff.0);
    let res = {
        let mut conn = state.db();
        let tx = school::begin(&mut conn)?;
        let r = affiliates::delete_affiliate(&tx, &who, &id);
        if r.is_ok() {
            tx.commit()?;
        }
        r
    };
    match res {
        Ok(n) => Ok(go(&headers, &format!("/affiliates?hecho=borrado&n={n}"))),
        Err(e) => Ok(e.into_response()),
    }
}
