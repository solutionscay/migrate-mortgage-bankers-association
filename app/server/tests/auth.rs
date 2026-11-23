//! Route tests of I1: T-05-01, T-05-02, T-05-05, T-05-17 (no secret in a body), T-05-19 (htmx fragment);
//! T-08-01..04, T-08-12, T-08-15..17, T-08-19, T-08-20.

use axum::body::Body;
use axum::http::{header, Request, Response, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use mba_server::{config::Config, db, router, seed, AppState};
use rusqlite::Connection;
use tower::ServiceExt;

struct App {
    router: Router,
    db_path: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

impl App {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("t.db");
        let mut conn = db::open(&db_path).unwrap();
        seed::run(&mut conn, seed::DEFAULT_SEED).unwrap();
        let state = AppState::new(conn, Config::for_db(&db_path));
        App { router: router(state), db_path, _dir: dir }
    }

    /// A second connection to the same file (WAL), to look at or change rows behind the server.
    fn conn(&self) -> Connection {
        Connection::open(&self.db_path).unwrap()
    }

    async fn send(&self, req: Request<Body>) -> Response<Body> {
        self.router.clone().oneshot(req).await.unwrap()
    }

    async fn get(&self, path: &str, cookie: Option<&str>) -> Response<Body> {
        let mut b = Request::get(path);
        if let Some(c) = cookie {
            b = b.header(header::COOKIE, c);
        }
        self.send(b.body(Body::empty()).unwrap()).await
    }

    async fn post_form(&self, path: &str, cookie: Option<&str>, body: &str) -> Response<Body> {
        let mut b = Request::post(path).header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
        if let Some(c) = cookie {
            b = b.header(header::COOKIE, c);
        }
        self.send(b.body(Body::from(body.to_owned())).unwrap()).await
    }

    /// Signs in through the form. Returns the session cookie pair `mba_session=...` (None on failure) and the response.
    async fn sign_in(&self, user: &str, password: &str) -> (Option<String>, Response<Body>) {
        let page = self.get("/login", None).await;
        let login_cookie = set_cookie(&page, "mba_login_csrf").unwrap();
        let html = body(page).await;
        let csrf = hidden(&html, "_csrf");
        let res = self
            .post_form(
                "/login",
                Some(&login_cookie),
                &format!("username={user}&password={password}&_csrf={csrf}&next=%2F"),
            )
            .await;
        (set_cookie(&res, "mba_session").filter(|c| c != "mba_session="), res)
    }

    async fn admin(&self) -> String {
        self.sign_in("mvega", seed::DEMO_PASSWORD).await.0.expect("admin signs in")
    }

    async fn staff(&self) -> String {
        self.sign_in("equintana", seed::DEMO_PASSWORD).await.0.expect("staff signs in")
    }

    /// The CSRF token rendered into the signed-in user's pages.
    async fn csrf(&self, cookie: &str) -> String {
        let html = body(self.get("/account/password", Some(cookie)).await).await;
        hidden(&html, "_csrf")
    }
}

fn set_cookie(res: &Response<Body>, name: &str) -> Option<String> {
    res.headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with(&format!("{name}=")))
        .map(|v| v.split(';').next().unwrap().to_owned())
}

async fn body(res: Response<Body>) -> String {
    String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap()
}

fn hidden(html: &str, name: &str) -> String {
    let marker = format!("name=\"{name}\" value=\"");
    let start = html.find(&marker).unwrap_or_else(|| panic!("no hidden {name} in {html}")) + marker.len();
    html[start..start + html[start..].find('"').unwrap()].to_owned()
}

fn location(res: &Response<Body>) -> String {
    res.headers().get(header::LOCATION).map(|v| v.to_str().unwrap().to_owned()).unwrap_or_default()
}

// --- AP-01 ------------------------------------------------------------------------------------------------------

/// T-05-01 / T-08-01: no session -> page 303 to /login, htmx 401 with HX-Redirect, POST too.
#[tokio::test]
async fn anonymous_is_sent_to_sign_in() {
    let app = App::new();
    for path in ["/", "/account/password", "/admin/users", "/people", "/print/convention/x"] {
        let res = app.get(path, None).await;
        assert_eq!(res.status(), StatusCode::SEE_OTHER, "{path}");
        assert!(location(&res).starts_with("/login"), "{path}");
    }
    assert_eq!(location(&app.get("/account/password", None).await), "/login?next=/account/password");

    let res = app.send(Request::get("/").header("hx-request", "true").body(Body::empty()).unwrap()).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(res.headers()["hx-redirect"], "/login");

    let res = app.post_form("/logout", None, "_csrf=x").await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    // Public: the sign-in page and the assets.
    assert_eq!(app.get("/login", None).await.status(), StatusCode::OK);
    assert_eq!(app.get("/assets/app.css", None).await.status(), StatusCode::OK);
    assert_eq!(app.get("/assets/vendor/htmx-2.0.4.min.js", None).await.status(), StatusCode::OK);
}

/// T-08-02: forged or deleted session token.
#[tokio::test]
async fn forged_or_deleted_session_is_refused() {
    let app = App::new();
    let res = app.get("/", Some("mba_session=deadbeef")).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let cookie = app.admin().await;
    assert_eq!(app.get("/", Some(&cookie)).await.status(), StatusCode::OK);
    app.conn().execute("DELETE FROM sessions", []).unwrap();
    assert_eq!(app.get("/", Some(&cookie)).await.status(), StatusCode::SEE_OTHER);
}

/// T-08-03: idle 11 minutes -> session ended (BR-AUTH-02); the row is gone.
#[tokio::test]
async fn idle_session_ends() {
    let app = App::new();
    let cookie = app.staff().await;
    let eleven_ago = mba_server::clock::in_minutes(-11);
    app.conn().execute("UPDATE sessions SET last_seen_at = ?1", [&eleven_ago]).unwrap();
    let res = app.get("/", Some(&cookie)).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let n: i64 = app.conn().query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
}

/// Session cookie flags (AP-02) and a successful sign-in redirect.
#[tokio::test]
async fn sign_in_sets_a_safe_cookie() {
    let app = App::new();
    let (cookie, res) = app.sign_in("MVEGA", seed::DEMO_PASSWORD).await; // usernames are case-insensitive
    assert!(cookie.is_some());
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&res), "/");
    let raw = res
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_owned())
        .find(|v| v.starts_with("mba_session="))
        .unwrap();
    assert!(raw.contains("HttpOnly") && raw.contains("SameSite=Lax"), "{raw}");
    let stored: String = app.conn().query_row("SELECT token_hash FROM sessions", [], |r| r.get(0)).unwrap();
    assert!(!raw.contains(&stored), "the table holds the hash, not the token");
}

// --- AP-11 ------------------------------------------------------------------------------------------------------

/// BR-AUTH-01, AP-11: generic error; 5 failures lock for 15 minutes; locked = same message.
#[tokio::test]
async fn wrong_password_and_lockout() {
    let app = App::new();
    let (c, res) = app.sign_in("inieves", "wrong-password").await;
    assert!(c.is_none());
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    let wrong = body(res).await;
    let (_, res) = app.sign_in("nobody", "wrong-password").await;
    let unknown = body(res).await;
    assert!(wrong.contains("Usuario o contraseña incorrectos."));
    assert!(unknown.contains("Usuario o contraseña incorrectos."));

    for _ in 0..4 {
        app.sign_in("inieves", "wrong-password").await;
    }
    let locked: Option<String> =
        app.conn().query_row("SELECT locked_until FROM users WHERE username = 'inieves'", [], |r| r.get(0)).unwrap();
    assert!(locked.is_some(), "locked after 5 failures");
    let (c, res) = app.sign_in("inieves", seed::DEMO_PASSWORD).await;
    assert!(c.is_none(), "the right password does not open a locked account");
    assert!(body(res).await.contains("Usuario o contraseña incorrectos."));
    let audit: i64 =
        app.conn().query_row("SELECT COUNT(*) FROM audit_log WHERE action = 'auth.locked'", [], |r| r.get(0)).unwrap();
    assert_eq!(audit, 1);
}

/// T-08-15: a disabled user with the right password gets the generic error and no session.
#[tokio::test]
async fn disabled_user_cannot_sign_in() {
    let app = App::new();
    app.conn().execute("UPDATE users SET active = 0 WHERE username = 'equintana'", []).unwrap();
    let (c, res) = app.sign_in("equintana", seed::DEMO_PASSWORD).await;
    assert!(c.is_none());
    assert!(body(res).await.contains("Usuario o contraseña incorrectos."));
    let n: i64 = app.conn().query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
}

/// Disabling a signed-in user ends the session at the next request.
#[tokio::test]
async fn disabled_user_session_ends() {
    let app = App::new();
    let cookie = app.staff().await;
    app.conn().execute("UPDATE users SET active = 0 WHERE username = 'equintana'", []).unwrap();
    assert_eq!(app.get("/", Some(&cookie)).await.status(), StatusCode::SEE_OTHER);
}

// --- AP-02 ------------------------------------------------------------------------------------------------------

/// T-05-02 / T-08-04: POST without the token, or with another session's token -> 403, nothing changes.
#[tokio::test]
async fn csrf_is_required() {
    let app = App::new();
    let staff = app.staff().await;
    let admin = app.admin().await;
    let admin_token = app.csrf(&admin).await;
    let sessions = || app.conn().query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get::<_, i64>(0)).unwrap();
    assert_eq!(sessions(), 2);

    assert_eq!(app.post_form("/logout", Some(&staff), "").await.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        app.post_form("/logout", Some(&staff), &format!("_csrf={admin_token}")).await.status(),
        StatusCode::FORBIDDEN
    );
    let res = app.send(Request::delete("/anything").header(header::COOKIE, &staff).body(Body::empty()).unwrap()).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN, "DELETE is checked before routing to a handler");
    assert_eq!(sessions(), 2, "nothing written");

    // Login form: without its cookie token -> 403, no session.
    let res = app.post_form("/login", None, &format!("username=mvega&password={}&_csrf=x", seed::DEMO_PASSWORD)).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(sessions(), 2);

    // The right token works: form field, and the htmx header.
    let staff_token = app.csrf(&staff).await;
    let res = app.post_form("/logout", Some(&staff), &format!("_csrf={staff_token}")).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&res), "/login");
    assert_eq!(sessions(), 1);
    let res = app
        .send(
            Request::post("/logout")
                .header(header::COOKIE, &admin)
                .header("x-csrf-token", &admin_token)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(sessions(), 0);
}

/// `_method=DELETE` on a form POST is routed as DELETE (05 "Methods"); the CSRF field is still read.
#[tokio::test]
async fn method_override_keeps_csrf() {
    let app = App::new();
    let staff = app.staff().await;
    let token = app.csrf(&staff).await;
    let res = app.post_form("/admin/users/1", Some(&staff), &format!("_method=DELETE&_csrf={token}")).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN, "staff: admin area");
    let admin = app.admin().await;
    let token = app.csrf(&admin).await;
    let res = app.post_form("/admin/nothing", Some(&admin), &format!("_method=DELETE&_csrf={token}")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND, "admin passes the guard; no such route yet");
}

// --- AP-08 / roles ------------------------------------------------------------------------------------------------

/// T-05-05 / T-08-12: ROLE-STAFF gets 403 (the design's 403 page) on every admin route; ROLE-ADMIN passes.
#[tokio::test]
async fn admin_area_is_admin_only() {
    let app = App::new();
    let staff = app.staff().await;
    let admin = app.admin().await;
    for path in [
        "/admin",
        "/admin/users",
        "/admin/settings",
        "/admin/templates/x",
        "/admin/value-lists",
        "/admin/certifications",
        "/admin/professors",
        "/admin/mail/templates",
        "/admin/mail/profiles",
    ] {
        let res = app.get(path, Some(&staff)).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN, "{path}");
        let html = body(res).await;
        assert!(html.contains("Sin permiso") && html.contains("msgpage"), "{path}: 403 page");
        assert_ne!(app.get(path, Some(&admin)).await.status(), StatusCode::FORBIDDEN, "{path}");
    }
    let token = app.csrf(&staff).await;
    let res = app.post_form("/admin/users", Some(&staff), &format!("_csrf={token}&username=x")).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

/// The shell shows the Admin module and tile to ROLE-ADMIN only (UI, secondary to the guard).
#[tokio::test]
async fn home_shows_role_and_counts() {
    let app = App::new();
    let admin = body(app.get("/", Some(&app.admin().await)).await).await;
    let staff = body(app.get("/", Some(&app.staff().await)).await).await;
    assert!(admin.contains("data-role=\"admin\"") && admin.contains("href=\"/admin/users\""));
    assert!(staff.contains("data-role=\"staff\"") && !staff.contains("href=\"/admin/users\""));
    assert!(admin.contains("eventos activos") && admin.contains("estudiantes activos"));
    assert!(staff.contains("<b>equintana</b>"));
}

// --- AP-12 ------------------------------------------------------------------------------------------------------

/// T-08-17: wrong current password -> 422; a good change sets a new argon2id hash and closes the other sessions.
#[tokio::test]
async fn change_own_password() {
    let app = App::new();
    let one = app.staff().await;
    let two = app.staff().await;
    let token = app.csrf(&one).await;
    let res = app
        .post_form(
            "/account/password",
            Some(&one),
            &format!("_csrf={token}&actual=wrong&nueva=nueva-clave-123&repetir=nueva-clave-123"),
        )
        .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body(res).await.contains("No es correcta."));
    let res = app
        .post_form(
            "/account/password",
            Some(&one),
            &format!("_csrf={token}&actual={}&nueva=corta&repetir=otra", seed::DEMO_PASSWORD),
        )
        .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let res = app
        .post_form(
            "/account/password",
            Some(&one),
            &format!("_csrf={token}&actual={}&nueva=nueva-clave-123&repetir=nueva-clave-123", seed::DEMO_PASSWORD),
        )
        .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(app.get("/", Some(&one)).await.status(), StatusCode::OK, "this session stays");
    assert_eq!(app.get("/", Some(&two)).await.status(), StatusCode::SEE_OTHER, "the other session ends");
    assert!(app.sign_in("equintana", seed::DEMO_PASSWORD).await.0.is_none());
    assert!(app.sign_in("equintana", "nueva-clave-123").await.0.is_some());
    // Another user's password has no route outside /admin (admin-only, T-08-17 second case).
    let res = app.post_form("/admin/users/1/password", Some(&one), &format!("_csrf={token}&password=x")).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

// --- AP-14, field policy, htmx ------------------------------------------------------------------------------------

/// T-08-19: no API, ODBC, XML or seed route over HTTP.
#[tokio::test]
async fn no_external_channel_routes() {
    let app = App::new();
    let admin = app.admin().await;
    for path in ["/api", "/api/users", "/seed", "/mba-seed", "/odbc", "/xml", "/fmi/xml/fmresultset.xml", "/php"] {
        assert_eq!(app.get(path, Some(&admin)).await.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

/// T-05-17 (secrets) / T-08-20: no page carries a password hash, a session token hash or another user's CSRF token.
#[tokio::test]
async fn no_secret_in_any_body() {
    let app = App::new();
    let admin = app.admin().await;
    let staff = app.staff().await;
    let staff_csrf = app.csrf(&staff).await;
    let conn = app.conn();
    let hashes: Vec<String> = conn
        .prepare("SELECT password_hash FROM users")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let tokens: Vec<String> = conn
        .prepare("SELECT token_hash FROM sessions")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for path in ["/", "/account/password", "/admin/users", "/nope", "/login"] {
        let html = body(app.get(path, Some(&admin)).await).await;
        for secret in hashes.iter().chain(tokens.iter()).chain(std::iter::once(&staff_csrf)) {
            assert!(!html.contains(secret.as_str()), "{path} leaks a secret");
        }
        assert!(!html.contains("$argon2"), "{path}");
    }
}

/// T-05-19 (htmx vs full page): the same GET returns the fragment with HX-Request and the full page without.
#[tokio::test]
async fn htmx_gets_the_fragment() {
    let app = App::new();
    let admin = app.admin().await;
    let full = body(app.get("/", Some(&admin)).await).await;
    let res = app
        .send(
            Request::get("/").header(header::COOKIE, &admin).header("hx-request", "true").body(Body::empty()).unwrap(),
        )
        .await;
    let frag = body(res).await;
    assert!(full.contains("<html") && full.contains("class=\"topbar\"") && full.contains("ma-tiles"));
    assert!(!frag.contains("<html") && !frag.contains("class=\"topbar\"") && frag.contains("ma-tiles"));
}
