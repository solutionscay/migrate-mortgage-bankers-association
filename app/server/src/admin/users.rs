//! UI-29 Usuarios: OP-AUTH-04 (BR-AUTH-04, AP-08, AP-11). Query `user_list` never selects `password_hash`
//! (T-08-20).
//!
//! * `GET /users` list; `GET /users/{id}` the inline edit row (`?vista=fila` the display row).
//! * `POST /users` create with role and first password (dialog with Guardar).
//! * `PATCH /users/{id}` per-field autosave of `display_name`, `role`, `active`. Role or active change deletes the
//!   user's sessions; the last active admin cannot be demoted or disabled (T-08-16). A plain form (the disable
//!   dialog) gets 303 back to the list.
//! * `POST /users/{id}/password` set a password (dialog); the user's sessions end.
//! * `POST /users/{id}/unlock` clears a sign-in lock (menu item of the design).

use crate::audit::{self, RequestId};
use crate::auth::{password, CurrentUser, Shell};
use crate::autosave::{self, Control, Ctx, FieldDef, FieldView, OneField, Outcome, Refusal, Registry, Stamp};
use crate::clock;
use crate::error::AppError;
use crate::web::{is_htmx, render_page, render_page_status, Page};
use crate::AppState;
use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Extension, Form, Router};
use rusqlite::types::Value;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::Deserialize;
use serde_json::json;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/users", get(list).post(create))
        .route("/users/{id}", get(row).patch(patch))
        .route("/users/{id}/password", post(set_password))
        .route("/users/{id}/unlock", post(unlock))
}

/// The fields UI-29 autosaves (05 `PATCH /admin/users/{id}`). Passwords are never patched.
pub static USERS: Registry = Registry {
    table: "users",
    key_col: "id",
    stamp: Stamp::AtOnly,
    fields: &[
        FieldDef::new("display_name", "display_name", "Nombre", |s| autosave::required_text(s, 100)),
        FieldDef::new("role", "role", "Rol", parse_role).show(show_role),
        FieldDef::new("active", "active", "Activo", autosave::flag),
    ],
};

fn parse_role(s: &str) -> Result<Value, String> {
    match s.trim() {
        "admin" | "staff" => Ok(Value::Text(s.trim().to_owned())),
        _ => Err("Escoja Administrador o Personal.".into()),
    }
}

fn show_role(v: &Value) -> String {
    autosave::text_of(v)
}

// ---------------------------------------------------------------------------------------------------------------
// Query `user_list`
// ---------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct UserRow {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub active: bool,
    /// Set only while the lock is in the future.
    pub locked_until: Option<String>,
    pub last_login: Option<String>,
    /// The only active admin: the menu does not offer to disable it (the handler refuses anyway).
    pub last_admin: bool,
}

impl UserRow {
    pub fn role_label(&self) -> &'static str {
        if self.role == "admin" {
            "Administrador"
        } else {
            "Personal"
        }
    }
}

/// "11/21/2026 4:31 p. m." (design date-time format).
pub fn us_datetime(stored: &str) -> String {
    use chrono::Datelike;
    match clock::parse(stored) {
        Some(t) => format!("{}/{}/{} {}", t.month(), t.day(), t.year(), clock::spanish_time(t)),
        None => stored.to_owned(),
    }
}

/// `user_list` (05): the shown columns only, never `password_hash`.
pub fn user_list(conn: &Connection) -> rusqlite::Result<Vec<UserRow>> {
    let now = clock::now();
    let admins: i64 =
        conn.query_row("SELECT COUNT(*) FROM users WHERE role = 'admin' AND active = 1", [], |r| r.get(0))?;
    let mut stmt = conn.prepare(
        "SELECT id, username, display_name, role, active, locked_until, last_login_at
           FROM users ORDER BY username COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |r| {
        let role: String = r.get(3)?;
        let active = r.get::<_, i64>(4)? == 1;
        let locked: Option<String> = r.get(5)?;
        Ok(UserRow {
            id: r.get(0)?,
            username: r.get(1)?,
            display_name: r.get(2)?,
            last_admin: active && role == "admin" && admins == 1,
            role,
            active,
            locked_until: locked.filter(|l| clock::parse(l).is_some_and(|t| t > now)).map(|l| us_datetime(&l)),
            last_login: r.get::<_, Option<String>>(6)?.map(|l| us_datetime(&l)),
        })
    })?;
    rows.collect()
}

fn one_user(conn: &Connection, id: i64) -> Result<UserRow, AppError> {
    user_list(conn)?.into_iter().find(|u| u.id == id).ok_or(AppError::NotFound)
}

// ---------------------------------------------------------------------------------------------------------------
// The page
// ---------------------------------------------------------------------------------------------------------------

/// The create dialog: typed values and messages.
#[derive(Default, Debug)]
pub struct NewUser {
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub active: bool,
    pub errors: Vec<(&'static str, &'static str, String)>,
}

impl NewUser {
    fn err(&self, field: &str) -> bool {
        self.errors.iter().any(|(f, _, _)| *f == field)
    }
    pub fn invalid(&self, field: &str) -> &'static str {
        if self.err(field) {
            " aria-invalid=\"true\""
        } else {
            ""
        }
    }
}

/// The password dialog of one user after a refusal.
#[derive(Default, Debug)]
pub struct PasswordErrors {
    pub user_id: i64,
    pub nueva: Option<&'static str>,
    pub repetir: Option<&'static str>,
}

#[derive(Template)]
#[template(path = "admin/users.html", blocks = ["content"])]
pub struct UsersPage {
    pub shell: Shell,
    pub users: Vec<UserRow>,
    pub active_count: usize,
    pub locked_count: usize,
    pub notice: Option<&'static str>,
    pub error: Option<String>,
    pub new_user: NewUser,
    pub pw: PasswordErrors,
    /// Dialog to open on load (`body[data-open]`), after a refused dialog form.
    pub open: Option<String>,
}

impl Page for UsersPage {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

impl UsersPage {
    fn build(conn: &Connection, user: &CurrentUser) -> Result<Self, AppError> {
        let users = user_list(conn)?;
        let mut shell = Shell::new(user, "admin", "admin-usuarios");
        shell.autosave = true;
        Ok(Self {
            shell,
            active_count: users.iter().filter(|u| u.active).count(),
            locked_count: users.iter().filter(|u| u.locked_until.is_some()).count(),
            users,
            notice: None,
            error: None,
            new_user: NewUser { role: "staff".into(), active: true, ..Default::default() },
            pw: PasswordErrors::default(),
            open: None,
        })
    }
    pub fn pw_error(&self, id: i64) -> bool {
        self.pw.user_id == id && (self.pw.nueva.is_some() || self.pw.repetir.is_some())
    }
}

#[derive(Deserialize)]
struct ListQuery {
    hecho: Option<String>,
}

fn notice(code: Option<&str>) -> Option<&'static str> {
    Some(match code? {
        "creado" => "Usuario creado. Entregue la contraseña en persona.",
        "clave" => "Contraseña nueva guardada. Las sesiones de ese usuario se cerraron.",
        "desactivado" => "Usuario desactivado. Sus sesiones se cerraron.",
        "activado" => "Usuario activado.",
        "desbloqueado" => "Usuario desbloqueado.",
        _ => return None,
    })
}

async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    Query(q): Query<ListQuery>,
) -> Result<Html<String>, AppError> {
    let mut page = UsersPage::build(&state.db(), &user)?;
    page.notice = notice(q.hecho.as_deref());
    render_page(&headers, &page)
}

// ---------------------------------------------------------------------------------------------------------------
// Rows: display and inline edit
// ---------------------------------------------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "admin/user_row.html")]
struct RowTemplate {
    u: UserRow,
}

#[derive(Template)]
#[template(path = "admin/user_edit_row.html")]
struct EditRowTemplate {
    u: UserRow,
    name: FieldView,
    role: FieldView,
    active: FieldView,
}

fn url(id: i64) -> String {
    format!("/admin/users/{id}")
}

pub fn name_field(u: &UserRow) -> FieldView {
    FieldView::new(url(u.id), "display_name", &format!("u{}-name", u.id), Control::Text, &u.display_name).aria("Nombre")
}

pub fn role_field(id: i64, saved: &str) -> FieldView {
    let opts = vec![("staff".into(), "Personal".into()), ("admin".into(), "Administrador".into())];
    FieldView::new(url(id), "role", &format!("u{id}-role"), Control::Select(opts), saved).aria("Rol")
}

pub fn active_field(id: i64, saved: &str) -> FieldView {
    let opts = vec![("1".into(), "Activo".into()), ("0".into(), "Inactivo".into())];
    FieldView::new(url(id), "active", &format!("u{id}-active"), Control::Select(opts), saved).aria("Activo")
}

#[derive(Deserialize)]
struct RowQuery {
    vista: Option<String>,
}

async fn row(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<RowQuery>,
) -> Result<Html<String>, AppError> {
    let u = one_user(&state.db(), id)?;
    let html = if q.vista.as_deref() == Some("fila") {
        RowTemplate { u }.render()?
    } else {
        let name = name_field(&u);
        let role = role_field(u.id, &u.role);
        let active = active_field(u.id, if u.active { "1" } else { "0" });
        EditRowTemplate { u, name, role, active }.render()?
    };
    Ok(Html(html))
}

// ---------------------------------------------------------------------------------------------------------------
// PATCH /users/{id}: one field
// ---------------------------------------------------------------------------------------------------------------

/// AP-11 / T-08-16: refuses to demote or disable the last active admin; a role or active change deletes the
/// target user's sessions in the same transaction.
fn user_rule(tx: &Transaction, id: i64, column: &str, old: &Value, new: &Value) -> Result<(), Refusal> {
    if column != "role" && column != "active" || old == new {
        return Ok(());
    }
    let (role, active): (String, i64) =
        tx.query_row("SELECT role, active FROM users WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let stays_active_admin = match column {
        "role" => *new == Value::Text("admin".into()) && active == 1,
        _ => *new == Value::Integer(1) && role == "admin",
    };
    if role == "admin" && active == 1 && !stays_active_admin {
        let others: i64 =
            tx.query_row("SELECT COUNT(*) FROM users WHERE role = 'admin' AND active = 1 AND id <> ?1", [id], |r| {
                r.get(0)
            })?;
        if others == 0 {
            return Err(Refusal::Invalid("Es el último administrador activo: no se puede cambiar.".into()));
        }
    }
    let closed = tx.execute("DELETE FROM sessions WHERE user_id = ?1", [id])?;
    tracing::info!(user_id = id, closed, "sessions revoked after {column} change");
    Ok(())
}

async fn patch(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    rid: Option<Extension<RequestId>>,
    Path(id): Path<i64>,
    field: OneField,
) -> Result<Response, AppError> {
    let Some(def) = USERS.get(&field.name) else {
        return Ok(autosave::contract_error("Este campo no se puede cambiar aquí."));
    };
    let actor = user.actor();
    let outcome = {
        let mut conn = state.db();
        let username: String = conn
            .query_row("SELECT username FROM users WHERE id = ?1", [id], |r| r.get(0))
            .optional()?
            .ok_or(AppError::NotFound)?;
        let record = format!("Usuario {username}");
        let ctx = Ctx { actor: &actor, request_id: rid.as_ref().map(|r| r.0 .0.as_str()), record: &record };
        autosave::save(&mut conn, &USERS, &Value::Integer(id), def, &field.value, &ctx, |tx, old, new| {
            user_rule(tx, id, def.column, old, new)
        })?
    };

    if !is_htmx(&headers) {
        // The disable dialog and the activate menu item are plain forms.
        return Ok(match outcome {
            Outcome::Saved { .. } => {
                let code = match (def.name, field.value.as_str()) {
                    ("active", "0") => "desactivado",
                    ("active", _) => "activado",
                    _ => "",
                };
                Redirect::to(&format!("/admin/users?hecho={code}")).into_response()
            }
            Outcome::Invalid(msg) => {
                let mut page = UsersPage::build(&state.db(), &user)?;
                page.error = Some(msg);
                render_page_status(StatusCode::UNPROCESSABLE_ENTITY, &headers, &page)
            }
        });
    }

    let (saved_text, err) = match &outcome {
        Outcome::Saved { new, .. } => (def.display(new), None),
        Outcome::Invalid(msg) => {
            let conn = state.db();
            let old: Value =
                conn.query_row(&format!("SELECT {} FROM users WHERE id = ?1", def.column), [id], |r| r.get(0))?;
            (def.display(&old), Some(msg.clone()))
        }
    };
    let view = match def.name {
        "display_name" => {
            FieldView::new(url(id), "display_name", &format!("u{id}-name"), Control::Text, saved_text).aria("Nombre")
        }
        "role" => role_field(id, &saved_text),
        _ => active_field(id, &saved_text),
    };
    let view = match err {
        Some(msg) => view.refused(&field.value, msg),
        None => view,
    };
    Ok(autosave::respond(&view, ""))
}

// ---------------------------------------------------------------------------------------------------------------
// POST /users: create
// ---------------------------------------------------------------------------------------------------------------

#[derive(Deserialize)]
struct CreateForm {
    username: String,
    display_name: String,
    role: String,
    #[serde(default)]
    active: Option<String>,
    nueva: String,
    repetir: String,
    #[serde(rename = "_csrf")]
    _csrf: String,
}

fn valid_username(s: &str) -> bool {
    (3..=40).contains(&s.chars().count())
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    Form(f): Form<CreateForm>,
) -> Result<Response, AppError> {
    let username = f.username.trim().to_lowercase();
    let display_name = f.display_name.trim().to_owned();
    let mut nu = NewUser {
        username: f.username.trim().to_owned(),
        display_name: display_name.clone(),
        role: f.role.clone(),
        active: f.active.is_some(),
        errors: vec![],
    };
    if !valid_username(&username) {
        nu.errors.push(("username", "nu-user", "Usuario: de 3 a 40 letras minúsculas, números, punto o guion.".into()));
    } else if state
        .db()
        .query_row("SELECT 1 FROM users WHERE username = ?1", [&username], |_| Ok(()))
        .optional()?
        .is_some()
    {
        nu.errors.push(("username", "nu-user", "Usuario: ya existe.".into()));
    }
    if display_name.is_empty() || display_name.chars().count() > 100 {
        nu.errors.push(("display_name", "nu-name", "Nombre: escriba de 1 a 100 caracteres.".into()));
    }
    if parse_role(&f.role).is_err() {
        nu.errors.push(("role", "nu-role", "Rol: escoja Administrador o Personal.".into()));
    }
    if f.nueva.chars().count() < password::MIN_LEN {
        nu.errors.push(("nueva", "nu-pw1", "Contraseña: use al menos 10 caracteres.".into()));
    }
    if f.repetir != f.nueva {
        nu.errors.push(("repetir", "nu-pw2", "Repetir: no coincide con la contraseña.".into()));
    }
    if !nu.errors.is_empty() {
        let mut page = UsersPage::build(&state.db(), &user)?;
        page.new_user = nu;
        page.open = Some("dlg-nuevo".into());
        return Ok(render_page_status(StatusCode::UNPROCESSABLE_ENTITY, &headers, &page));
    }

    let pw = f.nueva;
    let hash = tokio::task::spawn_blocking(move || password::hash(&pw))
        .await
        .map_err(|e| AppError::Internal(format!("hash task: {e}")))?;
    {
        let mut conn = state.db();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let now = clock::now_text();
        tx.execute(
            "INSERT INTO users (username, display_name, email, password_hash, role, active, created_at, modified_at)
             VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?6)",
            params![username, display_name, hash, f.role.trim(), nu.active as i64, now],
        )?;
        let id = tx.last_insert_rowid();
        audit::log(
            &tx,
            Some(user.id),
            "auth.user_created",
            Some("users"),
            Some(&id.to_string()),
            Some(&json!({ "role": f.role.trim(), "text": format!("Usuario {username} creado") })),
        )?;
        tx.commit()?;
    }
    Ok(Redirect::to("/admin/users?hecho=creado").into_response())
}

// ---------------------------------------------------------------------------------------------------------------
// POST /users/{id}/password, POST /users/{id}/unlock
// ---------------------------------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PasswordForm {
    nueva: String,
    repetir: String,
    #[serde(rename = "_csrf")]
    _csrf: String,
}

async fn set_password(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(f): Form<PasswordForm>,
) -> Result<Response, AppError> {
    let target = one_user(&state.db(), id)?;
    let mut pw = PasswordErrors { user_id: id, ..Default::default() };
    if f.nueva.chars().count() < password::MIN_LEN {
        pw.nueva = Some("Use al menos 10 caracteres.");
    }
    if f.repetir != f.nueva {
        pw.repetir = Some("No coincide con la nueva.");
    }
    if pw.nueva.is_some() || pw.repetir.is_some() {
        let mut page = UsersPage::build(&state.db(), &user)?;
        page.pw = pw;
        page.open = Some(format!("dlg-clave-{id}"));
        return Ok(render_page_status(StatusCode::UNPROCESSABLE_ENTITY, &headers, &page));
    }
    let nueva = f.nueva;
    let hash = tokio::task::spawn_blocking(move || password::hash(&nueva))
        .await
        .map_err(|e| AppError::Internal(format!("hash task: {e}")))?;
    {
        let mut conn = state.db();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE users SET password_hash = ?1, failed_login_count = 0, locked_until = NULL, modified_at = ?2
              WHERE id = ?3",
            params![hash, clock::now_text(), id],
        )?;
        // The admin's own current session stays open when the admin resets the own password here.
        let closed =
            tx.execute("DELETE FROM sessions WHERE user_id = ?1 AND token_hash <> ?2", params![id, user.token_hash])?;
        audit::log(
            &tx,
            Some(user.id),
            "auth.password_set",
            Some("users"),
            Some(&id.to_string()),
            Some(&json!({ "sessions_closed": closed, "text": format!("Contraseña nueva de {}", target.username) })),
        )?;
        tx.commit()?;
    }
    Ok(Redirect::to("/admin/users?hecho=clave").into_response())
}

async fn unlock(State(state): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> Result<Response, AppError> {
    {
        let conn = state.db();
        let username: String = conn
            .query_row("SELECT username FROM users WHERE id = ?1", [id], |r| r.get(0))
            .optional()?
            .ok_or(AppError::NotFound)?;
        conn.execute(
            "UPDATE users SET locked_until = NULL, failed_login_count = 0, modified_at = ?1 WHERE id = ?2",
            params![clock::now_text(), id],
        )?;
        audit::log(
            &conn,
            Some(user.id),
            "auth.unlocked",
            Some("users"),
            Some(&id.to_string()),
            Some(&json!({ "text": format!("Usuario {username} desbloqueado") })),
        )?;
    }
    Ok(Redirect::to("/admin/users?hecho=desbloqueado").into_response())
}
