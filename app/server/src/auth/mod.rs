//! Identity and authorization (08): sessions (AP-01), CSRF (AP-02), roles from the `users` table (AP-02 of 08's
//! role model, ROLE-ADMIN / ROLE-STAFF), `require_role` (AP-08), lockout (AP-11), own password (AP-12).
//!
//! Default deny: [`require_session`] wraps every route except `/login` and `/assets/*` (see `crate::router`). A route
//! added later is protected without doing anything. Handlers that need a role take [`RequireAdmin`] or
//! [`RequireStaff`]; whole areas use the [`require_admin`] layer. Hiding a link never replaces these checks.

pub mod password;
mod routes;
pub mod session;

pub use routes::{account_routes, public_routes};

use crate::error::AppError;
use crate::web::{is_htmx, urlencode, FormCsrf};
use crate::AppState;
use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::marker::PhantomData;

pub const SESSION_COOKIE: &str = "mba_session";
pub const LOGIN_CSRF_COOKIE: &str = "mba_login_csrf";
/// The CSRF header htmx sends (`hx-headers` on `<body>`, 05 "Per-field autosave").
pub const CSRF_HEADER: &str = "x-csrf-token";

/// ROLE-ADMIN and ROLE-STAFF (`users.role`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Admin,
    Staff,
}

impl Role {
    pub fn from_db(s: &str) -> Option<Role> {
        match s {
            "admin" => Some(Role::Admin),
            "staff" => Some(Role::Staff),
            _ => None,
        }
    }
    pub fn as_db(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Staff => "staff",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Role::Admin => "ROLE-ADMIN",
            Role::Staff => "ROLE-STAFF",
        }
    }
}

/// The signed-in user of a request, loaded by [`require_session`] from `sessions` + `users`. The role always comes
/// from the `users` row, never from the client.
#[derive(Clone, Debug)]
pub struct CurrentUser {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub role: Role,
    /// This session's CSRF token: only into this user's own forms (08 field policy).
    pub csrf: String,
    pub token_hash: String,
}

impl CurrentUser {
    pub fn actor(&self) -> crate::audit::Actor {
        crate::audit::Actor::user(self.id, self.username.clone())
    }
}

impl<S: Send + Sync> FromRequestParts<S> for CurrentUser {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        match parts.extensions.get::<CurrentUser>() {
            Some(u) => Ok(u.clone()),
            None => Err(deny(&parts.headers, &parts.method, "/")),
        }
    }
}

/// Which roles a [`RequireRole`] lets through.
pub trait RoleRule {
    const ALLOWED: &'static [Role];
}

/// ROLE-ADMIN only (AP-04, AP-05, AP-08, AP-11).
pub struct AdminOnly;
impl RoleRule for AdminOnly {
    const ALLOWED: &'static [Role] = &[Role::Admin];
}

/// ROLE-ADMIN or ROLE-STAFF (AP-03, AP-06, AP-09, AP-10).
pub struct AnyStaff;
impl RoleRule for AnyStaff {
    const ALLOWED: &'static [Role] = &[Role::Admin, Role::Staff];
}

/// `require_role` as an extractor: the handler runs only for an allowed role, else 403.
pub struct RequireRole<R: RoleRule>(pub CurrentUser, PhantomData<R>);
pub type RequireAdmin = RequireRole<AdminOnly>;
pub type RequireStaff = RequireRole<AnyStaff>;

impl<R: RoleRule, S: Send + Sync> FromRequestParts<S> for RequireRole<R> {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let user = CurrentUser::from_request_parts(parts, state).await?;
        require_role(&user, R::ALLOWED).map_err(IntoResponse::into_response)?;
        Ok(RequireRole(user, PhantomData))
    }
}

/// The role check itself.
pub fn require_role(user: &CurrentUser, allowed: &[Role]) -> Result<(), AppError> {
    if allowed.contains(&user.role) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

/// AP-08 as a layer: everything under `/admin` is ROLE-ADMIN only, including paths no handler serves yet.
pub async fn require_admin(req: Request, next: Next) -> Response {
    match req.extensions().get::<CurrentUser>() {
        Some(u) if require_role(u, AdminOnly::ALLOWED).is_ok() => next.run(req).await,
        _ => AppError::Forbidden.into_response(),
    }
}

/// AP-01: a valid session or no entry. Page requests go to the sign-in page (303, `?next=`), htmx requests get 401
/// with `HX-Redirect` (05 "Authorization matrix", T-05-01).
pub async fn require_session(State(state): State<AppState>, mut req: Request, next: Next) -> Response {
    let token = cookie(req.headers(), SESSION_COOKIE);
    let user = match &token {
        Some(t) => match session::lookup(&state, t) {
            Ok(u) => u,
            Err(e) => return AppError::Internal(format!("session lookup: {e}")).into_response(),
        },
        None => None,
    };
    match user {
        Some(user) => {
            let name = user.username.clone();
            req.extensions_mut().insert(user);
            let mut res = next.run(req).await;
            res.extensions_mut().insert(crate::audit::LogUser(name));
            res
        }
        None => {
            let target = match req.uri().path_and_query() {
                Some(pq) if req.method() == Method::GET => pq.as_str().to_owned(),
                _ => "/".to_owned(),
            };
            let mut res = deny(req.headers(), req.method(), &target);
            if token.is_some() {
                res.headers_mut().append(header::SET_COOKIE, clear_cookie(SESSION_COOKIE, "/"));
            }
            res
        }
    }
}

fn deny(headers: &HeaderMap, _method: &Method, target: &str) -> Response {
    if is_htmx(headers) {
        let mut res = StatusCode::UNAUTHORIZED.into_response();
        res.headers_mut().insert("hx-redirect", HeaderValue::from_static("/login"));
        res
    } else {
        let to = if target == "/" { "/login".to_owned() } else { format!("/login?next={}", urlencode(target)) };
        let mut res = StatusCode::SEE_OTHER.into_response();
        if let Ok(v) = HeaderValue::from_str(&to) {
            res.headers_mut().insert(header::LOCATION, v);
        }
        res
    }
}

/// AP-02 / BR-AUTH-03: every POST, PUT, PATCH and DELETE carries this session's token, in the `X-CSRF-Token` header
/// (htmx) or the `_csrf` form field. Missing or wrong: 403, nothing runs.
pub async fn csrf_guard(State(_state): State<AppState>, req: Request, next: Next) -> Response {
    if matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS) {
        return next.run(req).await;
    }
    let Some(user) = req.extensions().get::<CurrentUser>() else {
        return AppError::Forbidden.into_response();
    };
    let given = req
        .headers()
        .get(CSRF_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .or_else(|| req.extensions().get::<FormCsrf>().and_then(|f| f.0.clone()));
    match given {
        Some(g) if constant_time_eq(&g, &user.csrf) => next.run(req).await,
        _ => {
            tracing::warn!(user = %user.username, "csrf token missing or wrong");
            AppError::Forbidden.into_response()
        }
    }
}

/// The value of cookie `name`, if the request has it.
pub fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(n, _)| *n == name)
        .map(|(_, v)| v.to_owned())
}

/// `Set-Cookie` for a session-length, HttpOnly, SameSite=Lax cookie (AP-02); `Secure` when configured.
pub fn set_cookie(name: &str, value: &str, path: &str, secure: bool) -> HeaderValue {
    let secure = if secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!("{name}={value}; Path={path}; HttpOnly; SameSite=Lax{secure}"))
        .expect("hex cookie is a valid header value")
}

pub fn clear_cookie(name: &str, path: &str) -> HeaderValue {
    HeaderValue::from_str(&format!("{name}=; Path={path}; HttpOnly; SameSite=Lax; Max-Age=0"))
        .expect("valid header value")
}

/// Compares without stopping at the first differing byte.
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// 32 random bytes as hex (session tokens, CSRF tokens).
pub fn random_token() -> String {
    let mut b = [0u8; 32];
    rand::fill(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// What the base layout needs: who is signed in, the CSRF token, and the current module and nav tab
/// (`body[data-module]`, `body[data-nav]`, design/partials).
#[derive(Clone, Debug)]
pub struct Shell {
    pub username: String,
    pub display_name: String,
    pub role: &'static str,
    pub role_label: &'static str,
    pub is_admin: bool,
    pub csrf: String,
    pub module: &'static str,
    pub nav: &'static str,
    /// The page has autosave fields: the footer shows the save mark (05 "Save state"). Set by record pages.
    pub autosave: bool,
}

impl Shell {
    pub fn new(user: &CurrentUser, module: &'static str, nav: &'static str) -> Self {
        Self {
            username: user.username.clone(),
            display_name: user.display_name.clone(),
            role: user.role.as_db(),
            role_label: user.role.label(),
            is_admin: user.role == Role::Admin,
            csrf: user.csrf.clone(),
            module,
            nav,
            autosave: false,
        }
    }

    /// `aria-current` for a module tab.
    pub fn module_current(&self, module: &str) -> &'static str {
        if self.module == module {
            " aria-current=\"true\""
        } else {
            ""
        }
    }

    /// `aria-current` for a nav tab.
    pub fn nav_current(&self, nav: &str) -> &'static str {
        if self.nav == nav {
            " aria-current=\"page\""
        } else {
            ""
        }
    }
}
