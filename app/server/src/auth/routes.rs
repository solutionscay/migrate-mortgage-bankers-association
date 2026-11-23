//! OP-AUTH-01 sign in (UI-01), OP-AUTH-02 sign out, OP-AUTH-03 change own password (UI-33).

use super::{
    clear_cookie, constant_time_eq, cookie, password, random_token, session, set_cookie, CurrentUser, Shell,
    LOGIN_CSRF_COOKIE, SESSION_COOKIE,
};
use crate::audit;
use crate::clock;
use crate::error::AppError;
use crate::web::{render_page, render_page_status, safe_next, Page};
use crate::AppState;
use askama::Template;
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use serde_json::json;

/// Failed sign-ins before the lock (AP-11).
pub const MAX_FAILED: i64 = 5;
/// Lock length in minutes (AP-11).
pub const LOCK_MINUTES: i64 = 15;

/// Routes that work without a session.
pub fn public_routes() -> Router<AppState> {
    Router::new()
        .route("/login", get(login_page).post(login))
        .route("/favicon.ico", get(|| async { StatusCode::NO_CONTENT }))
}

/// Routes for any signed-in user (AP-01).
pub fn account_routes() -> Router<AppState> {
    Router::new().route("/logout", post(logout)).route("/account/password", get(password_page).post(change_password))
}

// ---------------------------------------------------------------------------------------------------------------
// UI-01 sign in
// ---------------------------------------------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "login.html")]
struct LoginTemplate {
    csrf: String,
    username: String,
    next: String,
    error: Option<&'static str>,
}

#[derive(Deserialize)]
struct NextQuery {
    next: Option<String>,
}

/// The sign-in form. CSRF before a session exists: a random token in an HttpOnly cookie scoped to `/login` and the
/// same value in the form (double submit).
async fn login_page(State(state): State<AppState>, headers: HeaderMap, Query(q): Query<NextQuery>) -> Response {
    if let Some(t) = cookie(&headers, SESSION_COOKIE) {
        if matches!(session::lookup(&state, &t), Ok(Some(_))) {
            return Redirect::to(safe_next(q.next.as_deref())).into_response();
        }
    }
    let next = safe_next(q.next.as_deref()).to_owned();
    login_form(&state, &headers, StatusCode::OK, String::new(), next, None)
}

fn login_form(
    state: &AppState,
    headers: &HeaderMap,
    status: StatusCode,
    username: String,
    next: String,
    error: Option<&'static str>,
) -> Response {
    // Keep the token the browser already has: a second tab, or a stray request that lands on /login, must not
    // invalidate a form that is already open.
    let csrf = cookie(headers, LOGIN_CSRF_COOKIE)
        .filter(|t| t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit()))
        .unwrap_or_else(random_token);
    let page = LoginTemplate { csrf: csrf.clone(), username, next, error };
    match page.render() {
        Ok(html) => {
            let mut res = (status, Html(html)).into_response();
            res.headers_mut()
                .append(header::SET_COOKIE, set_cookie(LOGIN_CSRF_COOKIE, &csrf, "/login", state.config.secure_cookie));
            res
        }
        Err(e) => AppError::from(e).into_response(),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginForm {
    username: String,
    password: String,
    #[serde(rename = "_csrf")]
    csrf: String,
    #[serde(default)]
    next: Option<String>,
}

/// One generic message for every failure: wrong user, wrong password, disabled, locked (BR-AUTH-01, AP-11).
const LOGIN_ERROR: &str = "Usuario o contraseña incorrectos.";

/// OP-AUTH-01.
async fn login(State(state): State<AppState>, headers: HeaderMap, Form(f): Form<LoginForm>) -> Response {
    let next = safe_next(f.next.as_deref()).to_owned();
    let expected = cookie(&headers, LOGIN_CSRF_COOKIE);
    if !expected.is_some_and(|e| constant_time_eq(&e, &f.csrf)) {
        return login_form(
            &state,
            &headers,
            StatusCode::FORBIDDEN,
            f.username,
            next,
            Some("El formulario venció. Intente otra vez."),
        );
    }
    let username = f.username.trim().to_owned();

    let row = {
        let conn = state.db();
        conn.query_row(
            "SELECT id, password_hash, active, locked_until FROM users WHERE username = ?1",
            [&username],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, Option<String>>(3)?)),
        )
        .optional()
    };
    let row = match row {
        Ok(r) => r,
        Err(e) => return AppError::from(e).into_response(),
    };

    // Verify off the async threads; an unknown user is checked against the dummy hash (same time either way).
    let phc = row.as_ref().map(|r| r.1.clone()).unwrap_or_else(|| password::DUMMY_HASH.clone());
    let pw = f.password;
    let ok = tokio::task::spawn_blocking(move || password::verify(&pw, &phc)).await.unwrap_or(false);

    let ua = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).map(str::to_owned);
    let outcome = {
        let conn = state.db();
        sign_in_outcome(&conn, row, ok, ua.as_deref())
    };
    match outcome {
        Ok(Some(token)) => {
            let mut res = Redirect::to(&next).into_response();
            let h = res.headers_mut();
            h.append(header::SET_COOKIE, set_cookie(SESSION_COOKIE, &token, "/", state.config.secure_cookie));
            h.append(header::SET_COOKIE, clear_cookie(LOGIN_CSRF_COOKIE, "/login"));
            res
        }
        Ok(None) => login_form(&state, &headers, StatusCode::UNAUTHORIZED, username, next, Some(LOGIN_ERROR)),
        Err(e) => AppError::from(e).into_response(),
    }
}

/// Counters, lock and session in the database. `Some(token)` on success.
fn sign_in_outcome(
    conn: &rusqlite::Connection,
    row: Option<(i64, String, i64, Option<String>)>,
    password_ok: bool,
    user_agent: Option<&str>,
) -> rusqlite::Result<Option<String>> {
    let Some((id, _, active, locked_until)) = row else {
        // The typed username is not stored: it may be a password typed in the wrong box.
        audit::log(conn, None, "auth.login_failed", Some("users"), None, None)?;
        return Ok(None);
    };
    let now = clock::now();
    let locked = locked_until.as_deref().and_then(clock::parse).is_some_and(|t| t > now);
    if active != 1 || locked {
        let why = if active != 1 { "disabled" } else { "locked" };
        audit::log(conn, Some(id), "auth.login_failed", Some("users"), None, Some(&json!({ "reason": why })))?;
        return Ok(None);
    }
    if !password_ok {
        let failed: i64 = conn.query_row(
            "UPDATE users SET failed_login_count = failed_login_count + 1 WHERE id = ?1 RETURNING failed_login_count",
            [id],
            |r| r.get(0),
        )?;
        audit::log(conn, Some(id), "auth.login_failed", Some("users"), None, Some(&json!({ "reason": "password" })))?;
        if failed >= MAX_FAILED {
            conn.execute(
                "UPDATE users SET locked_until = ?1, failed_login_count = 0 WHERE id = ?2",
                params![clock::in_minutes(LOCK_MINUTES), id],
            )?;
            audit::log(conn, Some(id), "auth.locked", Some("users"), None, Some(&json!({ "minutes": LOCK_MINUTES })))?;
        }
        return Ok(None);
    }
    let now = clock::fmt(now);
    conn.execute(
        "UPDATE users SET failed_login_count = 0, locked_until = NULL, last_login_at = ?1 WHERE id = ?2",
        params![now, id],
    )?;
    let token = session::create(conn, id, user_agent)?;
    audit::log(conn, Some(id), "auth.login", Some("users"), None, None)?;
    Ok(Some(token))
}

// ---------------------------------------------------------------------------------------------------------------
// OP-AUTH-02 sign out
// ---------------------------------------------------------------------------------------------------------------

/// Deletes this session (the CSRF layer has checked the token).
async fn logout(State(state): State<AppState>, user: CurrentUser) -> Result<Response, AppError> {
    {
        let conn = state.db();
        conn.execute("DELETE FROM sessions WHERE token_hash = ?1", [&user.token_hash])?;
        audit::log(&conn, Some(user.id), "auth.logout", Some("users"), None, None)?;
    }
    let mut res = Redirect::to("/login").into_response();
    res.headers_mut().append(header::SET_COOKIE, clear_cookie(SESSION_COOKIE, "/"));
    Ok(res)
}

// ---------------------------------------------------------------------------------------------------------------
// UI-33 change own password (AP-12)
// ---------------------------------------------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "password.html", blocks = ["content"])]
struct PasswordTemplate {
    shell: Shell,
    errors: PasswordErrors,
    done: bool,
}

impl Page for PasswordTemplate {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

#[derive(Default)]
struct PasswordErrors {
    actual: Option<&'static str>,
    nueva: Option<&'static str>,
    repetir: Option<&'static str>,
}

impl PasswordErrors {
    fn any(&self) -> bool {
        self.actual.is_some() || self.nueva.is_some() || self.repetir.is_some()
    }
}

#[derive(Deserialize)]
struct DoneQuery {
    hecho: Option<String>,
}

async fn password_page(
    user: CurrentUser,
    headers: HeaderMap,
    Query(q): Query<DoneQuery>,
) -> Result<Html<String>, AppError> {
    let page = PasswordTemplate {
        shell: Shell::new(&user, "inicio", "cambiar-contrasena"),
        errors: PasswordErrors::default(),
        done: q.hecho.is_some(),
    };
    render_page(&headers, &page)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PasswordForm {
    actual: String,
    nueva: String,
    repetir: String,
    #[serde(rename = "_csrf")]
    _csrf: String,
}

/// OP-AUTH-03: current password required; new hash argon2id; the user's other sessions end.
async fn change_password(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    Form(f): Form<PasswordForm>,
) -> Result<Response, AppError> {
    let phc: String = state.db().query_row("SELECT password_hash FROM users WHERE id = ?1", [user.id], |r| r.get(0))?;
    let actual = f.actual.clone();
    let current_ok = tokio::task::spawn_blocking(move || password::verify(&actual, &phc)).await.unwrap_or(false);

    let mut errors = PasswordErrors::default();
    if !current_ok {
        errors.actual = Some("No es correcta.");
    }
    if f.nueva.chars().count() < password::MIN_LEN {
        errors.nueva = Some("Use al menos 10 caracteres.");
    } else if f.nueva == f.actual {
        errors.nueva = Some("Debe ser distinta de la actual.");
    }
    if f.repetir != f.nueva {
        errors.repetir = Some("No coincide con la nueva.");
    }
    if errors.any() {
        let page = PasswordTemplate { shell: Shell::new(&user, "inicio", "cambiar-contrasena"), errors, done: false };
        return Ok(render_page_status(StatusCode::UNPROCESSABLE_ENTITY, &headers, &page));
    }

    let nueva = f.nueva;
    let new_hash = tokio::task::spawn_blocking(move || password::hash(&nueva))
        .await
        .map_err(|e| AppError::Internal(format!("hash task: {e}")))?;
    {
        let mut conn = state.db();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE users SET password_hash = ?1, modified_at = ?2 WHERE id = ?3",
            params![new_hash, clock::now_text(), user.id],
        )?;
        let closed = tx.execute(
            "DELETE FROM sessions WHERE user_id = ?1 AND token_hash <> ?2",
            params![user.id, user.token_hash],
        )?;
        audit::log(
            &tx,
            Some(user.id),
            "auth.password_changed",
            Some("users"),
            None,
            Some(&json!({ "sessions_closed": closed })),
        )?;
        tx.commit()?;
    }
    let mut res = Redirect::to("/account/password?hecho=1").into_response();
    if crate::web::is_htmx(&headers) {
        res.headers_mut().insert("hx-redirect", HeaderValue::from_static("/account/password?hecho=1"));
    }
    Ok(res)
}
