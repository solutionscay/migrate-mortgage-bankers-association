//! Shared harness of `tests/autosave.rs` and `tests/admin.rs`: a seeded app on a temp file, sign-in, and request
//! helpers. Compiled on its own too (no tests inside).
#![allow(dead_code, clippy::new_without_default)]

use axum::body::Body;
use axum::http::{header, Request, Response};
use axum::Router;
use http_body_util::BodyExt;
use mba_server::{config::Config, db, router, seed, AppState};
use rusqlite::Connection;
use tower::ServiceExt;

pub struct App {
    pub router: Router,
    pub db_path: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

/// A signed-in browser: its session cookie and the CSRF token of its pages.
pub struct Who {
    pub cookie: String,
    pub csrf: String,
}

impl App {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("t.db");
        let mut conn = db::open(&db_path).unwrap();
        seed::run(&mut conn, seed::DEFAULT_SEED).unwrap();
        let state = AppState::new(conn, Config::for_db(&db_path));
        App { router: router(state), db_path, _dir: dir }
    }

    pub fn conn(&self) -> Connection {
        Connection::open(&self.db_path).unwrap()
    }

    pub async fn send(&self, req: Request<Body>) -> Response<Body> {
        self.router.clone().oneshot(req).await.unwrap()
    }

    pub async fn get(&self, path: &str, who: Option<&Who>) -> Response<Body> {
        let mut b = Request::get(path);
        if let Some(w) = who {
            b = b.header(header::COOKIE, &w.cookie);
        }
        self.send(b.body(Body::empty()).unwrap()).await
    }

    /// A plain form post (CSRF in the body, as the page forms send it).
    pub async fn post_form(&self, path: &str, who: Option<&Who>, body: &str) -> Response<Body> {
        let mut b = Request::post(path).header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
        if let Some(w) = who {
            b = b.header(header::COOKIE, &w.cookie);
        }
        self.send(b.body(Body::from(body.to_owned())).unwrap()).await
    }

    /// An htmx autosave PATCH: CSRF in `X-CSRF-Token` (unless `csrf` is false), body = the given fields.
    pub async fn patch(&self, path: &str, who: &Who, body: &str, csrf: bool) -> Response<Body> {
        let mut b = Request::patch(path)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, &who.cookie)
            .header("hx-request", "true");
        if csrf {
            b = b.header("x-csrf-token", &who.csrf);
        }
        self.send(b.body(Body::from(body.to_owned())).unwrap()).await
    }

    pub async fn sign_in(&self, user: &str, password: &str) -> Option<Who> {
        let page = self.get("/login", None).await;
        let login_cookie = set_cookie(&page, "mba_login_csrf").unwrap();
        let html = body(page).await;
        let csrf = hidden(&html, "_csrf");
        let req = Request::post("/login")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, &login_cookie)
            .body(Body::from(format!("username={user}&password={password}&_csrf={csrf}&next=%2F")))
            .unwrap();
        let res = self.send(req).await;
        let cookie = set_cookie(&res, "mba_session").filter(|c| c != "mba_session=")?;
        let html =
            body(self.get("/account/password", Some(&Who { cookie: cookie.clone(), csrf: String::new() })).await).await;
        Some(Who { csrf: hidden(&html, "_csrf"), cookie })
    }

    pub async fn admin(&self) -> Who {
        self.sign_in("mvega", seed::DEMO_PASSWORD).await.expect("admin signs in")
    }

    pub async fn staff(&self) -> Who {
        self.sign_in("equintana", seed::DEMO_PASSWORD).await.expect("staff signs in")
    }

    pub fn user_id(&self, username: &str) -> i64 {
        self.conn().query_row("SELECT id FROM users WHERE username = ?1", [username], |r| r.get(0)).unwrap()
    }

    pub fn count(&self, sql: &str) -> i64 {
        self.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }
}

pub fn set_cookie(res: &Response<Body>, name: &str) -> Option<String> {
    res.headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with(&format!("{name}=")))
        .map(|v| v.split(';').next().unwrap().to_owned())
}

pub async fn body(res: Response<Body>) -> String {
    String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap()
}

pub fn hidden(html: &str, name: &str) -> String {
    let marker = format!("name=\"{name}\" value=\"");
    let start = html.find(&marker).unwrap_or_else(|| panic!("no hidden {name}")) + marker.len();
    html[start..start + html[start..].find('"').unwrap()].to_owned()
}

pub fn location(res: &Response<Body>) -> String {
    res.headers().get(header::LOCATION).map(|v| v.to_str().unwrap().to_owned()).unwrap_or_default()
}
