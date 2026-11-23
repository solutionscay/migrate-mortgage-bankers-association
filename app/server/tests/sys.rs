//! Route tests of the SYS area: T-05-20 (files: AP-03, upload, download), T-05-21 (upload limits).
//! T-05-22 (JOB-02 with a fixed clock) is the unit test in `src/sys/jobs.rs`.

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
    data_dir: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

impl App {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("t.db");
        let mut conn = db::open(&db_path).unwrap();
        seed::run(&mut conn, seed::DEFAULT_SEED).unwrap();
        let mut config = Config::for_db(&db_path);
        config.data_dir = dir.path().join("data");
        let data_dir = config.data_dir.clone();
        App { router: router(AppState::new(conn, config)), db_path, data_dir, _dir: dir }
    }

    fn conn(&self) -> Connection {
        Connection::open(&self.db_path).unwrap()
    }

    fn count(&self, sql: &str) -> i64 {
        self.conn().query_row(sql, [], |r| r.get(0)).unwrap()
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

    /// Signs in through the form; returns the session cookie and this session's CSRF token.
    async fn sign_in(&self, user: &str) -> (String, String) {
        let page = self.get("/login", None).await;
        let login_cookie = set_cookie(&page, "mba_login_csrf").unwrap();
        let csrf = hidden(&body(page).await, "_csrf");
        let req = Request::post("/login")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, login_cookie)
            .body(Body::from(format!("username={user}&password={}&_csrf={csrf}&next=%2F", seed::DEMO_PASSWORD)))
            .unwrap();
        let cookie = set_cookie(&self.send(req).await, "mba_session").expect("signed in");
        let html = body(self.get("/account/password", Some(&cookie)).await).await;
        (cookie, hidden(&html, "_csrf"))
    }

    /// `POST /files` as htmx would send it: multipart, CSRF in the header.
    async fn upload(&self, cookie: Option<&str>, csrf: Option<&str>, filename: &str, bytes: &[u8]) -> Response<Body> {
        let boundary = "XbOuNdArYx";
        let mut b = Vec::new();
        for (k, v) in [("name", "pdf_file_uuid"), ("label", "PDF")] {
            b.extend_from_slice(
                format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n").as_bytes(),
            );
        }
        b.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        b.extend_from_slice(bytes);
        b.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let mut r = Request::post("/files")
            .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
            .header("hx-request", "true");
        if let Some(c) = cookie {
            r = r.header(header::COOKIE, c);
        }
        if let Some(t) = csrf {
            r = r.header("x-csrf-token", t);
        }
        self.send(r.body(Body::from(b)).unwrap()).await
    }
}

fn set_cookie(res: &Response<Body>, name: &str) -> Option<String> {
    res.headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with(&format!("{name}=")))
        .map(|v| v.split(';').next().unwrap().to_owned())
        .filter(|c| !c.ends_with('='))
}

async fn body(res: Response<Body>) -> String {
    String::from_utf8_lossy(&res.into_body().collect().await.unwrap().to_bytes()).into_owned()
}

fn hidden(html: &str, name: &str) -> String {
    let marker = format!("name=\"{name}\" value=\"");
    let start = html.find(&marker).unwrap_or_else(|| panic!("no hidden {name} in {html}")) + marker.len();
    html[start..start + html[start..].find('"').unwrap()].to_owned()
}

fn header_of(res: &Response<Body>, h: header::HeaderName) -> String {
    res.headers().get(h).map(|v| v.to_str().unwrap().to_owned()).unwrap_or_default()
}

const PDF: &[u8] = b"%PDF-1.4\n1 0 obj << >> endobj\ntrailer << >>\n%%EOF\n";

/// T-05-20: AP-01/AP-02/AP-03 on `/files`, then upload -> chip, row, bytes on disk, audit row; download headers.
#[tokio::test]
async fn upload_and_download() {
    let app = App::new();

    // AP-01: no session -> 401 + HX-Redirect for htmx, 303 for a page; nothing stored.
    let res = app.upload(None, None, "programa.pdf", PDF).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(app.get("/files/abc", None).await.status(), StatusCode::SEE_OTHER);

    // AP-02: signed in, no CSRF token -> 403.
    let (cookie, csrf) = app.sign_in("equintana").await;
    assert_eq!(app.upload(Some(&cookie), None, "programa.pdf", PDF).await.status(), StatusCode::FORBIDDEN);
    assert_eq!(app.count("SELECT COUNT(*) FROM stored_files"), 0);

    // AP-03: ROLE-STAFF uploads.
    let res = app.upload(Some(&cookie), Some(&csrf), "programa convención 2027.pdf", PDF).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body(res).await;
    assert!(html.contains("class=\"chip\""), "{html}");
    assert!(html.contains("programa convención 2027.pdf"), "{html}");
    let uuid = hidden(&html, "pdf_file_uuid");
    let (mime, size, sha, path, by): (String, i64, String, String, String) = app
        .conn()
        .query_row(
            "SELECT mime_type, size_bytes, sha256, storage_path, created_by FROM stored_files WHERE uuid = ?1",
            [&uuid],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!((mime.as_str(), size, by.as_str()), ("application/pdf", PDF.len() as i64, "equintana"));
    assert_eq!(path, format!("files/{}/{}", &sha[..2], sha));
    assert_eq!(std::fs::read(app.data_dir.join(&path)).unwrap(), PDF);
    assert_eq!(app.count("SELECT COUNT(*) FROM audit_log WHERE action = 'file.upload'"), 1);

    // Download: stored type, inline for PDF.
    let res = app.get(&format!("/files/{uuid}"), Some(&cookie)).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(header_of(&res, header::CONTENT_TYPE), "application/pdf");
    let cd = header_of(&res, header::CONTENT_DISPOSITION);
    assert!(cd.starts_with("inline; filename=\""), "{cd}");
    assert!(cd.contains("filename*=UTF-8''programa%20convenci%C3%B3n%202027.pdf"), "{cd}");
    assert_eq!(&res.into_body().collect().await.unwrap().to_bytes()[..], PDF);

    // A spreadsheet downloads as an attachment; same bytes uploaded twice are stored once on disk.
    let (admin, admin_csrf) = app.sign_in("mvega").await;
    let html = body(app.upload(Some(&admin), Some(&admin_csrf), "notas.xlsx", b"PK\x03\x04demo").await).await;
    let x = hidden(&html, "pdf_file_uuid");
    let res = app.get(&format!("/files/{x}"), Some(&cookie)).await;
    assert!(header_of(&res, header::CONTENT_DISPOSITION).starts_with("attachment;"));
    assert_eq!(
        header_of(&res, header::CONTENT_TYPE),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    );
    let html = body(app.upload(Some(&cookie), Some(&csrf), "copia.pdf", PDF).await).await;
    assert_ne!(hidden(&html, "pdf_file_uuid"), uuid);
    assert_eq!(app.count("SELECT COUNT(DISTINCT storage_path) FROM stored_files"), 2);
    assert_eq!(app.count("SELECT COUNT(*) FROM stored_files"), 3);

    // Unknown uuid -> 404 page.
    assert_eq!(app.get("/files/no-such-file", Some(&cookie)).await.status(), StatusCode::NOT_FOUND);
    // The empty widget the `x` swaps back in.
    let html = body(app.get("/files/field?name=logo_file_uuid&label=logo", Some(&cookie)).await).await;
    assert!(html.contains("Escoger archivo") && html.contains("hx-post=\"/files\""), "{html}");
}

/// T-05-21: too large, empty, or a refused type -> 422 with the field error; nothing stored.
#[tokio::test]
async fn upload_limits() {
    let app = App::new();
    let (cookie, csrf) = app.sign_in("equintana").await;
    let big = vec![b'a'; 20 * 1024 * 1024 + 1];
    for (name, bytes, msg) in [
        ("grande.pdf", &big[..], "20 MB"),
        ("vacio.pdf", &b""[..], "vacío"),
        ("logo.svg", &b"<svg onload=alert(1)>"[..], "Tipo de archivo no permitido"),
        ("pagina.html", &b"<script>"[..], "Tipo de archivo no permitido"),
    ] {
        let res = app.upload(Some(&cookie), Some(&csrf), name, bytes).await;
        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY, "{name}");
        let html = body(res).await;
        assert!(html.contains("field-error") && html.contains(msg), "{name}: {html}");
    }
    // Exactly 20 MB is accepted.
    let ok = vec![b'a'; 20 * 1024 * 1024];
    assert_eq!(app.upload(Some(&cookie), Some(&csrf), "limite.txt", &ok).await.status(), StatusCode::OK);
    assert_eq!(app.count("SELECT COUNT(*) FROM stored_files"), 1);
}
