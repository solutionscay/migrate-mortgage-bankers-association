//! Phase-2 pane "grades_certs": UI-22 Importar notas and UI-32 Certificaciones y profesores.
//!
//! UI-22 (OP-ENR-07, AP-09 = both roles):
//! * `GET /grades/import`: step 1 (upload). With `?aplicado=N&sin_pareja=M` it shows step 3 (done).
//! * `POST /grades/import` (multipart `file`): `grades::read_xlsx` + `grades::preview`. Nothing is written. 200 with
//!   step 2 (old → new values, unmatched list, the token); 422 with step 1 and the message for a bad file.
//! * `POST /grades/import/{token}/apply`: `grades::apply` in one transaction, then 303 to step 3. Another user's,
//!   an expired or an unknown token answers 404 (T-05-12) with step 1 and a message.
//!
//! UI-32 (OP-CRT-01, OP-CRS-04, AP-08 = ROLE-ADMIN; inventory screen, D-19). The `/admin/*` paths are served here,
//! not in `crate::admin`; every handler takes `RequireAdmin` (staff: the 403 page).
//! * `GET /admin/certifications` (first certification selected), `GET /admin/certifications/{id}`,
//!   `GET /admin/professors` (same page).
//! * `POST /admin/certifications` (create dialog), `PATCH /admin/certifications/{id}` (autosave, one field),
//!   `DELETE /admin/certifications/{id}` (409 while students have progress on it).
//! * `POST /admin/certifications/{id}/requirements` (add a course), `PATCH .../requirements/{req_id}` (type),
//!   `DELETE .../requirements/{req_id}`. htmx gets the page content back.
//! * `POST /admin/professors` (create dialog), `GET /admin/professors/{id}/edit` (the row with its field),
//!   `PATCH /admin/professors/{id}` (autosave), `DELETE /admin/professors/{id}` (409 while a class date names them).

use crate::audit::RequestId;
use crate::auth::{CurrentUser, RequireAdmin, RequireStaff, Shell};
use crate::autosave::{self, Control, FieldView, OneField, Outcome};
use crate::error::AppError;
use crate::school::registries::{self, Patched, Resource};
use crate::school::{self, certifications, grades, DomainError, Who};
use crate::web::{is_htmx, render_page, render_page_status, Page};
use crate::AppState;
use askama::Template;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Extension, Form, Router};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use std::borrow::Borrow;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/grades/import", get(import_page).post(import_upload))
        .route("/grades/import/{token}/apply", post(import_apply))
        .route("/admin/certifications", get(crt_index).post(crt_create))
        .route("/admin/certifications/{id}", get(crt_show).patch(crt_patch).delete(crt_delete))
        .route("/admin/certifications/{id}/requirements", post(req_add))
        .route("/admin/certifications/{id}/requirements/{req_id}", axum::routing::patch(req_patch).delete(req_delete))
        .route("/admin/professors", get(prof_index).post(prof_create))
        .route("/admin/professors/{id}", axum::routing::patch(prof_patch).delete(prof_delete))
        .route("/admin/professors/{id}/edit", get(prof_edit))
}

fn rid(r: &Option<Extension<RequestId>>) -> Option<&str> {
    r.as_ref().map(|r| r.0 .0.as_str())
}

fn esc(s: &str) -> String {
    askama::filters::escape(s, askama::filters::Html).map(|s| s.to_string()).unwrap_or_default()
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

/// A grade as the design shows it: "94", "91.5"; empty for none.
fn show_grade(v: Option<f64>) -> String {
    match v {
        None => String::new(),
        Some(f) if f.fract() == 0.0 => format!("{}", f as i64),
        Some(f) => format!("{}", (f * 100.0).round() / 100.0),
    }
}

/// "9/14/2026" from `YYYY-MM-DD`.
fn show_date(iso: &str) -> String {
    crate::convention::registries::show_date(&Value::Text(iso.to_owned()))
}

// ===============================================================================================================
// UI-22 Importar notas
// ===============================================================================================================

/// One matched row of the preview table.
pub struct PreviewRow {
    pub serial: i64,
    pub student: String,
    /// (old, new) when the file changes the value; `None` = sin cambio.
    pub examen: Option<(String, String)>,
    pub final_avg: Option<(String, String)>,
    /// Other grade columns the file changes (count), shown under the name.
    pub others: usize,
    pub letter: String,
    pub passed: Option<bool>,
}

impl PreviewRow {
    pub fn other_text(&self) -> String {
        match self.others {
            0 => String::new(),
            1 => "+ 1 nota más".into(),
            n => format!("+ {n} notas más"),
        }
    }
}

pub struct UnmatchedRow {
    pub row: usize,
    pub serial: String,
    pub reason: &'static str,
}

impl UnmatchedRow {
    pub fn tag(&self) -> &'static str {
        match self.reason {
            "sin pareja" | "vacío" => "Sin pareja",
            "nota fuera de 0 a 100" => "Nota no válida",
            _ => "Serial no válido",
        }
    }
}

/// Step 2 data.
pub struct PreviewView {
    pub token: String,
    pub file_name: String,
    pub rows_read: usize,
    /// "Suscripción de Préstamos FHA · 9/14/2026", or "3 fechas de curso".
    pub scope: String,
    pub changes: usize,
    pub unchanged: usize,
    pub rows: Vec<PreviewRow>,
    pub unmatched: Vec<UnmatchedRow>,
}

#[derive(Template)]
#[template(path = "school/grades_certs/import.html", blocks = ["content"])]
pub struct ImportPage {
    pub shell: Shell,
    /// 1, 2 or 3.
    pub step: u8,
    pub preview: Option<PreviewView>,
    /// Step 1 message (bad file, expired token).
    pub error: Option<String>,
    /// Step 3: (updated, unmatched).
    pub applied: Option<(i64, i64)>,
}

impl ImportPage {
    pub fn step_class<N: Borrow<u8>>(&self, n: N) -> &'static str {
        let n = *n.borrow();
        if n < self.step || self.step == 3 {
            " class=\"done\""
        } else {
            ""
        }
    }
    pub fn step_current<N: Borrow<u8>>(&self, n: N) -> &'static str {
        if *n.borrow() == self.step && self.step != 3 {
            " aria-current=\"step\""
        } else {
            ""
        }
    }
    pub fn plural<N: Borrow<usize>>(&self, n: N, one: &'static str, many: &'static str) -> &'static str {
        if *n.borrow() == 1 {
            one
        } else {
            many
        }
    }
    /// Step 3 banner: (bold part, rest).
    pub fn applied_text(&self) -> (String, String) {
        let Some((u, m)) = self.applied else { return Default::default() };
        let s = |n: i64| if n == 1 { "" } else { "s" };
        let head = format!("{u} matrícula{} actualizada{}.", s(u), s(u));
        let rest = match m {
            0 => "Todas las filas tenían pareja.".to_owned(),
            1 => "1 fila sin pareja no cambió nada.".to_owned(),
            n => format!("{n} filas sin pareja no cambiaron nada."),
        };
        (head, rest)
    }
}

impl Page for ImportPage {
    fn render_fragment(&self) -> askama::Result<String> {
        // The title carries the step: refresh it with the content.
        let title = format!("<b>Importar notas</b> <span class=\"sub\">· paso {} de 3</span>", self.step);
        Ok(self.as_content().render()? + &autosave::oob("grd-title", &title))
    }
}

fn import_shell(user: &CurrentUser) -> Shell {
    Shell::new(user, "escuela", "importar-notas")
}

#[derive(Deserialize, Default)]
struct ImportQuery {
    aplicado: Option<i64>,
    sin_pareja: Option<i64>,
}

async fn import_page(
    staff: RequireStaff,
    headers: HeaderMap,
    Query(q): Query<ImportQuery>,
) -> Result<Response, AppError> {
    let page = match q.aplicado {
        Some(n) => ImportPage {
            shell: import_shell(&staff.0),
            step: 3,
            preview: None,
            error: None,
            applied: Some((n.max(0), q.sin_pareja.unwrap_or(0).max(0))),
        },
        None => ImportPage { shell: import_shell(&staff.0), step: 1, preview: None, error: None, applied: None },
    };
    Ok(render_page(&headers, &page)?.into_response())
}

/// Course and date of the matched serials: one calendar row, or the count.
fn scope_of(conn: &Connection, serials: &[i64]) -> Result<String, AppError> {
    if serials.is_empty() {
        return Ok(String::new());
    }
    let list = serials.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(",");
    let rows: Vec<(String, Option<String>)> = conn
        .prepare(&format!(
            "SELECT DISTINCT c.course,
                    (SELECT MIN(d.date) FROM calendar_course_dates d WHERE d.calendar_course_uuid = cc.uuid)
               FROM enrollments_course e
               JOIN calendar_course cc ON cc.uuid = e.course_calendar_uuid
               JOIN courses c ON c.uuid = cc.course_uuid
              WHERE e.serial IN ({list})"
        ))?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    Ok(match rows.as_slice() {
        [(c, Some(d))] => format!("{c} · {}", show_date(d)),
        [(c, None)] => c.clone(),
        many => format!("{} fechas de curso", many.len()),
    })
}

/// The stored score of a serial (unchanged rows show their current letter).
fn score_of(conn: &Connection, serial: i64) -> rusqlite::Result<Option<i64>> {
    Ok(conn
        .query_row("SELECT puntuacion FROM enrollments_course WHERE serial = ?1", [serial], |r| r.get(0))
        .optional()?
        .flatten())
}

fn preview_view(
    conn: &Connection,
    file_name: String,
    rows_read: usize,
    p: grades::Preview,
) -> Result<PreviewView, AppError> {
    let mut rows = Vec::new();
    for c in &p.changes {
        let pick = |col: &str| {
            c.fields.iter().find(|(k, _, _)| *k == col).map(|(_, old, new)| (show_grade(*old), show_grade(Some(*new))))
        };
        let score = c.new_score;
        rows.push(PreviewRow {
            serial: c.serial,
            student: c.student.clone(),
            examen: pick("examen"),
            final_avg: pick("promedio_final"),
            others: c.fields.iter().filter(|(k, _, _)| *k != "examen" && *k != "promedio_final").count(),
            letter: school::letter(score).unwrap_or("—").to_owned(),
            passed: score.map(|s| school::passed(Some(s))),
        });
    }
    for (serial, student) in &p.unchanged {
        let score = score_of(conn, *serial)?;
        rows.push(PreviewRow {
            serial: *serial,
            student: student.clone(),
            examen: None,
            final_avg: None,
            others: 0,
            letter: school::letter(score).unwrap_or("—").to_owned(),
            passed: score.map(|s| school::passed(Some(s))),
        });
    }
    rows.sort_by_key(|r| r.serial);
    let mut serials: Vec<i64> = rows.iter().map(|r| r.serial).collect();
    serials.dedup();
    Ok(PreviewView {
        token: p.token,
        file_name,
        rows_read,
        scope: scope_of(conn, &serials)?,
        changes: p.changes.len(),
        unchanged: p.unchanged.len(),
        rows,
        unmatched: p
            .skipped
            .into_iter()
            .map(|s| UnmatchedRow { row: s.row, serial: s.serial, reason: s.reason })
            .collect(),
    })
}

async fn import_upload(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    mut mp: Multipart,
) -> Result<Response, AppError> {
    let mut picked: Option<(String, Vec<u8>)> = None;
    let mut too_large = false;
    while let Ok(Some(mut field)) = mp.next_field().await {
        if field.name() != Some("file") || picked.is_some() {
            continue;
        }
        let name = field.file_name().unwrap_or_default().to_owned();
        let mut buf = Vec::new();
        while let Ok(Some(c)) = field.chunk().await {
            if buf.len() + c.len() > crate::sys::files::MAX_FILE_BYTES {
                too_large = true;
                break;
            }
            buf.extend_from_slice(&c);
        }
        picked = Some((name, buf));
    }
    let refuse = |msg: String| {
        let page =
            ImportPage { shell: import_shell(&staff.0), step: 1, preview: None, error: Some(msg), applied: None };
        Ok(render_page_status(StatusCode::UNPROCESSABLE_ENTITY, &headers, &page))
    };
    let (name, bytes) = match picked {
        _ if too_large => return refuse("El archivo pasa de 20 MB.".into()),
        Some((n, b)) if !n.is_empty() && !b.is_empty() => (n, b),
        _ => return refuse("Escoja el archivo XLSX exportado desde Matrículas.".into()),
    };
    let rows = match grades::read_xlsx(&bytes) {
        Ok(r) => r,
        Err(m) => return refuse(m),
    };
    let who = Who::of(&staff.0);
    let conn = state.db();
    let preview = grades::preview(&conn, &who, &rows).map_err(|e| match e {
        DomainError::Forbidden => AppError::Forbidden,
        e => AppError::Internal(e.message()),
    })?;
    let view = preview_view(&conn, name, rows.len(), preview)?;
    let page = ImportPage { shell: import_shell(&staff.0), step: 2, preview: Some(view), error: None, applied: None };
    Ok(render_page(&headers, &page)?.into_response())
}

async fn import_apply(
    State(state): State<AppState>,
    staff: RequireStaff,
    headers: HeaderMap,
    Path(token): Path<String>,
) -> Result<Response, AppError> {
    let who = Who::of(&staff.0);
    let res = {
        let mut conn = state.db();
        let tx = school::begin(&mut conn)?;
        match grades::apply(&tx, &who, &token) {
            Ok(a) => {
                tx.commit()?;
                Ok(a)
            }
            Err(e) => Err(e),
        }
    };
    match res {
        Ok(a) => Ok(go(&headers, &format!("/grades/import?aplicado={}&sin_pareja={}", a.updated, a.unmatched))),
        Err(DomainError::NotFound) => {
            let page = ImportPage {
                shell: import_shell(&staff.0),
                step: 1,
                preview: None,
                error: Some(
                    "La vista previa ya no existe: venció a los 30 minutos o es de otro usuario. Suba el archivo otra vez."
                        .into(),
                ),
                applied: None,
            };
            Ok(render_page_status(StatusCode::NOT_FOUND, &headers, &page))
        }
        Err(e) => Ok(e.into_response()),
    }
}

// ===============================================================================================================
// UI-32 Certificaciones y profesores
// ===============================================================================================================

pub struct CrtRow {
    pub uuid: String,
    pub name: String,
    pub requirements: i64,
}

pub struct ReqRow {
    pub uuid: String,
    pub course: String,
    pub kind: String,
}

impl ReqRow {
    pub fn tag(&self) -> &'static str {
        if self.kind == "Requisito" {
            "info"
        } else {
            "plain"
        }
    }
    pub fn other_kind(&self) -> &'static str {
        if self.kind == "Requisito" {
            "Electiva"
        } else {
            "Requisito"
        }
    }
}

pub struct ProfRow {
    pub uuid: String,
    pub name: String,
}

/// The selected certification (middle block).
pub struct CrtRecord {
    pub uuid: String,
    pub name: String,
    pub code: FieldView,
    pub description: FieldView,
    pub electives: FieldView,
    pub requirements: Vec<ReqRow>,
    /// Courses not yet on the certification (the add row).
    pub courses: Vec<(String, String)>,
}

/// Create dialogs: typed values and the message.
#[derive(Default)]
pub struct NewCrt {
    pub code: String,
    pub description: String,
    pub error: Option<(&'static str, String)>,
}

#[derive(Default)]
pub struct NewProf {
    pub name: String,
    pub error: Option<String>,
}

#[derive(Template)]
#[template(path = "school/grades_certs/admin.html", blocks = ["content"])]
pub struct AdminPage {
    pub shell: Shell,
    pub certs: Vec<CrtRow>,
    pub record: Option<CrtRecord>,
    pub profs: Vec<ProfRow>,
    pub new_crt: NewCrt,
    pub new_prof: NewProf,
    /// Dialog to open on load (create dialogs after a refusal, the 409 refusals).
    pub open: Option<&'static str>,
    /// Message of a refused delete (409).
    pub refusal: Option<String>,
    pub notice: Option<&'static str>,
}

impl Page for AdminPage {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

impl AdminPage {
    pub fn new_crt_error(&self, field: &str) -> Option<String> {
        self.new_crt.error.as_ref().filter(|(f, _)| *f == field).map(|(_, m)| m.clone())
    }
}

fn crt_url(id: &str) -> String {
    format!("/admin/certifications/{id}")
}

fn crt_field(id: &str, name: &str, saved: String) -> FieldView {
    match name {
        "certification" => FieldView::new(crt_url(id), name, "ct-code", Control::Text, saved),
        "description" => FieldView::new(crt_url(id), name, "ct-n", Control::Text, saved),
        _ => FieldView::new(crt_url(id), name, "ct-elec", Control::Number, saved),
    }
}

fn prof_field(id: &str, saved: String) -> FieldView {
    FieldView::new(format!("/admin/professors/{id}"), "name", &format!("pf-{id}"), Control::Text, saved)
        .aria("Nombre del profesor")
}

fn crt_name(code: &str, description: &Option<String>) -> String {
    match description.as_deref().map(str::trim) {
        Some(d) if !d.is_empty() => d.to_owned(),
        _ => code.to_owned(),
    }
}

fn cert_rows(conn: &Connection) -> rusqlite::Result<Vec<CrtRow>> {
    conn.prepare(
        "SELECT c.uuid, c.certification, c.description,
                (SELECT COUNT(*) FROM certification_requirements r WHERE r.certification_uuid = c.uuid)
           FROM certifications c ORDER BY COALESCE(NULLIF(c.description, ''), c.certification) COLLATE NOCASE",
    )?
    .query_map([], |r| {
        Ok(CrtRow { uuid: r.get(0)?, name: crt_name(&r.get::<_, String>(1)?, &r.get(2)?), requirements: r.get(3)? })
    })?
    .collect()
}

fn prof_rows(conn: &Connection) -> rusqlite::Result<Vec<ProfRow>> {
    conn.prepare("SELECT uuid, name FROM professors ORDER BY name COLLATE NOCASE")?
        .query_map([], |r| Ok(ProfRow { uuid: r.get(0)?, name: r.get(1)? }))?
        .collect()
}

fn crt_record(conn: &Connection, id: &str) -> Result<CrtRecord, AppError> {
    let (code, description, electives): (String, Option<String>, i64) = conn
        .query_row(
            "SELECT certification, description, number_of_required_electives FROM certifications WHERE uuid = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    let requirements = conn
        .prepare(
            "SELECT r.uuid, c.course, r.type FROM certification_requirements r JOIN courses c ON c.uuid = r.course_uuid
              WHERE r.certification_uuid = ?1 ORDER BY r.type = 'Electiva', c.course COLLATE NOCASE",
        )?
        .query_map([id], |r| Ok(ReqRow { uuid: r.get(0)?, course: r.get(1)?, kind: r.get(2)? }))?
        .collect::<Result<Vec<_>, _>>()?;
    let courses = conn
        .prepare(
            "SELECT uuid, course FROM courses WHERE uuid NOT IN
               (SELECT course_uuid FROM certification_requirements WHERE certification_uuid = ?1)
              ORDER BY course COLLATE NOCASE",
        )?
        .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(CrtRecord {
        uuid: id.to_owned(),
        name: crt_name(&code, &description),
        code: crt_field(id, "certification", code),
        description: crt_field(id, "description", description.unwrap_or_default()),
        electives: crt_field(id, "number_of_required_electives", electives.to_string()),
        requirements,
        courses,
    })
}

fn admin_page(conn: &Connection, user: &CurrentUser, selected: Option<&str>) -> Result<AdminPage, AppError> {
    let certs = cert_rows(conn)?;
    let pick = match selected {
        Some(id) => Some(id.to_owned()),
        None => certs.first().map(|c| c.uuid.clone()),
    };
    let record = match pick {
        Some(id) => Some(crt_record(conn, &id)?),
        None => None,
    };
    let mut shell = Shell::new(user, "admin", "admin-certificaciones");
    shell.autosave = true;
    Ok(AdminPage {
        shell,
        certs,
        record,
        profs: prof_rows(conn)?,
        new_crt: NewCrt::default(),
        new_prof: NewProf::default(),
        open: None,
        refusal: None,
        notice: None,
    })
}

#[derive(Deserialize, Default)]
struct AdminQuery {
    hecho: Option<String>,
}

fn notice_of(q: &AdminQuery) -> Option<&'static str> {
    match q.hecho.as_deref() {
        Some("borrada") => Some("Certificación borrada."),
        Some("profesor-borrado") => Some("Profesor borrado."),
        _ => None,
    }
}

async fn crt_index(
    State(state): State<AppState>,
    admin: RequireAdmin,
    headers: HeaderMap,
    Query(q): Query<AdminQuery>,
) -> Result<Response, AppError> {
    let mut page = admin_page(&state.db(), &admin.0, None)?;
    page.notice = notice_of(&q);
    Ok(render_page(&headers, &page)?.into_response())
}

async fn prof_index(
    State(state): State<AppState>,
    admin: RequireAdmin,
    headers: HeaderMap,
    Query(q): Query<AdminQuery>,
) -> Result<Response, AppError> {
    crt_index(State(state), admin, headers, Query(q)).await
}

async fn crt_show(
    State(state): State<AppState>,
    admin: RequireAdmin,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let page = admin_page(&state.db(), &admin.0, Some(&id))?;
    Ok(render_page(&headers, &page)?.into_response())
}

#[derive(Deserialize)]
struct NewCrtBody {
    #[serde(default)]
    certification: String,
    #[serde(default)]
    description: String,
}

async fn crt_create(
    State(state): State<AppState>,
    admin: RequireAdmin,
    headers: HeaderMap,
    Form(f): Form<NewCrtBody>,
) -> Result<Response, AppError> {
    let who = Who::of(&admin.0);
    let res = {
        let mut conn = state.db();
        let tx = school::begin(&mut conn)?;
        match certifications::create_certification(&tx, &who, &f.certification, &f.description) {
            Ok(id) => {
                tx.commit()?;
                Ok(id)
            }
            Err(e) => Err(e),
        }
    };
    match res {
        Ok(id) => Ok(go(&headers, &crt_url(&id))),
        Err(DomainError::Validation(field, m)) => {
            let mut page = admin_page(&state.db(), &admin.0, None)?;
            page.new_crt = NewCrt {
                code: f.certification,
                description: f.description,
                error: Some((if field == "description" { "description" } else { "certification" }, m)),
            };
            page.open = Some("dlg-nueva-crt");
            Ok(render_page_status(StatusCode::UNPROCESSABLE_ENTITY, &headers, &page))
        }
        Err(e) => Ok(e.into_response()),
    }
}

/// The list name and the record title after a name edit (out of band).
fn crt_name_oob(conn: &Connection, id: &str) -> Result<String, AppError> {
    let (code, description): (String, Option<String>) =
        conn.query_row("SELECT certification, description FROM certifications WHERE uuid = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
    let name = esc(&crt_name(&code, &description));
    Ok(autosave::oob(&format!("crt-name-{id}"), &name) + &autosave::oob("crt-title", &name))
}

async fn crt_patch(
    State(state): State<AppState>,
    admin: RequireAdmin,
    r: Option<Extension<RequestId>>,
    Path(id): Path<String>,
    field: OneField,
) -> Result<Response, AppError> {
    let who = Who::of(&admin.0);
    let mut conn = state.db();
    let outcome =
        match registries::patch(&mut conn, &who, rid(&r), Resource::Certification, &id, &field.name, &field.value)? {
            Patched::Unknown => return Ok(autosave::contract_error("Este campo no se puede cambiar aquí.")),
            Patched::Done(o) => o,
        };
    let def = registries::CERTIFICATION.get(&field.name).ok_or(AppError::NotFound)?;
    let (view, extra) = match outcome {
        Outcome::Saved { new, changed, .. } => {
            let extra = if changed && def.name != "number_of_required_electives" {
                crt_name_oob(&conn, &id)?
            } else {
                String::new()
            };
            (crt_field(&id, def.name, def.display(&new)), extra)
        }
        Outcome::Invalid(m) => {
            let old: Value =
                conn.query_row(&format!("SELECT {} FROM certifications WHERE uuid = ?1", def.column), [&id], |r| {
                    r.get(0)
                })?;
            (crt_field(&id, def.name, def.display(&old)).refused(&field.value, m), String::new())
        }
    };
    Ok(autosave::respond(&view, &extra))
}

/// After a domain refusal on the admin page: the page with the message dialog, the domain status.
fn admin_refusal(
    conn: &Connection,
    user: &CurrentUser,
    headers: &HeaderMap,
    selected: Option<&str>,
    e: DomainError,
) -> Result<Response, AppError> {
    let status = e.status();
    if matches!(e, DomainError::Forbidden | DomainError::NotFound) {
        return Ok(e.into_response());
    }
    let mut page = admin_page(conn, user, selected)?;
    page.refusal = Some(e.message());
    page.open = Some("dlg-refusal");
    Ok(render_page_status(status, headers, &page))
}

async fn crt_delete(
    State(state): State<AppState>,
    admin: RequireAdmin,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let who = Who::of(&admin.0);
    let res = {
        let mut conn = state.db();
        let tx = school::begin(&mut conn)?;
        match certifications::delete_certification(&tx, &who, &id) {
            Ok(()) => {
                tx.commit()?;
                Ok(())
            }
            Err(e) => Err(e),
        }
    };
    match res {
        Ok(()) => Ok(go(&headers, "/admin/certifications?hecho=borrada")),
        Err(DomainError::InUse(n)) => {
            let conn = state.db();
            let mut page = admin_page(&conn, &admin.0, Some(&id))?;
            page.refusal = Some(format!(
                "No se puede borrar: {n} estudiante{} tiene{} progreso o graduación en esta certificación.",
                if n == 1 { "" } else { "s" },
                if n == 1 { "" } else { "n" }
            ));
            page.open = Some("dlg-refusal");
            Ok(render_page_status(StatusCode::CONFLICT, &headers, &page))
        }
        Err(e) => admin_refusal(&state.db(), &admin.0, &headers, Some(&id), e),
    }
}

#[derive(Deserialize)]
struct ReqBody {
    #[serde(default)]
    course_uuid: String,
    #[serde(default, rename = "type")]
    kind: String,
}

/// The page content (htmx) or a 303 back to the certification (plain form).
fn crt_done(conn: &Connection, user: &CurrentUser, headers: &HeaderMap, id: &str) -> Result<Response, AppError> {
    if is_htmx(headers) {
        let page = admin_page(conn, user, Some(id))?;
        Ok(render_page(headers, &page)?.into_response())
    } else {
        Ok(Redirect::to(&crt_url(id)).into_response())
    }
}

async fn req_add(
    State(state): State<AppState>,
    admin: RequireAdmin,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(f): Form<ReqBody>,
) -> Result<Response, AppError> {
    let who = Who::of(&admin.0);
    let res = {
        let mut conn = state.db();
        let tx = school::begin(&mut conn)?;
        match certifications::add_requirement(&tx, &who, &id, f.course_uuid.trim(), f.kind.trim()) {
            Ok(_) => {
                tx.commit()?;
                Ok(())
            }
            Err(e) => Err(e),
        }
    };
    let conn = state.db();
    match res {
        Ok(()) => crt_done(&conn, &admin.0, &headers, &id),
        Err(e @ DomainError::Validation(..)) => {
            let mut page = admin_page(&conn, &admin.0, Some(&id))?;
            page.refusal = Some(e.message());
            page.open = Some("dlg-refusal");
            Ok(render_page_status(StatusCode::UNPROCESSABLE_ENTITY, &headers, &page))
        }
        Err(e) => admin_refusal(&conn, &admin.0, &headers, Some(&id), e),
    }
}

/// The requirement must belong to the certification of the path.
fn req_of(conn: &Connection, id: &str, req_id: &str) -> Result<(), AppError> {
    conn.query_row(
        "SELECT 1 FROM certification_requirements WHERE uuid = ?1 AND certification_uuid = ?2",
        [req_id, id],
        |_| Ok(()),
    )
    .optional()?
    .ok_or(AppError::NotFound)
}

async fn req_patch(
    State(state): State<AppState>,
    admin: RequireAdmin,
    headers: HeaderMap,
    r: Option<Extension<RequestId>>,
    Path((id, req_id)): Path<(String, String)>,
    field: OneField,
) -> Result<Response, AppError> {
    let who = Who::of(&admin.0);
    let mut conn = state.db();
    req_of(&conn, &id, &req_id)?;
    match registries::patch(&mut conn, &who, rid(&r), Resource::Requirement, &req_id, &field.name, &field.value)? {
        Patched::Unknown => Ok(autosave::contract_error("Este campo no se puede cambiar aquí.")),
        Patched::Done(Outcome::Invalid(m)) => {
            let mut page = admin_page(&conn, &admin.0, Some(&id))?;
            page.refusal = Some(m);
            page.open = Some("dlg-refusal");
            Ok(render_page_status(StatusCode::UNPROCESSABLE_ENTITY, &headers, &page))
        }
        Patched::Done(Outcome::Saved { .. }) => crt_done(&conn, &admin.0, &headers, &id),
    }
}

async fn req_delete(
    State(state): State<AppState>,
    admin: RequireAdmin,
    headers: HeaderMap,
    Path((id, req_id)): Path<(String, String)>,
) -> Result<Response, AppError> {
    let who = Who::of(&admin.0);
    let res = {
        let mut conn = state.db();
        req_of(&conn, &id, &req_id)?;
        let tx = school::begin(&mut conn)?;
        match certifications::remove_requirement(&tx, &who, &req_id) {
            Ok(()) => {
                tx.commit()?;
                Ok(())
            }
            Err(e) => Err(e),
        }
    };
    let conn = state.db();
    match res {
        Ok(()) => crt_done(&conn, &admin.0, &headers, &id),
        Err(e) => admin_refusal(&conn, &admin.0, &headers, Some(&id), e),
    }
}

#[derive(Deserialize)]
struct NewProfBody {
    #[serde(default)]
    name: String,
}

async fn prof_create(
    State(state): State<AppState>,
    admin: RequireAdmin,
    headers: HeaderMap,
    Form(f): Form<NewProfBody>,
) -> Result<Response, AppError> {
    let who = Who::of(&admin.0);
    let res = {
        let mut conn = state.db();
        let tx = school::begin(&mut conn)?;
        match certifications::create_professor(&tx, &who, &f.name) {
            Ok(_) => {
                tx.commit()?;
                Ok(())
            }
            Err(e) => Err(e),
        }
    };
    match res {
        Ok(()) => Ok(go(&headers, "/admin/professors")),
        Err(DomainError::Validation(_, m)) => {
            let mut page = admin_page(&state.db(), &admin.0, None)?;
            page.new_prof = NewProf { name: f.name, error: Some(m) };
            page.open = Some("dlg-nuevo-prof");
            Ok(render_page_status(StatusCode::UNPROCESSABLE_ENTITY, &headers, &page))
        }
        Err(e) => Ok(e.into_response()),
    }
}

/// `GET /admin/professors/{id}/edit`: the row with the autosave field ("Editar" in the row menu).
async fn prof_edit(
    State(state): State<AppState>,
    _admin: RequireAdmin,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let name: String = state
        .db()
        .query_row("SELECT name FROM professors WHERE uuid = ?1", [&id], |r| r.get(0))
        .optional()?
        .ok_or(AppError::NotFound)?;
    let field = prof_field(&id, name);
    Ok(Html(format!("<tr id=\"prof-{id}\"><td colspan=\"2\">{}</td></tr>", field.html())).into_response())
}

async fn prof_patch(
    State(state): State<AppState>,
    admin: RequireAdmin,
    r: Option<Extension<RequestId>>,
    Path(id): Path<String>,
    field: OneField,
) -> Result<Response, AppError> {
    let who = Who::of(&admin.0);
    let mut conn = state.db();
    let outcome =
        match registries::patch(&mut conn, &who, rid(&r), Resource::Professor, &id, &field.name, &field.value)? {
            Patched::Unknown => return Ok(autosave::contract_error("Este campo no se puede cambiar aquí.")),
            Patched::Done(o) => o,
        };
    let view = match outcome {
        Outcome::Saved { new, .. } => prof_field(&id, autosave::text_of(&new)),
        Outcome::Invalid(m) => {
            let old: String = conn.query_row("SELECT name FROM professors WHERE uuid = ?1", [&id], |r| r.get(0))?;
            prof_field(&id, old).refused(&field.value, m)
        }
    };
    Ok(autosave::respond(&view, ""))
}

async fn prof_delete(
    State(state): State<AppState>,
    admin: RequireAdmin,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let who = Who::of(&admin.0);
    let res = {
        let mut conn = state.db();
        let tx = school::begin(&mut conn)?;
        match certifications::delete_professor(&tx, &who, &id) {
            Ok(()) => {
                tx.commit()?;
                Ok(())
            }
            Err(e) => Err(e),
        }
    };
    match res {
        Ok(()) => Ok(go(&headers, "/admin/professors?hecho=profesor-borrado")),
        Err(DomainError::InUse(n)) => {
            let conn = state.db();
            let mut page = admin_page(&conn, &admin.0, None)?;
            page.refusal = Some(format!(
                "No se puede borrar: el profesor está en {n} fecha{} de curso o seminario.",
                if n == 1 { "" } else { "s" }
            ));
            page.open = Some("dlg-refusal");
            Ok(render_page_status(StatusCode::CONFLICT, &headers, &page))
        }
        Err(e) => admin_refusal(&state.db(), &admin.0, &headers, None, e),
    }
}
