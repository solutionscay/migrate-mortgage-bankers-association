//! Errors. Handlers return [`AppError`]; the response carries only a marker and a generic Spanish message. The
//! [`error_pages`] layer turns 403 and 404 markers into the full page inside the app shell (or a fragment for htmx),
//! because it knows the signed-in user. 5xx details go to the log only, never to the page (05).

use crate::auth::{CurrentUser, Shell};
use crate::web::{is_htmx, Page};
use crate::AppState;
use askama::Template;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Response};

#[derive(Debug)]
pub enum AppError {
    /// 403: wrong role, or a CSRF failure (AP-02, AP-08). No hint which record exists.
    Forbidden,
    NotFound,
    Internal(String),
}

/// Marker on a response that [`error_pages`] replaces with a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorPage {
    Forbidden,
    NotFound,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::Forbidden => marked(StatusCode::FORBIDDEN, ErrorPage::Forbidden, "Sin permiso."),
            AppError::NotFound => marked(StatusCode::NOT_FOUND, ErrorPage::NotFound, "Página no encontrada."),
            AppError::Internal(msg) => {
                tracing::error!(error = %msg, "internal error");
                (StatusCode::INTERNAL_SERVER_ERROR, Html("Ocurrió un error. Intente otra vez.")).into_response()
            }
        }
    }
}

fn marked(status: StatusCode, page: ErrorPage, text: &'static str) -> Response {
    let mut res = (status, Html(text)).into_response();
    res.extensions_mut().insert(page);
    res
}

impl From<askama::Error> for AppError {
    fn from(e: askama::Error) -> Self {
        AppError::Internal(format!("template: {e}"))
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        match e {
            rusqlite::Error::QueryReturnedNoRows => AppError::NotFound,
            e => AppError::Internal(format!("database: {e}")),
        }
    }
}

/// Fallback of the protected router: 404 inside the shell.
pub async fn not_found() -> AppError {
    AppError::NotFound
}

#[derive(Template)]
#[template(path = "error.html", blocks = ["content"])]
pub struct ErrorTemplate {
    pub shell: Shell,
    pub code: u16,
    pub title: &'static str,
    pub text: &'static str,
}

impl Page for ErrorTemplate {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

/// Replaces marked 403/404 responses with the error page of the design (`msgpage`, design/admin-*.html for 403).
pub async fn error_pages(State(_state): State<AppState>, req: Request, next: Next) -> Response {
    let user = req.extensions().get::<CurrentUser>().cloned();
    let htmx = is_htmx(req.headers());
    let res = next.run(req).await;
    let (Some(page), Some(user)) = (res.extensions().get::<ErrorPage>().copied(), user) else {
        return res;
    };
    let (code, title, text) = match page {
        ErrorPage::Forbidden => (403, "Sin permiso", "Su rol no permite esta acción. Pida acceso a un administrador."),
        ErrorPage::NotFound => {
            (404, "Página no encontrada", "Esta página no existe o llega en un incremento posterior del demo.")
        }
    };
    let t = ErrorTemplate { shell: Shell::new(&user, "inicio", ""), code, title, text };
    let body = if htmx { t.render_fragment() } else { t.render() };
    let mut out = match body {
        Ok(html) => (res.status(), Html(html)).into_response(),
        Err(e) => AppError::Internal(format!("template: {e}")).into_response(),
    };
    for (k, v) in res.headers() {
        if k != axum::http::header::CONTENT_TYPE && k != axum::http::header::CONTENT_LENGTH {
            out.headers_mut().insert(k.clone(), v.clone());
        }
    }
    out
}
