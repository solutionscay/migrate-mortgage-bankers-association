//! UI-30 template builder (OP-SYS-02, D-18): sanitizing to the toolbar tags, the merged preview, unknown tokens,
//! the preview writes nothing and needs CSRF, staff 403, and the body autosave writes one audit row.

#[path = "autosave_support.rs"]
mod support;

use axum::body::Body;
use axum::http::{header, Request, Response, StatusCode};
use mba_server::seed;
use support::{body, App, Who};

const WELCOME: &str = "/admin/templates/mbs.student_welcome_letter";

fn enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// The editor's preview request: form body, CSRF in `X-CSRF-Token` when `csrf` is true.
async fn preview(app: &App, code: &str, who: &Who, body_html: &str, record: &str, csrf: bool) -> Response<Body> {
    let mut b = Request::post(format!("/admin/templates/{code}/preview"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, &who.cookie);
    if csrf {
        b = b.header("x-csrf-token", &who.csrf);
    }
    let form = format!("body_html={}&record={}", enc(body_html), enc(record));
    app.send(b.body(Body::from(form)).unwrap()).await
}

fn stored_body(app: &App, code: &str) -> String {
    app.conn().query_row("SELECT body_html FROM document_templates WHERE code = ?1", [code], |r| r.get(0)).unwrap()
}

/// Elena Muestra Ficticia in Suscripción de Préstamos FHA (first class 2026-09-14): `enr:uuid` and the price.
fn elena_fha(app: &App) -> (String, i64) {
    app.conn()
        .query_row(
            "SELECT l.uuid, l.costo FROM v_enrollments_course_lines l JOIN students s ON s.uuid = l.student_uuid
              WHERE s.nombre = 'Elena Muestra Ficticia' AND l.course = 'Suscripción de Préstamos FHA'
                AND l.course_date = '2026-09-14'",
            [],
            |r| Ok((format!("enr:{}", r.get::<_, String>(0)?), r.get(1)?)),
        )
        .unwrap()
}

#[tokio::test]
async fn body_keeps_only_toolbar_tags_and_tokens_with_one_audit_row() {
    let app = App::new();
    let admin = app.admin().await;
    let before = app.count("SELECT COUNT(*) FROM audit_log WHERE action = 'field.change'");
    let dirty = "<h2 style=\"color:red\">Aviso</h2><p class=\"x\" onclick=\"y()\">Hola <b>{{nombre}}</b>, \
                 <i>curso</i> <u>sub</u> <strong>fuerte</strong> <a href=\"/otra\">enlace</a>\
                 <img src=\"x\" onerror=\"z()\"></p><ul><li>uno</li></ul><table><tr><td>t</td></tr></table>\
                 <script>alert(1)</script><style>p{}</style>";
    let res = app.patch(WELCOME, &admin, &format!("body_html={}", enc(dirty)), true).await;
    assert_eq!(res.status(), StatusCode::OK);
    let stored = stored_body(&app, "mbs.student_welcome_letter");
    assert!(
        stored.starts_with(
            "<h2>Aviso</h2><p>Hola <b>{{nombre}}</b>, <i>curso</i> sub fuerte enlace</p><ul><li>uno</li></ul>"
        ),
        "{stored}"
    );
    for gone in ["<u>", "<strong>", "<a", "<img", "<table", "<td", "alert", "style", "class=", "onclick", "onerror"] {
        assert!(!stored.contains(gone), "{gone} in {stored}");
    }
    let after = app.count("SELECT COUNT(*) FROM audit_log WHERE action = 'field.change'");
    assert_eq!(after - before, 1, "one audit row per body save");
    // The same body again writes nothing; an empty body is refused.
    app.patch(WELCOME, &admin, &format!("body_html={}", enc(&stored)), true).await;
    let res = app.patch(WELCOME, &admin, "body_html=%3Cp%3E%3Cbr%3E%3C%2Fp%3E", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(app.count("SELECT COUNT(*) FROM audit_log WHERE action = 'field.change'"), after);
}

#[tokio::test]
async fn preview_merges_a_seeded_record_with_formatted_date_and_money() {
    let app = App::new();
    let admin = app.admin().await;
    let (record, costo) = elena_fha(&app);
    let res = preview(
        &app,
        "mbs.student_welcome_letter",
        &admin,
        "<p>{{nombre}} · {{fecha}} · {{costo}} · {{curso}}</p>",
        &record,
        true,
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body(res).await;
    let money = seed::usd(costo);
    assert!(
        html.contains(&format!(
            "Elena Muestra Ficticia · 14 de septiembre de 2026 · {money} · Suscripción de Préstamos FHA"
        )),
        "{html}"
    );
    // Letterhead and signer come from settings; the signer shows at the bottom when the body has none.
    assert!(html.contains("Mortgage Bankers Association") && html.contains("Dra. Laura Ejemplo Ruiz"), "{html}");

    // Convention: registration #1283 of María Ejemplo Demo, totals in dollars.
    let reg: String = app
        .conn()
        .query_row("SELECT uuid FROM registration WHERE registration_number = 1283", [], |r| r.get(0))
        .unwrap();
    let html = body(
        preview(
            &app,
            "mba.confirmation_letter",
            &admin,
            "<p>{{nombre}} #{{registro}} {{total}} {{balance}} {{fecha}}</p>",
            &format!("reg:{reg}"),
            true,
        )
        .await,
    )
    .await;
    assert!(html.contains("María Ejemplo Demo #1283 $1,218.00 $718.00 6 de mayo de 2027"), "{html}");

    // The page opens on these records by default and shows no technical text.
    let page = body(app.get(WELCOME, Some(&admin)).await).await;
    assert!(page.contains(&format!("value=\"{record}\" selected")), "default record");
    assert!(page.contains("Elena Muestra Ficticia") && page.contains("Nombre del estudiante"));
    for tech in ["HTML simple", "HTML limpio", "Origen", "objects/", ">mbs.student_welcome_letter<"] {
        assert!(!page.contains(tech), "{tech} on the page");
    }
    let page = body(app.get("/admin/templates/mba.confirmation_letter", Some(&admin)).await).await;
    assert!(page.contains(&format!("value=\"reg:{reg}\" selected")), "#1283 by default");
}

#[tokio::test]
async fn unknown_and_missing_tokens_show_an_empty_placeholder() {
    let app = App::new();
    let admin = app.admin().await;
    let (record, _) = elena_fha(&app);
    // `auspiciador` is a convention field: unknown in a school letter. A record of another module fills nothing.
    let html = body(
        preview(
            &app,
            "mbs.student_welcome_letter",
            &admin,
            "<p>A {{campo_viejo}} B {{auspiciador}} C</p>",
            &record,
            true,
        )
        .await,
    )
    .await;
    assert!(!html.contains("{{"), "{html}");
    assert_eq!(html.matches("class=\"mf-empty\"").count(), 2, "{html}");
    let html =
        body(preview(&app, "mbs.student_welcome_letter", &admin, "<p>{{nombre}}</p>", "reg:nope", true).await).await;
    assert!(html.contains("title=\"Sin dato: Nombre del estudiante\""), "{html}");
}

#[tokio::test]
async fn preview_saves_nothing_needs_csrf_and_is_admin_only() {
    let app = App::new();
    let admin = app.admin().await;
    let staff = app.staff().await;
    let (record, _) = elena_fha(&app);
    let saved = stored_body(&app, "mbs.student_welcome_letter");
    let audits = app.count("SELECT COUNT(*) FROM audit_log");
    let res = preview(&app, "mbs.student_welcome_letter", &admin, "<p>Otro texto {{nombre}}</p>", &record, true).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(stored_body(&app, "mbs.student_welcome_letter"), saved);
    assert_eq!(app.count("SELECT COUNT(*) FROM audit_log"), audits);

    let res = preview(&app, "mbs.student_welcome_letter", &admin, "<p>x</p>", &record, false).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN, "no CSRF token");
    let res = preview(&app, "mbs.student_welcome_letter", &staff, "<p>x</p>", &record, true).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN, "ROLE-STAFF");
    assert_eq!(app.get(WELCOME, Some(&staff)).await.status(), StatusCode::FORBIDDEN);
    let res = preview(&app, "no-such", &admin, "<p>x</p>", "", true).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}
