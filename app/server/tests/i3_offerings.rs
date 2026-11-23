//! Route-level tests of UI-18 Ofrecimientos, UI-19 Fecha del curso, UI-20 Matrícula and UI-21 Matrículas
//! (OP-CRS-01..03, OP-CRS-05, OP-ENR-01..06, OP-ENR-08; T-05-07, T-05-11): status codes, AP checks, autosave 200/422
//! with the out-of-band fragments, the dialog states and the grades export.

#[path = "autosave_support.rs"]
mod support;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use support::{body, location, App, Who};

fn one(app: &App, sql: &str) -> String {
    app.conn().query_row(sql, [], |r| r.get(0)).unwrap()
}

fn student(app: &App, name: &str) -> String {
    one(app, &format!("SELECT uuid FROM students WHERE nombre = '{name}'"))
}

fn course(app: &App, name: &str) -> String {
    one(app, &format!("SELECT uuid FROM courses WHERE course = '{name}'"))
}

/// The FHA calendar row whose first class is `date`.
fn fha_row(app: &App, date: &str) -> String {
    one(
        app,
        &format!(
            "SELECT c.uuid FROM calendar_course c JOIN courses o ON o.uuid = c.course_uuid
              WHERE o.course = 'Suscripción de Préstamos FHA'
                AND (SELECT MIN(date) FROM calendar_course_dates d WHERE d.calendar_course_uuid = c.uuid) = '{date}'"
        ),
    )
}

async fn post(app: &App, path: &str, who: &Who, form: &str) -> axum::http::Response<Body> {
    app.post_form(path, Some(who), &format!("_csrf={}&{form}", who.csrf)).await
}

async fn delete(app: &App, path: &str, who: &Who, extra: &str) -> axum::http::Response<Body> {
    app.post_form(path, Some(who), &format!("_method=DELETE&_csrf={}{extra}", who.csrf)).await
}

async fn hx_get(app: &App, path: &str, who: &Who) -> axum::http::Response<Body> {
    let req = Request::get(path)
        .header(header::COOKIE, &who.cookie)
        .header("hx-request", "true")
        .body(Body::empty())
        .unwrap();
    app.send(req).await
}

/// UI-18: the FHA calendar from `v_calendar_course_summary`, "Se necesitan" clamped at 0 (18 enrolled of 12), the
/// kind switch, unknown kind 404, AP-01, and the admin-only delete (AP-04).
#[tokio::test]
async fn ui18_offerings_page() {
    let app = App::new();
    assert_eq!(app.get("/offerings/course", None).await.status(), StatusCode::SEE_OTHER);
    let staff = app.staff().await;
    let res = app.get("/offerings/course", Some(&staff)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body(res).await;
    assert!(html.contains(
        "<b>Suscripción de Préstamos FHA</b> <span class=\"sub\">· curso especializado · 3 créditos · 45 horas</span>"
    ));
    assert!(html.contains("Calendario<span class=\"tab-count\">5</span>"));
    let row = fha_row(&app, "2026-09-14");
    assert!(html.contains(&format!("id=\"cal-{row}-nec\">0</td>")), "needed clamped at 0");
    assert!(html.contains(&format!("id=\"cal-{row}-mat\">18</td>")));
    let cancelled = fha_row(&app, "2025-10-06");
    assert!(html.contains(&format!("id=\"cal-{cancelled}-nec\">8</td>")));
    assert!(!html.contains("data-open=\"dlg-borrar\""), "staff sees no delete (AP-04)");
    assert!(!html.contains("Borrar fecha"), "no delete on calendar rows (AP-07)");
    let seminars = body(app.get("/offerings/seminar", Some(&staff)).await).await;
    assert!(seminars.contains("· seminario") && seminars.contains("Abrir seminario"));
    let short = body(app.get("/offerings/short-course", Some(&staff)).await).await;
    assert!(short.contains("Educación Continua NMLS (8 h)") && short.contains("NMLS $"), "BR-CRS-04 total");
    assert_eq!(app.get("/offerings/workshop", Some(&staff)).await.status(), StatusCode::NOT_FOUND);
    let admin = app.admin().await;
    assert!(body(app.get("/offerings/course", Some(&admin)).await).await.contains("data-open=\"dlg-borrar\""));
    // The offering picker swaps #main and the pagebar out of band.
    let frag = body(hx_get(&app, &format!("/offerings/course?o={}", course(&app, "Seguros")), &staff).await).await;
    assert!(
        !frag.contains("<html")
            && frag.contains("hx-swap-oob=\"true\" class=\"pagebar\"")
            && frag.contains("<b>Seguros</b>")
    );
}

/// OP-CRS-01: create (422 without a name, 403 without CSRF), Detalles autosave 200/422 with the title out of band,
/// delete: staff 403, with calendar rows 409, a fresh offering 303.
#[tokio::test]
async fn op_crs_01_offering_create_edit_delete() {
    let app = App::new();
    let staff = app.staff().await;
    let res = post(&app, "/offerings/course", &staff, "name=&member_price=1").await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body(res).await.contains("data-open=\"dlg-nuevo\""));
    assert_eq!(app.post_form("/offerings/course", Some(&staff), "name=X").await.status(), StatusCode::FORBIDDEN);
    let res =
        post(&app, "/offerings/course", &staff, "name=Curso+Nuevo+Demo&member_price=300&individual_price=375").await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let new_id = location(&res).trim_start_matches("/offerings/course/").trim_end_matches("?tab=det").to_owned();
    assert_eq!(one(&app, &format!("SELECT CAST(cost_member AS TEXT) FROM courses WHERE uuid = '{new_id}'")), "30000");

    let url = format!("/offerings/course/{new_id}");
    let res = app.patch(&url, &staff, "credits=4", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    let frag = body(res).await;
    assert!(frag.contains("data-saved=\"4\"") && frag.contains("innerHTML:#of-title") && frag.contains("4 créditos"));
    let res = app.patch(&url, &staff, "quorum=abc", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body(res).await.contains("data-unsaved"));
    assert_eq!(app.patch(&url, &staff, "quorum=3", false).await.status(), StatusCode::FORBIDDEN);
    assert_eq!(app.patch(&url, &staff, "uuid=x", true).await.status(), StatusCode::UNPROCESSABLE_ENTITY);

    assert_eq!(delete(&app, &url, &staff, "").await.status(), StatusCode::FORBIDDEN);
    let admin = app.admin().await;
    let fha = course(&app, "Suscripción de Préstamos FHA");
    let res = delete(&app, &format!("/offerings/course/{fha}"), &admin, "").await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert!(body(res).await.contains("5 fechas en el calendario"));
    assert_eq!(delete(&app, &url, &admin, "").await.status(), StatusCode::SEE_OTHER);
    assert_eq!(app.count(&format!("SELECT COUNT(*) FROM courses WHERE uuid = '{new_id}'")), 0);
}

/// OP-CRS-02 "Abrir curso" (BR-CRS-02) and UI-19: the dialog, PATCH 200 with the calendar row out of band, 422, and
/// T-05-07: DELETE /calendar/{kind}/{id} is 405 (GET and PATCH only).
#[tokio::test]
async fn op_crs_02_ui19_calendar_row() {
    let app = App::new();
    let staff = app.staff().await;
    let fha = course(&app, "Suscripción de Préstamos FHA");
    let res = post(&app, &format!("/offerings/course/{fha}/calendar"), &staff, "").await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let cal = location(&res).trim_start_matches("/calendar/course/").to_owned();
    assert_eq!(
        one(&app, &format!("SELECT status || ' ' || cost_member || ' ' || cost_individual FROM calendar_course WHERE uuid = '{cal}'")),
        "Pendiente 35000 42500"
    );
    let html = body(app.get(&format!("/calendar/course/{cal}"), Some(&staff)).await).await;
    assert!(html.contains("data-open=\"dlg-fecha\"") && html.contains("Fecha del curso · sin fecha"));

    let row = fha_row(&app, "2026-09-14");
    let url = format!("/calendar/course/{row}");
    let html = body(app.get(&url, Some(&staff)).await).await;
    assert!(html.contains("18 de quórum 12 · se necesitan 0") && html.contains("aria-selected=\"true\""));
    let res = app.patch(&url, &staff, "status=Cancelado", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    let frag = body(res).await;
    assert!(frag.contains(&format!("innerHTML:#cal-{row}-st")) && frag.contains("tag err\">Cancelado"));
    let res = app.patch(&url, &staff, "cost_member=abc", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body(res).await.contains("value=\"abc\""));

    let admin = app.admin().await;
    for kind in ["course", "seminar", "short-course"] {
        let req = Request::delete(format!("/calendar/{kind}/{row}"))
            .header(header::COOKIE, &admin.cookie)
            .header("x-csrf-token", &admin.csrf)
            .body(Body::empty())
            .unwrap();
        assert_eq!(app.send(req).await.status(), StatusCode::METHOD_NOT_ALLOWED, "{kind}");
    }
    assert_eq!(delete(&app, &url, &admin, "").await.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(app.count(&format!("SELECT COUNT(*) FROM calendar_course WHERE uuid = '{row}'")), 1);
}

/// OP-CRS-03: add (with the day type and note), bad date 422, remove; short courses stop at 15 (409).
#[tokio::test]
async fn op_crs_03_class_dates() {
    let app = App::new();
    let staff = app.staff().await;
    let row = fha_row(&app, "2026-09-14");
    let before = app.count(&format!("SELECT COUNT(*) FROM calendar_course_dates WHERE calendar_course_uuid = '{row}'"));
    let res = post(
        &app,
        &format!("/calendar/course/{row}/dates"),
        &staff,
        "date=10%2F12%2F2026&status=D%C3%ADa+de+Fiesta&note=D%C3%ADa+de+la+Raza&professor=",
    )
    .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(one(&app, &format!("SELECT status || '|' || note FROM calendar_course_dates WHERE calendar_course_uuid = '{row}' AND date = '2026-10-12'")), "Día de Fiesta|Día de la Raza");
    let res =
        post(&app, &format!("/calendar/course/{row}/dates"), &staff, "date=31%2F9%2F2026&status=D%C3%ADa+de+Clase")
            .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body(res).await.contains("Fecha no válida (m/d/aaaa)."));
    let d = one(
        &app,
        &format!("SELECT uuid FROM calendar_course_dates WHERE calendar_course_uuid = '{row}' AND date = '2026-10-12'"),
    );
    assert_eq!(
        delete(&app, &format!("/calendar/course/{row}/dates/{d}"), &staff, "").await.status(),
        StatusCode::SEE_OTHER
    );
    assert_eq!(
        app.count(&format!("SELECT COUNT(*) FROM calendar_course_dates WHERE calendar_course_uuid = '{row}'")),
        before
    );

    let short = one(&app, "SELECT uuid FROM calendar_short_course WHERE status = 'Confirmado' LIMIT 1");
    let have = app.count(&format!(
        "SELECT COUNT(*) FROM calendar_short_course_dates WHERE calendar_short_course_uuid = '{short}'"
    ));
    for i in have..15 {
        let res =
            post(&app, &format!("/calendar/short-course/{short}/dates"), &staff, &format!("date=2027-01-{:02}", i + 1))
                .await;
        assert_eq!(res.status(), StatusCode::SEE_OTHER, "date {i}");
    }
    let res = post(&app, &format!("/calendar/short-course/{short}/dates"), &staff, "date=2027-02-01").await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let html = body(res).await;
    assert!(html.contains("hasta 15 fechas") && html.contains("15 de 15 fechas"));
}

/// T-05-11: `/calendar-options/{kind}` lists only active rows, with the price this student pays.
#[tokio::test]
async fn t05_11_calendar_options() {
    let app = App::new();
    let staff = app.staff().await;
    let fha = course(&app, "Suscripción de Préstamos FHA");
    let row = fha_row(&app, "2026-09-14");
    let elena = student(&app, "Elena Muestra Ficticia");
    let frag = body(
        app.get(&format!("/calendar-options/course?offering={fha}&student={elena}&calendar={row}"), Some(&staff)).await,
    )
    .await;
    assert!(frag.contains("9/14/2026 · Confirmado") && frag.contains("1/18/2027 · Pendiente"));
    assert!(!frag.contains("3/9/2026") && !frag.contains("10/6/2025"), "inactive rows absent");
    assert!(
        frag.contains("hx-swap-oob=\"true\"")
            && frag.contains("$350.00")
            && frag.contains("Afiliado ACTIVO: precio de socio")
    );
    let rafael = student(&app, "Rafael Prueba Muestra");
    let frag = body(
        app.get(&format!("/calendar-options/course?offering={fha}&student={rafael}&calendar={row}"), Some(&staff))
            .await,
    )
    .await;
    assert!(frag.contains("$425.00") && frag.contains("Sin afiliado: precio individual"));
    let tas = course(&app, "Tasación Residencial Básica");
    let frag =
        body(app.get(&format!("/calendar-options/course?offering={tas}&student={elena}"), Some(&staff)).await).await;
    assert!(frag.contains("Sin fechas activas"));
    assert_eq!(app.get("/calendar-options/course?offering=nope", Some(&staff)).await.status(), StatusCode::NOT_FOUND);
}

/// OP-ENR-01 from UI-15: the dialog (price outcome, no-active-dates state), 422 with every message, then a create that
/// stores the individual price, the payment and the grades with the score from the final average (BR-ENR-03).
#[tokio::test]
async fn op_enr_01_enroll_with_settle_form() {
    let app = App::new();
    let staff = app.staff().await;
    let elena = student(&app, "Elena Muestra Ficticia");
    let html = body(app.get(&format!("/students/{elena}/enrollments/course/new"), Some(&staff)).await).await;
    assert!(html.contains("data-open=\"dlg-mat\"") && html.contains("Matricular en curso · Elena Muestra Ficticia"));
    assert!(
        html.contains("Costo · precio de socio")
            && html.contains("$350.00")
            && html.contains("Notas · archivo académico")
    );
    let tas = course(&app, "Tasación Residencial Básica");
    let html =
        body(app.get(&format!("/students/{elena}/enrollments/course/new?offering={tas}"), Some(&staff)).await).await;
    assert!(html.contains("Sin fechas activas") && html.contains("Escoja una fecha para ver el precio."));
    let sem = body(app.get(&format!("/students/{elena}/enrollments/seminar/new"), Some(&staff)).await).await;
    assert!(sem.contains("Matricular en seminario") && !sem.contains("Notas · archivo académico"), "seminar: no Notas");

    let rafael = student(&app, "Rafael Prueba Muestra");
    let fha = course(&app, "Suscripción de Préstamos FHA");
    let row = fha_row(&app, "2027-01-18");
    let url = format!("/students/{rafael}/enrollments/course");
    let res = post(
        &app,
        &url,
        &staff,
        &format!("offering={fha}&calendar=&status=ACTIVO&pago=-50.00&fecha_de_pago=31%2F9%2F2026&promedio_final=104"),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = body(res).await;
    assert!(html.contains("Revise 4 campos antes de guardar."));
    assert!(
        html.contains("Escoja una fecha activa.")
            && html.contains("El pago no puede ser negativo.")
            && html.contains("Fecha no válida (m/d/aaaa).")
    );
    assert!(html.contains("value=\"-50.00\""), "typed values kept");
    let completed = fha_row(&app, "2026-03-09");
    let res = post(&app, &url, &staff, &format!("offering={fha}&calendar={completed}&status=ACTIVO")).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY, "inactive row refused (BR-CRS-01)");

    let before = app.count("SELECT COUNT(*) FROM enrollments_course");
    let res = post(&app, &url, &staff, &format!("offering={fha}&calendar={row}&status=ACTIVO&costeado_por=INDIVIDUAL&pago=100&fecha_de_pago=1%2F5%2F2027&examen=90&promedio_final=88.6&expectations=Repasar&next=%2Fenrollments%2Fcourse")).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&res), "/enrollments/course");
    assert_eq!(app.count("SELECT COUNT(*) FROM enrollments_course"), before + 1);
    assert_eq!(
        one(&app, &format!("SELECT costo || '|' || pago || '|' || fecha_de_pago || '|' || puntuacion || '|' || examen FROM enrollments_course WHERE student_uuid = '{rafael}' AND course_calendar_uuid = '{row}'")),
        "42500|10000|2027-01-05|89|90"
    );
    assert_eq!(
        app.post_form(&url, Some(&staff), &format!("offering={fha}&calendar={row}")).await.status(),
        StatusCode::FORBIDDEN
    );
}

/// UI-20 opened from a calendar row: the student pick; 422 without a student; 303 back to the row's list.
#[tokio::test]
async fn ui20_enroll_from_calendar_row() {
    let app = App::new();
    let staff = app.staff().await;
    let row = fha_row(&app, "2027-01-18");
    let html = body(app.get(&format!("/enrollments/course/new?calendar={row}"), Some(&staff)).await).await;
    assert!(
        html.contains("id=\"mt-student\"") && html.contains("data-open=\"dlg-mat\"") && html.contains("Matrículas</b>")
    );
    let fha = course(&app, "Suscripción de Préstamos FHA");
    let res =
        post(&app, "/enrollments/course", &staff, &format!("offering={fha}&calendar={row}&student=&status=ACTIVO"))
            .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body(res).await.contains("Escoja un estudiante."));
    let gloria = student(&app, "Gloria Prueba Ficticia");
    let res = post(
        &app,
        "/enrollments/course",
        &staff,
        &format!("offering={fha}&calendar={row}&student={gloria}&status=ACTIVO"),
    )
    .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&res), format!("/calendar/course/{row}/enrollments?hecho=matriculado"));
    assert_eq!(one(&app, &format!("SELECT costo || costeado_por FROM enrollments_course WHERE student_uuid = '{gloria}' AND course_calendar_uuid = '{row}'")), "35000COMPAÑIA");
}

/// OP-ENR-04 / OP-ENR-08 (Guardar) and the per-field PATCH: grades, BR-ENR-03, 422 on a score out of range.
#[tokio::test]
async fn op_enr_04_08_save_enrollment() {
    let app = App::new();
    let staff = app.staff().await;
    let e = one(&app, "SELECT uuid FROM enrollments_course WHERE serial = 10490");
    let url = format!("/enrollments/course/{e}");
    let html = body(app.get(&url, Some(&staff)).await).await;
    assert!(
        html.contains("Matrícula · Suscripción de Préstamos FHA")
            && html.contains("Guardar")
            && html.contains("Pedro Prueba Demo")
    );
    let res = post(&app, &url, &staff, "status=ACTIVO&costeado_por=COMPA%C3%91IA&pago=350&fecha_de_pago=9%2F2%2F2026&examen=94&promedio_final=91.4&puntuacion=").await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        one(&app, &format!("SELECT pago || '|' || puntuacion FROM enrollments_course WHERE uuid = '{e}'")),
        "35000|91"
    );
    let res = post(&app, &url, &staff, "status=ACTIVO&pago=1&puntuacion=104").await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body(res).await.contains("La puntuación va de 0 a 100."));
    assert_eq!(
        one(&app, &format!("SELECT CAST(pago AS TEXT) FROM enrollments_course WHERE uuid = '{e}'")),
        "35000",
        "nothing written"
    );

    let res = app.patch(&url, &staff, "promedio_final=79.6", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body(res).await.contains("hx-swap-oob=\"innerHTML:#en-score-"));
    assert_eq!(one(&app, &format!("SELECT CAST(puntuacion AS TEXT) FROM enrollments_course WHERE uuid = '{e}'")), "80");
    assert_eq!(app.patch(&url, &staff, "puntuacion=101", true).await.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        app.patch(&url, &staff, "costo=1", true).await.status(),
        StatusCode::UNPROCESSABLE_ENTITY,
        "price not editable"
    );
}

/// OP-ENR-05 (BR-ENR-08, AP-05): with a payment the dialog shows Dar de baja and a disabled Borrar, delete is 409 and
/// cancel gives BAJA; without a payment staff may remove it (AP-06): confirm first, then the row goes.
#[tokio::test]
async fn op_enr_05_cancel_and_delete() {
    let app = App::new();
    let staff = app.staff().await;
    let paid = one(&app, "SELECT uuid FROM enrollments_course WHERE serial = 10482");
    let url = format!("/enrollments/course/{paid}");
    let html = body(app.get(&url, Some(&staff)).await).await;
    assert!(html.contains("Tiene un pago: use Dar de baja (BR-ENR-08)") && html.contains("form=\"f-baja\""));
    let admin = app.admin().await;
    let res = delete(&app, &url, &admin, "&confirm=1").await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(app.count(&format!("SELECT COUNT(*) FROM enrollments_course WHERE uuid = '{paid}'")), 1);
    let res = post(&app, &format!("{url}/cancel"), &staff, "").await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(one(&app, &format!("SELECT status FROM enrollments_course WHERE uuid = '{paid}'")), "BAJA");

    let free = one(&app, "SELECT uuid FROM enrollments_course WHERE serial = 10490");
    let url = format!("/enrollments/course/{free}");
    // AP-06: no payment, so ROLE-STAFF sees Borrar and removes it after the confirm.
    let html = body(app.get(&url, Some(&staff)).await).await;
    assert!(html.contains("data-open=\"dlg-mat-borrar\"") && html.contains("¿Quitar la matrícula?"));
    let res = delete(&app, &url, &staff, "").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body(res).await.contains("data-open=\"dlg-mat-borrar\""), "confirm first");
    let res = delete(&app, &url, &staff, "&confirm=1").await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(app.count(&format!("SELECT COUNT(*) FROM enrollments_course WHERE uuid = '{free}'")), 0);
}

/// UI-21 / OP-CRS-05: the row's enrollments with grades and totals, status and name filters, the htmx swap with the
/// pagebar out of band, the empty state, and OP-ENR-06: the XLSX export.
#[tokio::test]
async fn ui21_enrollment_list_and_export() {
    let app = App::new();
    let staff = app.staff().await;
    let row = fha_row(&app, "2026-09-14");
    let res = app.get(&format!("/calendar/course/{row}/enrollments"), Some(&staff)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body(res).await;
    assert!(html.contains("<b>Matrículas</b> <span class=\"sub\">· Suscripción de Préstamos FHA · 9/14/2026</span>"));
    assert!(
        html.contains("18 matrículas · 17 ACTIVO · 1 BAJA") && html.contains("F-26114") && html.contains(">92</td>")
    );
    assert!(html.contains("Exportar notas (XLSX)") && html.contains("href=\"/grades/import\""));
    let html = body(app.get(&format!("/enrollments/course?calendar={row}&status=BAJA"), Some(&staff)).await).await;
    assert!(
        html.contains("Héctor Muestra Demo")
            && !html.contains("Elena Muestra Ficticia")
            && html.contains("1 matrícula · 1 BAJA")
    );
    let html = body(app.get(&format!("/enrollments/course?calendar={row}&q=26114"), Some(&staff)).await).await;
    assert!(html.contains("Elena Muestra Ficticia") && html.contains("1 matrícula"));
    let html = body(app.get(&format!("/enrollments/course?calendar={row}&q=zzz"), Some(&staff)).await).await;
    assert!(html.contains("Sin resultados"));
    let frag = body(hx_get(&app, &format!("/enrollments/course?calendar={row}"), &staff).await).await;
    assert!(!frag.contains("<html") && frag.contains("hx-swap-oob=\"true\" class=\"pagebar\""));
    let fha = course(&app, "Suscripción de Préstamos FHA");
    let res = post(&app, &format!("/offerings/course/{fha}/calendar"), &staff, "").await;
    let fresh = location(&res).trim_start_matches("/calendar/course/").to_owned();
    let html = body(app.get(&format!("/calendar/course/{fresh}/enrollments"), Some(&staff)).await).await;
    assert!(html.contains("Sin matrículas"));
    assert!(body(app.get("/enrollments/seminar", Some(&staff)).await).await.contains("Seminarios"));
    assert_eq!(app.get("/calendar/course/nope/enrollments", Some(&staff)).await.status(), StatusCode::NOT_FOUND);

    let res = app.get(&format!("/exports/grades.xlsx?calendar={row}"), Some(&staff)).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(res.headers()[header::CONTENT_TYPE].to_str().unwrap().contains("spreadsheetml"));
    assert!(res.headers()[header::CONTENT_DISPOSITION].to_str().unwrap().contains("notas-2026-09-14.xlsx"));
    let bytes = http_body_util::BodyExt::collect(res.into_body()).await.unwrap().to_bytes();
    assert!(bytes.starts_with(b"PK"), "an XLSX (zip) file");
}
