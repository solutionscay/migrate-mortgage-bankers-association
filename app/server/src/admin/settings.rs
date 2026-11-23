//! UI-30 Configuración: OP-SYS-01 settings (BR-SYS-03), OP-SYS-02 document templates (D-18), value lists.
//!
//! * `GET /settings` the tabs of `design/admin-configuracion.html`; `PATCH /settings` one setting per request. The
//!   field name is the `app_settings.key`; only the keys in [`SETTINGS`] are accepted, any other key is refused
//!   (05 "unknown key refused").
//! * Document templates (OP-SYS-02) live in [`super::templates`]; the tab here lists them.
//! * `GET /value-lists?lista=` the values 04 seeds; `POST /value-lists` add, rename or move an item (05). A value
//!   that a CHECK constraint of the schema names cannot be renamed.

use crate::audit::{self, RequestId};
use crate::auth::{CurrentUser, Shell};
use crate::autosave::{self, Control, Ctx, FieldDef, FieldView, OneField, Outcome, Refusal, Registry, Stamp};
use crate::error::AppError;
use crate::web::{render_page, Page};
use crate::AppState;
use askama::Template;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::{Extension, Form, Router};
use rusqlite::types::Value;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::json;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/settings", get(settings_page).patch(patch_setting))
        .route("/value-lists", get(value_lists_page).post(value_list_command))
}

// ---------------------------------------------------------------------------------------------------------------
// OP-SYS-01 settings
// ---------------------------------------------------------------------------------------------------------------

/// The settings ROLE-ADMIN edits (BR-SYS-03). The field name is the key. `org.timezone` is fixed (D-67), so it is
/// not here. `logo_file_uuid` takes the uuid of an image uploaded through OP-SYS-04 (`sys::files::FileField`).
pub static SETTINGS: Registry = Registry {
    table: "app_settings",
    key_col: "key",
    stamp: Stamp::Full,
    fields: &[
        FieldDef::new("org.name", "value", "Nombre de la organización", |s| autosave::required_text(s, 200)),
        FieldDef::new("school.signer_name", "value", "Firmante de la Escuela", |s| autosave::optional_text(s, 200)),
        FieldDef::new("school.signer_title", "value", "Título del firmante de la Escuela", |s| {
            autosave::optional_text(s, 200)
        }),
        FieldDef::new("convention.signer_name", "value", "Firmante de la Convención", |s| {
            autosave::optional_text(s, 200)
        }),
        FieldDef::new("convention.signer_title", "value", "Título del firmante de la Convención", |s| {
            autosave::optional_text(s, 200)
        }),
        FieldDef::new("nmls_cost_x_hour_cents", "value", "Tarifa NMLS por hora", parse_nmls).show(show_dollars),
        FieldDef::new("csbs.provider_id", "value", "CSBS: id de proveedor", |s| autosave::required_text(s, 20)),
        FieldDef::new("csbs.course_id", "value", "CSBS: id de curso", |s| autosave::required_text(s, 20)),
        FieldDef::new("csbs.state_abbr", "value", "CSBS: estado", parse_state),
        FieldDef::new("csbs.postal_code", "value", "CSBS: código postal", |s| autosave::required_text(s, 10)),
        FieldDef::new("csbs.begin_time", "value", "CSBS: hora de inicio", parse_time),
        FieldDef::new("mail.default_template_uuid", "value", "Plantilla para envíos nuevos", |s| {
            autosave::optional_text(s, 64)
        }),
        FieldDef::new("session.idle_minutes", "value", "Minutos sin uso", parse_idle),
        FieldDef::new("logo_file_uuid", "value", "Logo", |s| autosave::optional_text(s, 64)),
    ],
};

fn parse_nmls(s: &str) -> Result<Value, String> {
    autosave::cents(s).map(|c| Value::Text(c.to_string()))
}

fn show_dollars(v: &Value) -> String {
    autosave::text_of(v).parse::<i64>().map(autosave::dollars).unwrap_or_default()
}

fn parse_state(s: &str) -> Result<Value, String> {
    let t = s.trim().to_uppercase();
    if t.len() == 2 && t.chars().all(|c| c.is_ascii_uppercase()) {
        Ok(Value::Text(t))
    } else {
        Err("Use dos letras, por ejemplo CS.".into())
    }
}

/// "8:00 AM" (the CSBS file format, D-25).
fn parse_time(s: &str) -> Result<Value, String> {
    let t = s.trim().to_uppercase();
    let ok = t
        .split_once(' ')
        .filter(|(_, ap)| *ap == "AM" || *ap == "PM")
        .and_then(|(hm, _)| hm.split_once(':'))
        .is_some_and(|(h, m)| {
            h.parse::<u8>().is_ok_and(|h| (1..=12).contains(&h))
                && m.len() == 2
                && m.parse::<u8>().is_ok_and(|m| m < 60)
        });
    if ok {
        Ok(Value::Text(t))
    } else {
        Err("Use el formato 8:00 AM.".into())
    }
}

fn parse_idle(s: &str) -> Result<Value, String> {
    autosave::int_in(s, 1, 480).map(|n| Value::Text(n.to_string()))
}

/// The form ids of the design page, so labels keep their `for`.
fn setting_id(key: &str) -> &'static str {
    match key {
        "org.name" => "cf-org",
        "school.signer_name" => "cf-f1",
        "school.signer_title" => "cf-f1t",
        "convention.signer_name" => "cf-f2",
        "convention.signer_title" => "cf-f2t",
        "nmls_cost_x_hour_cents" => "cf-nmls",
        "csbs.provider_id" => "cf-csbs1",
        "csbs.course_id" => "cf-csbs3",
        "csbs.state_abbr" => "cf-csbs2",
        "csbs.postal_code" => "cf-csbs4",
        "csbs.begin_time" => "cf-csbs5",
        "mail.default_template_uuid" => "cf-tpl",
        "session.idle_minutes" => "cf-idle",
        _ => "cf-x",
    }
}

fn template_options(conn: &Connection) -> rusqlite::Result<Vec<(String, String)>> {
    let mut out = vec![(String::new(), "Sin plantilla".to_owned())];
    let mut stmt = conn.prepare("SELECT uuid, subject FROM mail_templates ORDER BY subject")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        out.push(row?);
    }
    Ok(out)
}

/// The field of one setting, with its saved value.
fn setting_field(conn: &Connection, def: &FieldDef, saved: &str) -> rusqlite::Result<FieldView> {
    let id = setting_id(def.name);
    let f = |c| FieldView::new("/admin/settings", def.name, id, c, saved);
    Ok(match def.name {
        "nmls_cost_x_hour_cents" => f(Control::Money).class(" ma-short"),
        "session.idle_minutes" => {
            f(Control::Number).class(" ma-short").hint("Después la sesión cierra y pide entrar otra vez (BR-AUTH-02).")
        }
        "mail.default_template_uuid" => f(Control::Select(template_options(conn)?)),
        "csbs.provider_id" | "csbs.course_id" | "csbs.state_abbr" | "csbs.postal_code" | "csbs.begin_time" => {
            f(Control::Text).class(" ma-short")
        }
        "school.signer_title" => f(Control::Text).aria("Título del firmante de la Escuela"),
        "convention.signer_title" => f(Control::Text).aria("Título del firmante de la Convención"),
        _ => f(Control::Text),
    })
}

/// The stored value of a setting, as its field shows it.
fn saved_text(conn: &Connection, def: &FieldDef) -> rusqlite::Result<String> {
    let v: Value = conn.query_row("SELECT value FROM app_settings WHERE key = ?1", [def.name], |r| r.get(0))?;
    Ok(def.display(&v))
}

pub struct DocRow {
    pub code: String,
    pub title: String,
    pub module: &'static str,
    pub changed: String,
}

pub struct ListRow {
    pub list: String,
    pub label: &'static str,
    pub count: i64,
}

#[derive(Template)]
#[template(path = "admin/settings.html", blocks = ["content"])]
pub struct SettingsPage {
    pub shell: Shell,
    pub f: std::collections::HashMap<&'static str, FieldView>,
    pub timezone: String,
    pub logo: String,
    pub provider: &'static str,
    pub docs: Vec<DocRow>,
    pub lists: Vec<ListRow>,
}

impl Page for SettingsPage {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

impl SettingsPage {
    /// HTML of the field of `key` (every key of [`SETTINGS`] has one).
    pub fn field(&self, key: &str) -> String {
        self.f.get(key).map(FieldView::html).unwrap_or_default()
    }
}

pub fn module_label(m: &str) -> &'static str {
    match m {
        "convention" => "Convención",
        "school" => "Escuela",
        _ => "Correo",
    }
}

/// Spanish names of the value lists 04 seeds.
pub fn list_label(list: &str) -> &'static str {
    match list {
        "event_status" => "Estatus de evento",
        "registration_status" => "Estatus de registro",
        "event_item_types" => "Tipo de actividad",
        "tipo_de_pago" => "Tipo de pago",
        "registro_puerta_status" => "Estatus en la puerta",
        "categoria_auspiciador" => "Categoría de auspiciador",
        "costeado_por" => "Costeado por",
        "course_types" => "Tipo de requisito",
        "course_statuses" => "Estatus de curso",
        "course_calendar_day_type" => "Tipo de día del calendario",
        "course_enrollment_paid_by" => "Matrícula pagada por",
        "irregular_course_statuses" => "Estatus de asistencia",
        "activo_inactivo" => "Estatus de afiliado",
        "affiliates_membresia" => "Membresía de afiliado",
        "course_enrollment_status" => "Estatus de matrícula",
        _ => "Lista",
    }
}

async fn settings_page(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
) -> Result<Html<String>, AppError> {
    let page = {
        let conn = state.db();
        let mut f = std::collections::HashMap::new();
        for def in SETTINGS.fields {
            let saved = saved_text(&conn, def)?;
            f.insert(def.name, setting_field(&conn, def, &saved)?);
        }
        let timezone = crate::db::setting(&conn, "org.timezone")?.unwrap_or_default();
        let logo = logo_field(&conn)?.render()?;
        let docs = conn
            .prepare(
                "SELECT code, title, module, modified_at FROM document_templates ORDER BY modified_at DESC, title",
            )?
            .query_map([], |r| {
                let at: String = r.get(3)?;
                Ok(DocRow {
                    code: r.get(0)?,
                    title: r.get(1)?,
                    module: module_label(&r.get::<_, String>(2)?),
                    changed: us_date(&at),
                })
            })?
            .collect::<Result<_, _>>()?;
        let lists = conn
            .prepare("SELECT list, COUNT(*) FROM value_list_items GROUP BY list ORDER BY MIN(rowid)")?
            .query_map([], |r| {
                let list: String = r.get(0)?;
                Ok(ListRow { label: list_label(&list), list, count: r.get(1)? })
            })?
            .collect::<Result<_, _>>()?;
        let mut shell = Shell::new(&user, "admin", "admin-configuracion");
        shell.autosave = true;
        SettingsPage {
            shell,
            f,
            timezone,
            logo,
            provider: if state.config.resend_api_key.is_some() {
                "Resend · la clave vive en el entorno del servidor"
            } else {
                "mock · sin clave de Resend en el servidor"
            },
            docs,
            lists,
        }
    };
    render_page(&headers, &page)
}

/// The logo widget with the saved file (OP-SYS-04 chip); it autosaves the uuid to `PATCH /admin/settings`.
fn logo_field(conn: &Connection) -> rusqlite::Result<crate::sys::files::FileField> {
    let file = match crate::db::setting(conn, "logo_file_uuid")? {
        Some(uuid) => crate::sys::files::get_file(conn, &uuid)?,
        None => None,
    };
    Ok(crate::sys::files::FileField::new("logo_file_uuid", "logo", file)
        .with_save_url("/admin/settings")
        .with_accept(crate::sys::files::IMAGE_ACCEPT))
}

/// The empty widget with a refusal message (the stored logo stays as it was).
fn widget_error(mut w: crate::sys::files::FileField, msg: String) -> crate::sys::files::FileField {
    w.file = None;
    w.error =
        Some(if msg.contains("imagen") { "Use una imagen PNG, JPEG, GIF o WebP." } else { "No se guardó el logo." });
    w
}

/// "11/2/2026" (design date format).
pub fn us_date(stored: &str) -> String {
    use chrono::Datelike;
    crate::clock::parse(stored).map(|t| format!("{}/{}/{}", t.month(), t.day(), t.year())).unwrap_or_default()
}

async fn patch_setting(
    State(state): State<AppState>,
    user: CurrentUser,
    rid: Option<Extension<RequestId>>,
    field: OneField,
) -> Result<Response, AppError> {
    // BR-SYS-03: known keys only.
    let Some(def) = SETTINGS.get(&field.name) else {
        return Ok(autosave::contract_error("Esta configuración no existe o no se cambia aquí."));
    };
    let actor = user.actor();
    let mut conn = state.db();
    let ctx = Ctx { actor: &actor, request_id: rid.as_ref().map(|r| r.0 .0.as_str()), record: "Configuración" };
    let key = Value::Text(def.name.to_owned());
    let outcome = autosave::save(&mut conn, &SETTINGS, &key, def, &field.value, &ctx, |tx, _old, new| {
        if def.name == "logo_file_uuid" {
            if let Value::Text(uuid) = new {
                let mime: Option<String> = tx
                    .query_row("SELECT mime_type FROM stored_files WHERE uuid = ?1", [uuid], |r| r.get(0))
                    .optional()?;
                if !mime.is_some_and(|m| m.starts_with("image/")) {
                    return Err(Refusal::Invalid("Use una imagen PNG, JPEG, GIF o WebP.".into()));
                }
            }
        }
        if def.name == "mail.default_template_uuid" {
            if let Value::Text(uuid) = new {
                let known = tx
                    .query_row("SELECT 1 FROM mail_templates WHERE uuid = ?1", [uuid], |_| Ok(()))
                    .optional()?
                    .is_some();
                if !known {
                    return Err(Refusal::Invalid("Escoja una plantilla de la lista.".into()));
                }
            }
        }
        Ok(())
    })?;
    if def.name == "logo_file_uuid" {
        let mut widget = logo_field(&conn)?;
        let status = match outcome {
            Outcome::Saved { .. } => axum::http::StatusCode::OK,
            Outcome::Invalid(msg) => {
                widget = widget_error(widget, msg);
                axum::http::StatusCode::UNPROCESSABLE_ENTITY
            }
        };
        return Ok((status, Html(widget.render()?)).into_response());
    }
    let saved = saved_text(&conn, def)?;
    let view = setting_field(&conn, def, &saved)?;
    let view = match outcome {
        Outcome::Saved { .. } => view,
        Outcome::Invalid(msg) => view.refused(&field.value, msg),
    };
    Ok(autosave::respond(&view, ""))
}

// ---------------------------------------------------------------------------------------------------------------
// Value lists (`value_list_items`)
// ---------------------------------------------------------------------------------------------------------------

pub struct ValueRow {
    pub value: String,
    pub sort_order: i64,
    /// A CHECK constraint names this value: renaming it would break the rows that use it.
    pub locked: bool,
}

#[derive(Template)]
#[template(path = "admin/value_lists.html", blocks = ["content"])]
struct ValueListsPage {
    shell: Shell,
    lists: Vec<ListRow>,
    list: String,
    label: &'static str,
    values: Vec<ValueRow>,
    error: Option<String>,
    notice: Option<&'static str>,
}

impl Page for ValueListsPage {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

#[derive(Deserialize)]
struct ListQuery {
    lista: Option<String>,
    hecho: Option<String>,
    error: Option<String>,
}

/// True when a CHECK constraint (or other schema text) quotes the value. The schema is the source of truth: the
/// CHECK lists of 04 are the values the code compares with.
fn schema_names(conn: &Connection, value: &str) -> rusqlite::Result<bool> {
    let needle = format!("'{}'", value.replace('\'', "''"));
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND instr(sql, ?1) > 0",
        [&needle],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

fn list_rows(conn: &Connection) -> rusqlite::Result<Vec<ListRow>> {
    conn.prepare("SELECT list, COUNT(*) FROM value_list_items GROUP BY list ORDER BY MIN(rowid)")?
        .query_map([], |r| {
            let list: String = r.get(0)?;
            Ok(ListRow { label: list_label(&list), list, count: r.get(1)? })
        })?
        .collect()
}

async fn value_lists_page(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    Query(q): Query<ListQuery>,
) -> Result<Html<String>, AppError> {
    let page = {
        let conn = state.db();
        let lists = list_rows(&conn)?;
        let list = match q.lista {
            Some(l) if lists.iter().any(|r| r.list == l) => l,
            Some(_) => return Err(AppError::NotFound),
            None => lists.first().map(|l| l.list.clone()).unwrap_or_default(),
        };
        let mut values = Vec::new();
        let mut stmt =
            conn.prepare("SELECT value, sort_order FROM value_list_items WHERE list = ?1 ORDER BY sort_order, value")?;
        for row in stmt.query_map([&list], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
            let (value, sort_order) = row?;
            values.push(ValueRow { locked: schema_names(&conn, &value)?, value, sort_order });
        }
        ValueListsPage {
            shell: Shell::new(&user, "admin", "admin-configuracion"),
            label: list_label(&list),
            lists,
            list,
            values,
            error: q.error.as_deref().map(value_error).map(str::to_owned),
            notice: match q.hecho.as_deref() {
                Some("add") => Some("Valor añadido."),
                Some("rename") => Some("Valor cambiado."),
                Some("move") => Some("Orden cambiado."),
                _ => None,
            },
        }
    };
    render_page(&headers, &page)
}

fn value_error(code: &str) -> &'static str {
    match code {
        "vacio" => "Escriba un valor de 1 a 100 caracteres.",
        "existe" => "Ese valor ya está en la lista.",
        "bloqueado" => "Una regla de la base de datos usa este valor: no se puede cambiar.",
        _ => "No se pudo hacer el cambio.",
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ValueForm {
    op: String,
    list: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    new_value: String,
    #[serde(default)]
    dir: String,
    #[serde(rename = "_csrf")]
    _csrf: String,
}

/// `POST /admin/value-lists` (OP-SYS-01): `op=add|rename|move`. One transaction, one audit row.
async fn value_list_command(
    State(state): State<AppState>,
    user: CurrentUser,
    Form(f): Form<ValueForm>,
) -> Result<Response, AppError> {
    let back =
        |extra: &str| Redirect::to(&format!("/admin/value-lists?lista={}&{extra}", crate::web::urlencode(&f.list)));
    let mut conn = state.db();
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let exists: bool = tx
        .query_row("SELECT 1 FROM value_list_items WHERE list = ?1 LIMIT 1", [&f.list], |_| Ok(()))
        .optional()?
        .is_some();
    if !exists {
        return Err(AppError::NotFound);
    }
    let new = f.new_value.trim();
    let detail = match f.op.as_str() {
        "add" => {
            if new.is_empty() || new.chars().count() > 100 {
                return Ok(back("error=vacio").into_response());
            }
            let n = tx.execute(
                "INSERT OR IGNORE INTO value_list_items (list, value, sort_order)
                 VALUES (?1, ?2, (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM value_list_items WHERE list = ?1))",
                params![f.list, new],
            )?;
            if n == 0 {
                return Ok(back("error=existe").into_response());
            }
            json!({ "op": "add", "list": f.list, "value": new, "text": format!("Lista «{}»: valor «{new}» añadido", list_label(&f.list)) })
        }
        "rename" => {
            if new.is_empty() || new.chars().count() > 100 {
                return Ok(back("error=vacio").into_response());
            }
            if schema_names(&tx, &f.value)? {
                return Ok(back("error=bloqueado").into_response());
            }
            let taken = tx
                .query_row(
                    "SELECT 1 FROM value_list_items WHERE list = ?1 AND value = ?2",
                    params![f.list, new],
                    |_| Ok(()),
                )
                .optional()?;
            if taken.is_some() && new != f.value {
                return Ok(back("error=existe").into_response());
            }
            let n = tx.execute(
                "UPDATE value_list_items SET value = ?1 WHERE list = ?2 AND value = ?3",
                params![new, f.list, f.value],
            )?;
            if n == 0 {
                return Err(AppError::NotFound);
            }
            json!({ "op": "rename", "list": f.list, "old": f.value, "new": new, "text": format!("Lista «{}»: «{}» ahora es «{new}»", list_label(&f.list), f.value) })
        }
        "move" => {
            let cur: i64 = tx.query_row(
                "SELECT sort_order FROM value_list_items WHERE list = ?1 AND value = ?2",
                params![f.list, f.value],
                |r| r.get(0),
            )?;
            let neighbour = if f.dir == "up" {
                "SELECT value, sort_order FROM value_list_items WHERE list = ?1 AND sort_order < ?2 ORDER BY sort_order DESC LIMIT 1"
            } else {
                "SELECT value, sort_order FROM value_list_items WHERE list = ?1 AND sort_order > ?2 ORDER BY sort_order LIMIT 1"
            };
            let other: Option<(String, i64)> =
                tx.query_row(neighbour, params![f.list, cur], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
            let Some((other_value, other_order)) = other else {
                return Ok(back("hecho=move").into_response());
            };
            tx.execute(
                "UPDATE value_list_items SET sort_order = ?1 WHERE list = ?2 AND value = ?3",
                params![other_order, f.list, f.value],
            )?;
            tx.execute(
                "UPDATE value_list_items SET sort_order = ?1 WHERE list = ?2 AND value = ?3",
                params![cur, f.list, other_value],
            )?;
            json!({ "op": "move", "list": f.list, "value": f.value, "dir": f.dir, "text": format!("Lista «{}»: orden cambiado", list_label(&f.list)) })
        }
        _ => return Ok(back("error=op").into_response()),
    };
    audit::log(&tx, Some(user.id), "settings.change", Some("value_list_items"), Some(&f.list), Some(&detail))?;
    tx.commit()?;
    Ok(back(&format!("hecho={}", f.op)).into_response())
}
