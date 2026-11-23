//! Shared web pieces: static assets, security headers, `_method` override, htmx detection, page rendering.

use crate::error::AppError;
use crate::AppState;
use askama::Template;
use axum::body::Body;
use axum::extract::{Path, Request};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;

/// Request body limit (05: 25 MB).
pub const MAX_BODY: usize = 25 * 1024 * 1024;

/// Static files, embedded at build time. CSS, JS and fonts are the M1 design files as written
/// (`design/assets/`); htmx is vendored with its version in the name (05 "Static files").
const ASSETS: &[(&str, &str, &[u8])] = &[
    ("app.css", "text/css; charset=utf-8", include_bytes!("../../../design/assets/app.css")),
    ("app.js", "text/javascript; charset=utf-8", include_bytes!("../../../design/assets/app.js")),
    (
        "fonts/IBMPlexSans-Regular.woff2",
        "font/woff2",
        include_bytes!("../../../design/assets/fonts/IBMPlexSans-Regular.woff2"),
    ),
    (
        "fonts/IBMPlexSans-Medium.woff2",
        "font/woff2",
        include_bytes!("../../../design/assets/fonts/IBMPlexSans-Medium.woff2"),
    ),
    (
        "fonts/IBMPlexSans-SemiBold.woff2",
        "font/woff2",
        include_bytes!("../../../design/assets/fonts/IBMPlexSans-SemiBold.woff2"),
    ),
    (
        "fonts/IBMPlexSans-Bold.woff2",
        "font/woff2",
        include_bytes!("../../../design/assets/fonts/IBMPlexSans-Bold.woff2"),
    ),
    (
        "vendor/htmx-2.0.4.min.js",
        "text/javascript; charset=utf-8",
        include_bytes!("../assets/vendor/htmx-2.0.4.min.js"),
    ),
    ("icons/favicon.svg", "image/svg+xml", include_bytes!("../../../design/assets/icons/favicon.svg")),
    ("icons/favicon.ico", "image/x-icon", include_bytes!("../../../design/assets/icons/favicon.ico")),
    ("icons/icon-180.png", "image/png", include_bytes!("../../../design/assets/icons/icon-180.png")),
    ("icons/icon-192.png", "image/png", include_bytes!("../../../design/assets/icons/icon-192.png")),
    ("icons/icon-512.png", "image/png", include_bytes!("../../../design/assets/icons/icon-512.png")),
    (
        "icons/site.webmanifest",
        "application/manifest+json",
        include_bytes!("../../../design/assets/icons/site.webmanifest"),
    ),
];

pub fn asset_routes() -> Router<AppState> {
    Router::new()
        .route("/assets/{*path}", get(asset))
        // Browsers ask for /favicon.ico without reading the head; public, so the login page has it too.
        .route("/favicon.ico", get(|| asset(Path("icons/favicon.ico".into()))))
}

async fn asset(Path(path): Path<String>) -> Response {
    match ASSETS.iter().find(|(p, _, _)| *p == path) {
        Some((_, ctype, body)) => {
            ([(header::CONTENT_TYPE, *ctype), (header::CACHE_CONTROL, "no-cache")], *body).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Scripts, styles and fonts from this server only. htmx adds its indicator style inline, hence 'unsafe-inline'
/// for styles; scripts stay 'self' (no inline script anywhere).
const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; \
                   font-src 'self'; connect-src 'self'; form-action 'self'; base-uri 'none'; \
                   frame-ancestors 'none'; object-src 'none'";

pub async fn security_headers(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.entry(header::CONTENT_SECURITY_POLICY).or_insert(HeaderValue::from_static(CSP));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    if !h.contains_key(header::CACHE_CONTROL) {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    res
}

/// The form fields the outer layer read from a url-encoded body, for the CSRF check (AP-02).
#[derive(Clone, Debug, Default)]
pub struct FormCsrf(pub Option<String>);

/// 05 "Methods": a form without htmx sends `POST` with `_method=DELETE` (or `PATCH`); this rewrites the method before
/// routing. It also reads `_csrf` from the same body so the CSRF check does not read the body twice. The body is put
/// back unchanged.
pub async fn method_override(req: Request, next: Next) -> Response {
    let is_form = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/x-www-form-urlencoded"));
    if !is_form || req.method() == Method::GET || req.method() == Method::HEAD {
        return next.run(req).await;
    }
    let (mut parts, body) = req.into_parts();
    let bytes = match axum::body::to_bytes(body, MAX_BODY).await {
        Ok(b) => b,
        Err(_) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
    };
    let mut csrf = None;
    for (k, v) in form_pairs(&bytes) {
        match k.as_str() {
            "_csrf" if csrf.is_none() => csrf = Some(v),
            "_method" if parts.method == Method::POST => match v.to_ascii_uppercase().as_str() {
                "DELETE" => parts.method = Method::DELETE,
                "PATCH" => parts.method = Method::PATCH,
                _ => {}
            },
            _ => {}
        }
    }
    parts.extensions.insert(FormCsrf(csrf));
    next.run(Request::from_parts(parts, Body::from(bytes))).await
}

/// Decoded (name, value) pairs of a url-encoded body.
pub fn form_pairs(body: &[u8]) -> Vec<(String, String)> {
    body.split(|b| *b == b'&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut it = p.splitn(2, |b| *b == b'=');
            let k = it.next().unwrap_or_default();
            let v = it.next().unwrap_or_default();
            (form_decode(k), form_decode(v))
        })
        .collect()
}

fn form_decode(s: &[u8]) -> String {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < s.len() => {
                match std::str::from_utf8(&s[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(b) => {
                        out.push(b);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// True for a request made by htmx, which wants the main fragment instead of the full page.
pub fn is_htmx(headers: &HeaderMap) -> bool {
    headers.get("hx-request").is_some_and(|v| v == "true")
}

/// A page template that can also render only its `content` block (askama `blocks = ["content"]`).
pub trait Page: Template {
    fn render_fragment(&self) -> askama::Result<String>;
}

/// Full page, or the `content` block for htmx (05 "Pages and fragments").
pub fn render_page<P: Page>(headers: &HeaderMap, page: &P) -> Result<Html<String>, AppError> {
    Ok(Html(if is_htmx(headers) { page.render_fragment()? } else { page.render()? }))
}

/// Same with a status code (422 form errors).
pub fn render_page_status<P: Page>(status: StatusCode, headers: &HeaderMap, page: &P) -> Response {
    match render_page(headers, page) {
        Ok(html) => (status, html).into_response(),
        Err(e) => e.into_response(),
    }
}

/// A local redirect target: a path on this server, never `//host` or a scheme (open redirect).
pub fn safe_next(next: Option<&str>) -> &str {
    match next {
        Some(n) if n.starts_with('/') && !n.starts_with("//") && !n.contains('\\') && !n.contains("://") => n,
        _ => "/",
    }
}

/// Percent-encodes a query-string value.
pub fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_decoding() {
        let p = form_pairs(b"a=1&_csrf=ab%2Bc&name=Jos%C3%A9+P&x=%zz&y=%4");
        assert_eq!(p[1], ("_csrf".into(), "ab+c".into()));
        assert_eq!(p[2].1, "José P");
        assert_eq!(p[3].1, "%zz");
        assert_eq!(p[4].1, "%4");
    }

    #[test]
    fn next_must_be_local() {
        assert_eq!(safe_next(Some("/account/password")), "/account/password");
        for bad in ["//evil.example", "https://evil.example", "evil", "/\\evil"] {
            assert_eq!(safe_next(Some(bad)), "/", "{bad}");
        }
    }
}
