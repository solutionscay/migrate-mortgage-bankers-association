//! UI-30 Plantillas de documentos: OP-SYS-02 document templates (D-18).
//!
//! * `GET /templates/{code}`: the template builder. The rich-text editor of `app.js` (`[data-rte]`) writes the body
//!   into a hidden autosave field. Merge fields show as chips and are stored as `{{token}}` in `body_html`.
//! * `PATCH /templates/{code}`: title or body, one field per request (05 "Per-field autosave"). The body is
//!   sanitized with `ammonia`: only the tags the editor toolbar makes ([`BODY_TAGS`]) and no attributes.
//! * `POST /templates/{code}/preview`: the letter with merged data of one record of the module. It saves nothing.
//!
//! [`fields`] lists the merge fields of a module; [`Kind`] holds the SQL that fills them for one record.

use crate::audit::RequestId;
use crate::auth::{CurrentUser, Shell};
use crate::autosave::{self, Control, Ctx, FieldDef, FieldView, OneField, Outcome, Registry, Stamp};
use crate::error::AppError;
use crate::web::{render_page, Page};
use crate::AppState;
use askama::Template;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::{Html, Response};
use axum::routing::{get, post};
use axum::{Extension, Form, Router};
use rusqlite::types::Value;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/templates/{code}", get(template_page).patch(patch_template))
        .route("/templates/{code}/preview", post(preview))
}

// ---------------------------------------------------------------------------------------------------------------
// Sanitizing
// ---------------------------------------------------------------------------------------------------------------

/// The tags the editor toolbar makes: paragraph, line break, bold, italic, heading, bullet list.
pub const BODY_TAGS: &[&str] = &["p", "br", "b", "i", "h2", "ul", "li"];

/// Sanitized body: [`BODY_TAGS`] only, no attributes; script and style content is dropped. `{{token}}` text stays.
pub fn sanitize(html: &str) -> String {
    let mut b = ammonia::Builder::default();
    b.tags(BODY_TAGS.iter().copied().collect())
        .generic_attributes(HashSet::new())
        .tag_attributes(HashMap::new())
        .link_rel(None);
    b.clean(&html.replace('\u{200b}', "")).to_string()
}

/// True when the HTML has text (or a merge field) outside its tags.
fn has_text(html: &str) -> bool {
    let mut inside = false;
    let text: String = html
        .chars()
        .filter(|c| match c {
            '<' => {
                inside = true;
                false
            }
            '>' => {
                inside = false;
                false
            }
            _ => !inside,
        })
        .collect();
    !text.replace("&nbsp;", " ").trim().is_empty()
}

fn parse_body(s: &str) -> Result<Value, String> {
    let clean = sanitize(s.trim());
    if !has_text(&clean) {
        Err("Escriba el texto del documento.".into())
    } else if clean.len() > 100_000 {
        Err("El texto es demasiado largo.".into())
    } else {
        Ok(Value::Text(clean))
    }
}

/// `PATCH /admin/templates/{code}`: title and body (D-18).
pub static TEMPLATES: Registry = Registry {
    table: "document_templates",
    key_col: "code",
    stamp: Stamp::Full,
    fields: &[
        FieldDef::new("title", "title", "Título", |s| autosave::required_text(s, 200)),
        FieldDef::new("body_html", "body_html", "Texto", parse_body),
    ],
};

// ---------------------------------------------------------------------------------------------------------------
// Merge fields
// ---------------------------------------------------------------------------------------------------------------

/// How a merge field value is shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fmt {
    Text,
    /// "14 de septiembre de 2026".
    Date,
    /// "$1,218.00" from integer cents.
    Money,
    /// The logo image of `app_settings.logo_file_uuid`.
    Logo,
}

/// One merge field a template may use: `{{token}}` in `body_html`, `label` on the chip and in the menu.
pub struct MergeField {
    pub token: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub fmt: Fmt,
}

const fn mf(token: &'static str, label: &'static str, group: &'static str, fmt: Fmt) -> MergeField {
    MergeField { token, label, group, fmt }
}

/// Organization settings (`app_settings`), in every module. The signer is the one of the module (BR-SYS-03).
pub const ORG_FIELDS: &[MergeField] = &[
    mf("organizacion", "Nombre de la organización", "Organización", Fmt::Text),
    mf("logo", "Logo de la organización", "Organización", Fmt::Logo),
    mf("firmante", "Firmante", "Organización", Fmt::Text),
    mf("titulo_firmante", "Título del firmante", "Organización", Fmt::Text),
    mf("fecha_hoy", "Fecha de hoy", "Organización", Fmt::Date),
];

/// School letters and certificates: one course enrollment (student, course, calendar, certification).
pub const SCHOOL_FIELDS: &[MergeField] = &[
    mf("nombre", "Nombre del estudiante", "Estudiante", Fmt::Text),
    mf("numero_estudiante", "Número de estudiante", "Estudiante", Fmt::Text),
    mf("nmls", "Número NMLS", "Estudiante", Fmt::Text),
    mf("correo", "Correo del estudiante", "Estudiante", Fmt::Text),
    mf("compania", "Compañía", "Estudiante", Fmt::Text),
    mf("curso", "Curso", "Curso", Fmt::Text),
    mf("fecha", "Fecha del curso", "Curso", Fmt::Date),
    mf("horas", "Horas del curso", "Curso", Fmt::Text),
    mf("creditos", "Créditos", "Curso", Fmt::Text),
    mf("semestre", "Semestre", "Curso", Fmt::Text),
    mf("costo", "Costo de la matrícula", "Curso", Fmt::Money),
    mf("pagado", "Pagado", "Curso", Fmt::Money),
    mf("balance", "Balance de la matrícula", "Curso", Fmt::Money),
    mf("nota", "Nota final", "Curso", Fmt::Text),
    mf("certificacion", "Certificación", "Certificación", Fmt::Text),
    mf("fecha_graduacion", "Fecha de graduación", "Certificación", Fmt::Date),
];

/// Convention letters: one registration (person, event, totals) or one sponsor (sponsor, event).
pub const CONVENTION_FIELDS: &[MergeField] = &[
    mf("nombre", "Nombre de la persona", "Persona y registro", Fmt::Text),
    mf("compania", "Compañía", "Persona y registro", Fmt::Text),
    mf("correo", "Correo", "Persona y registro", Fmt::Text),
    mf("registro", "Número de registro", "Persona y registro", Fmt::Text),
    mf("tipo_registro", "Tipo de registro", "Persona y registro", Fmt::Text),
    mf("total", "Total del registro", "Persona y registro", Fmt::Money),
    mf("pagos", "Pagos recibidos", "Persona y registro", Fmt::Money),
    mf("balance", "Balance pendiente", "Persona y registro", Fmt::Money),
    mf("llegada", "Llegada al hotel", "Persona y registro", Fmt::Date),
    mf("salida", "Salida del hotel", "Persona y registro", Fmt::Date),
    mf("evento", "Evento", "Evento", Fmt::Text),
    mf("fecha", "Fecha del evento", "Evento", Fmt::Date),
    mf("fecha_fin", "Último día del evento", "Evento", Fmt::Date),
    mf("lugar", "Lugar del evento", "Evento", Fmt::Text),
    mf("auspiciador", "Auspiciador", "Auspiciador", Fmt::Text),
    mf("saludo", "Saludo de la carta", "Auspiciador", Fmt::Text),
    mf("categoria", "Categoría de auspicio", "Auspiciador", Fmt::Text),
    mf("monto_solicitado", "Monto solicitado", "Auspiciador", Fmt::Money),
    mf("monto_recibido", "Monto recibido", "Auspiciador", Fmt::Money),
    mf("monto_prometido", "Monto prometido", "Auspiciador", Fmt::Money),
];

/// Mail templates: one student (the recipients of MBS_SendMail batches).
pub const MAIL_FIELDS: &[MergeField] = &[
    mf("nombre", "Nombre del destinatario", "Destinatario", Fmt::Text),
    mf("numero_estudiante", "Número de estudiante", "Destinatario", Fmt::Text),
    mf("correo", "Correo", "Destinatario", Fmt::Text),
    mf("compania", "Compañía", "Destinatario", Fmt::Text),
];

/// The merge fields of a module: its record fields, then the organization fields.
pub fn fields(module: &str) -> Vec<&'static MergeField> {
    let own = match module {
        "school" => SCHOOL_FIELDS,
        "convention" => CONVENTION_FIELDS,
        _ => MAIL_FIELDS,
    };
    own.iter().chain(ORG_FIELDS).collect()
}

/// The tokens of a body, in order (`{{ token }}`, spaces allowed).
pub fn tokens(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(i) = rest.find("{{") {
        let after = &rest[i + 2..];
        let Some(j) = after.find("}}") else { break };
        let t = after[..j].trim();
        if !t.is_empty() && t.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
            out.push(t.to_owned());
        }
        rest = &after[j + 2..];
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------
// Preview records
// ---------------------------------------------------------------------------------------------------------------

/// A record kind that fills merge fields. The id on the page is `prefix:uuid`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Enrollment,
    Registration,
    Sponsor,
    Student,
}

impl Kind {
    fn of_module(module: &str) -> &'static [Kind] {
        match module {
            "school" => &[Kind::Enrollment],
            "convention" => &[Kind::Registration, Kind::Sponsor],
            _ => &[Kind::Student],
        }
    }
    fn prefix(self) -> &'static str {
        match self {
            Kind::Enrollment => "enr",
            Kind::Registration => "reg",
            Kind::Sponsor => "spo",
            Kind::Student => "stu",
        }
    }
    fn group(self) -> &'static str {
        match self {
            Kind::Enrollment => "Matrículas",
            Kind::Registration => "Registros",
            Kind::Sponsor => "Auspiciadores",
            Kind::Student => "Estudiantes",
        }
    }
    /// `(id, label)` of the records, in the order of the select.
    fn list_sql(self) -> &'static str {
        match self {
            Kind::Enrollment => {
                "SELECT l.uuid, s.nombre || ' · ' || l.course
                        || COALESCE(' · ' || strftime('%m/%d/%Y', l.course_date), '')
                   FROM v_enrollments_course_lines l JOIN students s ON s.uuid = l.student_uuid
                  ORDER BY s.nombre, l.course_date DESC LIMIT 500"
            }
            Kind::Registration => {
                "SELECT r.uuid, '#' || r.registration_number || ' · '
                        || TRIM(COALESCE(p.name_first, '') || ' ' || COALESCE(p.name_last, ''))
                        || COALESCE(' · ' || e.name, '')
                   FROM registration r JOIN people p ON p.uuid = r.person_uuid
                   LEFT JOIN events e ON e.uuid = r.event_uuid
                  ORDER BY e.date_start DESC, r.registration_number LIMIT 500"
            }
            Kind::Sponsor => {
                "SELECT s.uuid, s.sponsor_name || ' · ' || e.name
                   FROM sponsor s JOIN events e ON e.uuid = s.event_uuid
                  ORDER BY e.date_start DESC, s.sponsor_name LIMIT 500"
            }
            Kind::Student => "SELECT uuid, nombre FROM students ORDER BY nombre LIMIT 500",
        }
    }
    /// The default record: the first one that fills the most fields of a letter.
    fn default_sql(self) -> &'static str {
        match self {
            // The first student on the roll, in a confirmed course when there is one.
            Kind::Enrollment => {
                "SELECT l.uuid FROM v_enrollments_course_lines l JOIN students s ON s.uuid = l.student_uuid
                  ORDER BY CAST(s.student_id AS INTEGER) IS NULL, CAST(s.student_id AS INTEGER),
                           l.course_status = 'Confirmado' DESC, l.course_date DESC LIMIT 1"
            }
            // A registration with payments, a balance and a hotel stay fills every field; latest event first.
            Kind::Registration => {
                "SELECT r.uuid FROM registration r JOIN v_registration_totals t ON t.registration_uuid = r.uuid
                   LEFT JOIN events e ON e.uuid = r.event_uuid
                  ORDER BY (t.payment_total > 0 AND t.balance > 0) DESC, t.has_hotel DESC, e.date_start DESC,
                           r.registration_number
                  LIMIT 1"
            }
            Kind::Sponsor => {
                "SELECT s.uuid FROM sponsor s JOIN events e ON e.uuid = s.event_uuid
                  ORDER BY e.date_start DESC, s.sponsor_name LIMIT 1"
            }
            Kind::Student => {
                "SELECT uuid FROM students ORDER BY CAST(student_id AS INTEGER) IS NULL, CAST(student_id AS INTEGER)
                  LIMIT 1"
            }
        }
    }
    /// One row whose column names are the tokens it fills (`?1` = uuid). Documented in 05 "Merge fields".
    fn values_sql(self) -> &'static str {
        match self {
            Kind::Enrollment => {
                "SELECT s.nombre AS nombre, s.student_id AS numero_estudiante, s.nmls AS nmls,
                        COALESCE(s.email, s.email_work) AS correo, s.compania AS compania,
                        l.course AS curso, l.course_date AS fecha, l.hours AS horas, l.credits AS creditos,
                        cc.semester AS semestre, l.costo AS costo, l.pago AS pagado, l.balance AS balance,
                        l.nota AS nota,
                        (SELECT c.description FROM student_certifications sc
                           JOIN certifications c ON c.uuid = sc.certification_uuid
                          WHERE sc.student_uuid = s.uuid
                          ORDER BY sc.graduation_date IS NULL, sc.graduation_date DESC, sc.date DESC LIMIT 1)
                          AS certificacion,
                        (SELECT MAX(sc.graduation_date) FROM student_certifications sc
                          WHERE sc.student_uuid = s.uuid) AS fecha_graduacion
                   FROM v_enrollments_course_lines l JOIN students s ON s.uuid = l.student_uuid
                   LEFT JOIN calendar_course cc ON cc.uuid = l.course_calendar_uuid
                  WHERE l.uuid = ?1"
            }
            Kind::Registration => {
                "SELECT TRIM(COALESCE(p.name_first, '') || ' ' || COALESCE(p.name_last, '')) AS nombre,
                        p.company AS compania, p.email AS correo, r.registration_number AS registro,
                        t.registration_type AS tipo_registro, t.total AS total, t.payment_total AS pagos,
                        t.balance AS balance, t.arrival_date AS llegada, t.departure_date AS salida,
                        e.name AS evento, e.date_start AS fecha, COALESCE(e.date_end, e.date_start) AS fecha_fin,
                        e.location AS lugar
                   FROM registration r JOIN people p ON p.uuid = r.person_uuid
                   JOIN v_registration_totals t ON t.registration_uuid = r.uuid
                   LEFT JOIN events e ON e.uuid = r.event_uuid
                  WHERE r.uuid = ?1"
            }
            Kind::Sponsor => {
                "SELECT TRIM(COALESCE(s.title, '') || ' ' || COALESCE(s.contact_name, '')) AS nombre, s.greeting AS saludo,
                        s.sponsor_name AS compania, s.sponsor_name AS auspiciador, s.type AS categoria,
                        s.requested_amount AS monto_solicitado, s.amount_received AS monto_recibido,
                        s.promised AS monto_prometido, e.name AS evento, e.date_start AS fecha,
                        COALESCE(e.date_end, e.date_start) AS fecha_fin, e.location AS lugar
                   FROM sponsor s JOIN events e ON e.uuid = s.event_uuid
                  WHERE s.uuid = ?1"
            }
            Kind::Student => {
                "SELECT nombre AS nombre, student_id AS numero_estudiante, COALESCE(email, email_work) AS correo,
                        compania AS compania
                   FROM students WHERE uuid = ?1"
            }
        }
    }
}

pub struct RecordGroup {
    pub label: &'static str,
    pub opts: Vec<(String, String)>,
}

fn record_groups(conn: &Connection, module: &str) -> rusqlite::Result<Vec<RecordGroup>> {
    Kind::of_module(module)
        .iter()
        .map(|k| {
            let opts = conn
                .prepare(k.list_sql())?
                .query_map([], |r| Ok((format!("{}:{}", k.prefix(), r.get::<_, String>(0)?), r.get(1)?)))?
                .collect::<Result<_, _>>()?;
            Ok(RecordGroup { label: k.group(), opts })
        })
        .collect()
}

/// The record the preview starts with. A sponsor letter (code or sponsor fields) starts with a sponsor.
fn default_record(conn: &Connection, doc: &Doc) -> rusqlite::Result<String> {
    let kinds = Kind::of_module(&doc.module);
    let sponsorish = doc.code.contains("sponsor")
        || tokens(&doc.body).iter().any(|t| CONVENTION_FIELDS.iter().any(|f| f.token == t && f.group == "Auspiciador"));
    let mut order: Vec<Kind> = kinds.to_vec();
    if sponsorish {
        order.sort_by_key(|k| *k != Kind::Sponsor);
    }
    for k in order {
        if let Some(uuid) = conn.query_row(k.default_sql(), [], |r| r.get::<_, String>(0)).optional()? {
            return Ok(format!("{}:{uuid}", k.prefix()));
        }
    }
    Ok(String::new())
}

/// Values by token for `prefix:uuid` of the module (raw stored values; [`show`] formats them).
fn record_values(conn: &Connection, module: &str, id: &str) -> rusqlite::Result<HashMap<String, Value>> {
    let mut out = HashMap::new();
    let Some((prefix, uuid)) = id.split_once(':') else { return Ok(out) };
    let Some(kind) = Kind::of_module(module).iter().find(|k| k.prefix() == prefix) else { return Ok(out) };
    let mut stmt = conn.prepare(kind.values_sql())?;
    let names: Vec<String> = stmt.column_names().into_iter().map(str::to_owned).collect();
    let row = stmt
        .query_row([uuid], |r| (0..names.len()).map(|i| r.get::<_, Value>(i)).collect::<Result<Vec<_>, _>>())
        .optional()?;
    if let Some(vals) = row {
        out.extend(names.into_iter().zip(vals));
    }
    Ok(out)
}

/// Organization settings: name, logo, the signer of the module, today.
struct Org {
    name: String,
    logo_uuid: Option<String>,
    signer: String,
    signer_title: String,
}

fn org(conn: &Connection, module: &str) -> rusqlite::Result<Org> {
    let get = |k: &str| crate::db::setting(conn, k).map(|v| v.unwrap_or_default());
    let who = if module == "convention" { "convention" } else { "school" };
    Ok(Org {
        name: get("org.name")?,
        logo_uuid: crate::db::setting(conn, "logo_file_uuid")?.filter(|s| !s.is_empty()),
        signer: get(&format!("{who}.signer_name"))?,
        signer_title: get(&format!("{who}.signer_title"))?,
    })
}

fn org_values(o: &Org) -> HashMap<String, Value> {
    let t = |s: &str| if s.is_empty() { Value::Null } else { Value::Text(s.to_owned()) };
    HashMap::from([
        ("organizacion".to_owned(), t(&o.name)),
        ("logo".to_owned(), o.logo_uuid.as_deref().map_or(Value::Null, t)),
        ("firmante".to_owned(), t(&o.signer)),
        ("titulo_firmante".to_owned(), t(&o.signer_title)),
        ("fecha_hoy".to_owned(), Value::Text(crate::clock::now_text())),
    ])
}

pub fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

/// The HTML of one value as the app shows it, or `None` when there is no value.
pub fn show(fmt: Fmt, v: &Value) -> Option<String> {
    let text = match v {
        Value::Null | Value::Blob(_) => return None,
        Value::Integer(n) => n.to_string(),
        Value::Real(x) if x.fract() == 0.0 => format!("{x:.0}"),
        Value::Real(x) => x.to_string(),
        Value::Text(s) => s.trim().to_owned(),
    };
    if text.is_empty() {
        return None;
    }
    match fmt {
        Fmt::Text => Some(escape(&text)),
        Fmt::Date => crate::clock::spanish_date(&text).map(|d| escape(&d)),
        Fmt::Money => match v {
            Value::Integer(c) => Some(crate::seed::usd(*c)),
            _ => None,
        },
        Fmt::Logo => Some(format!("<img class=\"tpl-logo-img\" src=\"/files/{}\" alt=\"Logo\">", escape(&text))),
    }
}

/// Replaces every `{{token}}` of a sanitized body. A missing value or an unknown token becomes an empty, light
/// placeholder; the body never shows a raw token.
pub fn merge(body: &str, fields: &[&MergeField], values: &HashMap<String, Value>) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(i) = rest.find("{{") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        let Some(j) = after.find("}}") else {
            out.push_str(&rest[i..]);
            return out;
        };
        let token = after[..j].trim();
        let field = fields.iter().find(|f| f.token == token);
        let shown = field.and_then(|f| values.get(token).and_then(|v| show(f.fmt, v)));
        match (shown, field) {
            (Some(html), _) => out.push_str(&html),
            (None, Some(f)) => {
                out.push_str(&format!("<span class=\"mf-empty\" title=\"Sin dato: {}\"></span>", f.label))
            }
            (None, None) => out.push_str("<span class=\"mf-empty\" title=\"Sin dato\"></span>"),
        }
        rest = &after[j + 2..];
    }
    out.push_str(rest);
    out
}

#[derive(Template)]
#[template(path = "admin/template_preview.html")]
struct Letter {
    org: String,
    initials: String,
    logo_url: Option<String>,
    body: String,
    signer: String,
    signer_title: String,
    show_signer: bool,
}

/// The letter of a body for one record (`prefix:uuid`, or "" for none).
fn render_letter(conn: &Connection, module: &str, body: &str, record: &str) -> Result<String, AppError> {
    let o = org(conn, module)?;
    let mut values = record_values(conn, module, record)?;
    values.extend(org_values(&o));
    let clean = sanitize(body);
    let used = tokens(&clean);
    let letter = Letter {
        initials: o
            .name
            .split_whitespace()
            .filter_map(|w| w.chars().next())
            .filter(|c| c.is_uppercase())
            .take(4)
            .collect(),
        logo_url: o.logo_uuid.as_ref().map(|u| format!("/files/{u}")),
        body: merge(&clean, &fields(module), &values),
        show_signer: !used.iter().any(|t| t == "firmante") && !o.signer.is_empty(),
        org: o.name,
        signer: o.signer,
        signer_title: o.signer_title,
    };
    Ok(letter.render()?)
}

// ---------------------------------------------------------------------------------------------------------------
// Page, autosave, preview
// ---------------------------------------------------------------------------------------------------------------

struct Doc {
    code: String,
    title: String,
    module: String,
    module_label: &'static str,
    body: String,
    changed: String,
    changed_by: String,
}

fn doc_get(conn: &Connection, code: &str) -> Result<Doc, AppError> {
    Ok(conn.query_row(
        "SELECT code, title, module, body_html, modified_at, COALESCE(modified_by, '')
           FROM document_templates WHERE code = ?1",
        [code],
        |r| {
            let module: String = r.get(2)?;
            let at: String = r.get(4)?;
            Ok(Doc {
                code: r.get(0)?,
                title: r.get(1)?,
                module_label: super::settings::module_label(&module),
                module,
                body: r.get(3)?,
                changed: crate::admin::users::us_datetime(&at),
                changed_by: r.get(5)?,
            })
        },
    )?)
}

fn doc_url(code: &str) -> String {
    format!("/admin/templates/{code}")
}

fn title_field(code: &str, saved: &str) -> FieldView {
    FieldView::new(doc_url(code), "title", "dt-title", Control::Text, saved)
}

/// The hidden autosave field of the body; the editor above it writes into it.
fn body_field(code: &str, saved: &str) -> FieldView {
    FieldView::new(doc_url(code), "body_html", "dt-body", Control::Textarea(4), saved).class(" rte-source")
}

pub struct FieldGroup {
    pub label: &'static str,
    pub fields: Vec<&'static MergeField>,
}

fn field_groups(module: &str) -> Vec<FieldGroup> {
    let mut out: Vec<FieldGroup> = Vec::new();
    for f in fields(module) {
        match out.last_mut() {
            Some(g) if g.label == f.group => g.fields.push(f),
            _ => out.push(FieldGroup { label: f.group, fields: vec![f] }),
        }
    }
    out
}

#[derive(Template)]
#[template(path = "admin/template.html", blocks = ["content"])]
struct TemplatePage {
    shell: Shell,
    doc: Doc,
    title: FieldView,
    body: FieldView,
    groups: Vec<FieldGroup>,
    records: Vec<RecordGroup>,
    record: String,
    letter: String,
}

impl Page for TemplatePage {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

async fn template_page(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    Path(code): Path<String>,
) -> Result<Html<String>, AppError> {
    let page = {
        let conn = state.db();
        let doc = doc_get(&conn, &code)?;
        let record = default_record(&conn, &doc)?;
        let letter = render_letter(&conn, &doc.module, &doc.body, &record)?;
        let mut shell = Shell::new(&user, "admin", "admin-configuracion");
        shell.autosave = true;
        TemplatePage {
            title: title_field(&doc.code, &doc.title),
            body: body_field(&doc.code, &doc.body),
            groups: field_groups(&doc.module),
            records: record_groups(&conn, &doc.module)?,
            record,
            letter,
            shell,
            doc,
        }
    };
    render_page(&headers, &page)
}

async fn patch_template(
    State(state): State<AppState>,
    user: CurrentUser,
    rid: Option<Extension<RequestId>>,
    Path(code): Path<String>,
    field: OneField,
) -> Result<Response, AppError> {
    let Some(def) = TEMPLATES.get(&field.name) else {
        return Ok(autosave::contract_error("Este campo no se puede cambiar aquí."));
    };
    let actor = user.actor();
    let mut conn = state.db();
    let doc = doc_get(&conn, &code)?;
    let record = format!("Plantilla «{}»", doc.title);
    let ctx = Ctx { actor: &actor, request_id: rid.as_ref().map(|r| r.0 .0.as_str()), record: &record };
    let outcome =
        autosave::save(&mut conn, &TEMPLATES, &Value::Text(code.clone()), def, &field.value, &ctx, |_, _, _| Ok(()))?;
    let doc = doc_get(&conn, &code)?;
    let view = if def.name == "title" { title_field(&code, &doc.title) } else { body_field(&code, &doc.body) };
    Ok(match outcome {
        Outcome::Saved { .. } => autosave::respond(&view, ""),
        Outcome::Invalid(msg) => autosave::respond(&view.refused(&field.value, msg), ""),
    })
}

#[derive(Deserialize)]
struct PreviewForm {
    #[serde(default)]
    body_html: String,
    #[serde(default)]
    record: String,
}

/// `POST /admin/templates/{code}/preview`: the editor's current body merged with one record. Writes nothing.
async fn preview(
    State(state): State<AppState>,
    Path(code): Path<String>,
    Form(f): Form<PreviewForm>,
) -> Result<Html<String>, AppError> {
    let conn = state.db();
    let doc = doc_get(&conn, &code)?;
    Ok(Html(render_letter(&conn, &doc.module, &f.body_html, &f.record)?))
}

// ---------------------------------------------------------------------------------------------------------------
// Seed
// ---------------------------------------------------------------------------------------------------------------

/// The document templates of D-18 that the demo seed writes (`mba-seed`). Texts are synthetic.
pub fn seed_document_templates(conn: &Connection, at: &str, actor: &str) -> rusqlite::Result<()> {
    let rows: [(&str, &str, &str, Option<&str>, &str); 6] = [
        (
            "mba.sponsor_letter",
            "convention",
            "Carta a auspiciadores",
            Some("objects/MBA.fmp12/layouts/48-Carta_Auspicio_español.txt"),
            "<p>{{fecha_hoy}}</p><p>{{nombre}}<br>{{auspiciador}}</p><p>{{saludo}}</p><p>Le invitamos a auspiciar la {{evento}}. Su apoyo hace posible la educación continua de la industria hipotecaria.</p><p>La aportación de la categoría {{categoria}} es de <b>{{monto_solicitado}}</b>.</p><p>Cordialmente,</p><p><b>{{firmante}}</b><br>{{titulo_firmante}}</p>",
        ),
        (
            "mba.confirmation_letter",
            "convention",
            "Carta de confirmación",
            Some("objects/MBA.fmp12/layouts/61-Carta_de_Confirmacion.txt"),
            "<p>{{fecha_hoy}}</p><p>Estimado/a {{nombre}}:</p><p>Confirmamos su registro número {{registro}} para la {{evento}}, del {{fecha}} al {{fecha_fin}} en {{lugar}}. Su llegada al hotel es el {{llegada}} y su salida el {{salida}}.</p><ul><li>Total del registro: <b>{{total}}</b></li><li>Pagos recibidos: {{pagos}}</li><li>Balance pendiente: <b>{{balance}}</b></li></ul><p>Cordialmente,</p><p><b>{{firmante}}</b><br>{{titulo_firmante}}</p>",
        ),
        (
            "mbs.graduation_certificate",
            "school",
            "Certificado de graduación",
            Some("objects/MBS.fmp12/layouts/47-Certificado_Graduacion.txt"),
            "<h2>Certificado de graduación</h2><p>Se certifica que <b>{{nombre}}</b> completó los requisitos de la certificación {{certificacion}} el {{fecha_graduacion}}.</p><p><b>{{firmante}}</b><br>{{titulo_firmante}}</p>",
        ),
        (
            "mbs.course_attendance_certificate",
            "school",
            "Certificado de asistencia a curso",
            None,
            "<h2>Certificado de asistencia</h2><p><b>{{nombre}}</b> asistió al curso {{curso}} el {{fecha}}.</p><p><b>{{firmante}}</b><br>{{titulo_firmante}}</p>",
        ),
        (
            "mba.cancellation_policy",
            "convention",
            "Política de cancelación",
            None,
            "<p>Las cancelaciones recibidas por escrito antes de la fecha límite de la {{evento}} reciben reembolso, menos un cargo administrativo. Después de esa fecha no hay reembolso.</p>",
        ),
        (
            "mbs.student_welcome_letter",
            "school",
            "Carta de bienvenida a estudiante",
            None,
            "<p>Estimado/a {{nombre}}:</p><p>Bienvenido/a a la Escuela. Su primer curso, {{curso}}, comienza el {{fecha}}.</p><p><b>{{firmante}}</b><br>{{titulo_firmante}}</p>",
        ),
    ];
    for (code, module, title, source, body) in rows {
        conn.execute(
            "INSERT INTO document_templates (code, module, title, body_html, source_layout, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![code, module, title, body, source, at, actor],
        )?;
    }
    Ok(())
}
