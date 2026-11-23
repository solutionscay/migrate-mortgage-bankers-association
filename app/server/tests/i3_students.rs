//! Route-level tests of UI-14, UI-15, UI-16 and UI-17 (OP-STU-01..05, OP-AFF-01, EXT-02): status codes, AP-03/AP-04/
//! AP-06 checks, autosave 200/422 with the out-of-band fragments, BR-STU-02 by key, BR-STU-05, BR-CRT-04.

#[path = "autosave_support.rs"]
mod support;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use support::{body, location, App, Who};

fn one(app: &App, sql: &str) -> String {
    app.conn().query_row(sql, [], |r| r.get(0)).unwrap()
}

fn elena(app: &App) -> String {
    one(app, "SELECT uuid FROM students WHERE nombre = 'Elena Muestra Ficticia'")
}

fn affiliate(app: &App, name: &str) -> String {
    one(app, &format!("SELECT uuid FROM affiliates WHERE socio = '{name}'"))
}

async fn delete(app: &App, path: &str, who: &Who) -> axum::http::Response<Body> {
    app.post_form(path, Some(who), &format!("_method=DELETE&_csrf={}", who.csrf)).await
}

async fn hx(app: &App, method: &str, path: &str, who: &Who, form: &str) -> axum::http::Response<Body> {
    let mut b = Request::builder()
        .method(method)
        .uri(path)
        .header(header::COOKIE, &who.cookie)
        .header("x-csrf-token", &who.csrf)
        .header("hx-request", "true");
    if !form.is_empty() {
        b = b.header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    }
    app.send(b.body(Body::from(form.to_owned())).unwrap()).await
}

async fn new_student(app: &App, who: &Who, name: &str) -> String {
    let res = app.post_form("/students", Some(who), &format!("_csrf={}&nombre={name}", who.csrf)).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    location(&res).trim_start_matches("/students/").to_owned()
}

/// UI-14: AP-01, title counts, search, filters, no results, the htmx fragment, OP-STU-05 one hit opens the student.
#[tokio::test]
async fn ui14_students_list_search_and_filters() {
    let app = App::new();
    assert_eq!(app.get("/students", None).await.status(), StatusCode::SEE_OTHER);
    let staff = app.staff().await;
    let res = app.get("/students", Some(&staff)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body(res).await;
    // The active count comes from v_student_academic (BAJA does not count since 0003, D-95).
    let active = app.count("SELECT COUNT(*) FROM v_student_academic WHERE status = 'ACTIVO'");
    assert!(active > 0 && active < 80);
    assert!(html.contains(&format!("<b>Estudiantes</b> <span class=\"sub\">· 80 en total, {active} activos</span>")));
    assert!(html.contains("Elena Muestra Ficticia") && html.contains("1–50 de 80"));
    assert!(!html.contains("Borrar estudiante"), "staff sees no delete (AP-04)");

    let html = body(app.get("/students?q=zzz", Some(&staff)).await).await;
    assert!(html.contains("Sin resultados"));
    // OP-STU-05: one hit opens the student (plain search); many show the list.
    let res = app.get("/students?q=elena.muestra%40ejemplo.com", Some(&staff)).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&res), format!("/students/{}", elena(&app)));
    let html = body(app.get("/students?q=Elena", Some(&staff)).await).await;
    assert!(html.contains("Elena Muestra Ficticia") && html.contains("Elena Ejemplo Modelo"));

    let html = body(app.get("/students?status=INACTIVO&q=Elena", Some(&staff)).await).await;
    assert!(!html.contains(">Elena Muestra Ficticia<"), "Elena is ACTIVO (BR-STU-03)");
    let html = body(app.get("/students?affiliate=none&q=Rafael", Some(&staff)).await).await;
    assert!(html.contains("Rafael Prueba Muestra"));
    let acme = affiliate(&app, "Acme Mortgage Demo");
    let html = body(app.get(&format!("/students?affiliate={acme}"), Some(&staff)).await).await;
    assert!(html.contains("1–8 de 8"));

    let req = Request::get("/students?q=Pedro")
        .header(header::COOKIE, &staff.cookie)
        .header("hx-request", "true")
        .header("hx-target", "stu-results")
        .body(Body::empty())
        .unwrap();
    let frag = body(app.send(req).await).await;
    assert!(frag.starts_with("<div id=\"stu-results\">") && !frag.contains("<html"));

    let admin = app.admin().await;
    assert!(body(app.get("/students", Some(&admin)).await).await.contains("Borrar estudiante"));
}

/// OP-STU-01 "Nuevo estudiante": the dialog, 422 without a name or with an unknown affiliate or a bad e-mail, 403
/// without CSRF; Guardar creates the student with the affiliate by key, the company copied (BR-STU-02) and one row
/// per certification (BR-STU-01).
#[tokio::test]
async fn op_stu_01_create_student() {
    let app = App::new();
    let staff = app.staff().await;
    assert!(body(app.get("/students/new", Some(&staff)).await).await.contains("data-open=\"dlg-nuevo\""));
    let res = app.post_form("/students", Some(&staff), &format!("_csrf={}&nombre=&email=x%40y", staff.csrf)).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = body(res).await;
    assert!(
        html.contains("Revise 2 campos") && html.contains("Escriba el nombre.") && html.contains("nombre@ejemplo.com")
    );
    let res =
        app.post_form("/students", Some(&staff), &format!("_csrf={}&nombre=Ana&afiliado=Nadie+SA", staff.csrf)).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body(res).await.contains("Escoja un afiliado de la lista."));
    assert_eq!(app.post_form("/students", Some(&staff), "nombre=Ana").await.status(), StatusCode::FORBIDDEN);

    let before = app.count("SELECT COUNT(*) FROM students");
    let res = app
        .post_form(
            "/students",
            Some(&staff),
            &format!(
                "_csrf={}&nombre=Ana+Nueva+Demo&afiliado=Cooperativa+Muestra+%28A-1003%29&email=ana%40ejemplo.com&telefono=787-555-0190",
                staff.csrf
            ),
        )
        .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let id = location(&res).trim_start_matches("/students/").to_owned();
    assert_eq!(app.count("SELECT COUNT(*) FROM students"), before + 1);
    assert_eq!(one(&app, &format!("SELECT compania FROM students WHERE uuid = '{id}'")), "Cooperativa Muestra");
    assert_eq!(
        one(&app, &format!("SELECT member_uuid FROM students WHERE uuid = '{id}'")),
        affiliate(&app, "Cooperativa Muestra")
    );
    assert_eq!(one(&app, &format!("SELECT telefono_r FROM students WHERE uuid = '{id}'")), "787-555-0190");
    assert_eq!(app.count(&format!("SELECT COUNT(*) FROM student_certifications WHERE student_uuid = '{id}'")), 4);
    let html = body(app.get(&format!("/students/{id}?tab=courses"), Some(&staff)).await).await;
    assert!(html.contains("<b>Ana Nueva Demo</b>") && html.contains("Sin matrículas"));
}

/// UI-15: summary tiles from `v_student_academic`, the tabs (`?tab=`, htmx swap with push-url), certification progress
/// with the missing courses and "Certificado" only for the graduated row (BR-CRT-04), enrollment links to UI-20,
/// EXT-02 map link, staff view.
#[tokio::test]
async fn ui15_student_page_and_tabs() {
    let app = App::new();
    let staff = app.staff().await;
    let id = elena(&app);
    assert_eq!(app.get("/students/nope", Some(&staff)).await.status(), StatusCode::NOT_FOUND);
    let html = body(app.get(&format!("/students/{id}"), Some(&staff)).await).await;
    assert!(html.contains("<b>Elena Muestra Ficticia</b> <span class=\"sub\"> · Banco Ejemplo · #4821</span>"));
    assert!(
        html.contains("<div class=\"v\">90 <span class=\"sch-letter\">A</span></div>")
            && html.contains("4 cursos aprobados")
    );
    assert!(html.contains("<div class=\"v\">12</div>") && html.contains("<div class=\"v\">180</div>"));
    assert!(html.contains("title=\"BR-AFF-01\">Afiliado</span>") && html.contains(">ACTIVO</span>"));
    assert_eq!(html.matches(">Certificado</a>").count(), 1, "only the graduated certification (BR-CRT-04)");
    assert!(html.contains("Documentación de Cierre") && html.contains("hx-push-url=\"true\""));
    assert!(html.contains(&format!("href=\"/students/{id}/enrollments/seminar/new\"")));
    assert!(html.contains(&format!("href=\"/students/{id}/transcript\"")));
    assert!(!html.contains("Borrar estudiante"), "staff (AP-04)");

    let html = body(app.get(&format!("/students/{id}?tab=courses"), Some(&staff)).await).await;
    assert!(
        html.contains("Suscripción de Préstamos FHA")
            && html.contains("No aprobado")
            && html.contains("balance $575.00")
    );
    let html = body(app.get(&format!("/students/{id}?tab=seminars"), Some(&staff)).await).await;
    assert!(html.contains("No Canceló") && html.contains("/enrollments/seminar/"));
    let html = body(app.get(&format!("/students/{id}?tab=short-courses"), Some(&staff)).await).await;
    assert!(html.contains("Elena no tiene cursos cortos"));
    let html = body(app.get(&format!("/students/{id}?tab=details"), Some(&staff)).await).await;
    let at = html.find("google.com/maps").unwrap_or(0);
    assert!(html.contains("value=\"Banco Ejemplo (A-1001)\""), "{}", &html[html.find("es-afi").unwrap_or(0)..][..300]);
    assert!(html.contains("google.com/maps/search/?api=1&#38;query=Calle+Ficticia+123"), "{}", &html[at..at + 120]);
    assert!(html.contains("Suscriptor Hipotecario (CSH)") && !html.contains("ssn"));

    let req = Request::get(format!("/students/{id}?tab=seminars"))
        .header(header::COOKIE, &staff.cookie)
        .header("hx-request", "true")
        .body(Body::empty())
        .unwrap();
    let frag = body(app.send(req).await).await;
    assert!(frag.contains("id=\"stu-tabs\"") && !frag.contains("<html"));

    let admin = app.admin().await;
    assert!(body(app.get(&format!("/students/{id}"), Some(&admin)).await).await.contains("Borrar estudiante"));
}

/// UI-15 autosave: 200 + audit row; 422 typed value, nothing written; contract errors 422; no CSRF 403; name edit
/// returns the title out of band; address edit returns the map link out of band; 404 unknown student.
#[tokio::test]
async fn ui15_student_autosave() {
    let app = App::new();
    let staff = app.staff().await;
    let id = elena(&app);
    let url = format!("/students/{id}");
    let res = app.patch(&url, &staff, "phone_mobile=939-555-0999", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body(res).await.contains("data-saved=\"939-555-0999\""));
    assert_eq!(app.count("SELECT COUNT(*) FROM audit_log WHERE action = 'field.change' AND entity = 'students'"), 1);

    let res = app.patch(&url, &staff, "email=elena%40ejemplo", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let frag = body(res).await;
    assert!(frag.contains("data-unsaved") && frag.contains("value=\"elena@ejemplo\""));
    assert_eq!(one(&app, &format!("SELECT email FROM students WHERE uuid = '{id}'")), "elena.muestra@ejemplo.com");
    assert_eq!(app.patch(&url, &staff, "nombre=", true).await.status(), StatusCode::UNPROCESSABLE_ENTITY);

    assert_eq!(app.patch(&url, &staff, "nmls=1&fax=2", true).await.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(app.patch(&url, &staff, "ssn=1", true).await.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(app.patch(&url, &staff, "nmls=1", false).await.status(), StatusCode::FORBIDDEN);
    assert_eq!(app.patch("/students/nope", &staff, "nmls=1", true).await.status(), StatusCode::NOT_FOUND);

    let res = app.patch(&url, &staff, "nombre=Elena+Muestra+Cambiada", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body(res).await.contains("hx-swap-oob=\"innerHTML:#stu-title\""));
    let res = app.patch(&url, &staff, "home_city=Ponce", true).await;
    let frag = body(res).await;
    assert!(frag.contains("hx-swap-oob=\"innerHTML:#stu-map\"") && frag.contains("Ponce"));
}

/// BR-STU-02: the affiliate is picked by key from the typeahead; its name goes to the company (out of band with the
/// title and tags); an unknown value is 422; empty unlinks and keeps the typed company (BR-AFF-02).
#[tokio::test]
async fn br_stu_02_affiliate_typeahead_by_key() {
    let app = App::new();
    let staff = app.staff().await;
    let id = elena(&app);
    let url = format!("/students/{id}");
    let opts = body(app.get("/affiliates/options?q=Acme", Some(&staff)).await).await;
    assert_eq!(opts, "<option value=\"Acme Mortgage Demo (A-1002)\"></option>");

    let res = app.patch(&url, &staff, "member_uuid=Acme+Mortgage+Demo+%28A-1002%29", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    let frag = body(res).await;
    assert!(
        frag.contains("value=\"Acme Mortgage Demo (A-1002)\"") && frag.contains("hx-swap-oob=\"innerHTML:#stu-comp\"")
    );
    assert!(frag.contains("hx-swap-oob=\"innerHTML:#stu-tags\"") && frag.contains("· Acme Mortgage Demo"));
    assert_eq!(one(&app, &format!("SELECT compania FROM students WHERE uuid = '{id}'")), "Acme Mortgage Demo");
    assert_eq!(
        one(&app, &format!("SELECT member_uuid FROM students WHERE uuid = '{id}'")),
        affiliate(&app, "Acme Mortgage Demo")
    );

    let res = app.patch(&url, &staff, "member_uuid=Acme", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body(res).await.contains("Escoja un afiliado de la lista."));

    assert_eq!(app.patch(&url, &staff, "member_uuid=", true).await.status(), StatusCode::OK);
    assert_eq!(app.count(&format!("SELECT COUNT(*) FROM students WHERE uuid = '{id}' AND member_uuid IS NULL")), 1);
    assert_eq!(one(&app, &format!("SELECT compania FROM students WHERE uuid = '{id}'")), "Acme Mortgage Demo");
}

/// BR-CRT-04 on UI-15: a graduation date returns the whole row with "Graduado" and "Certificado"; a bad date is 422
/// with the typed value in the row; clearing it hides the button again.
#[tokio::test]
async fn br_crt_04_graduation_date_row() {
    let app = App::new();
    let staff = app.staff().await;
    let id = elena(&app);
    let sc = one(
        &app,
        &format!(
            "SELECT sc.uuid FROM student_certifications sc JOIN certifications c ON c.uuid = sc.certification_uuid
              WHERE sc.student_uuid = '{id}' AND c.certification = 'CSH'"
        ),
    );
    let url = format!("/student-certifications/{sc}");
    let res = app.patch(&url, &staff, "graduation_date=7%2F1%2F2026", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    let row = body(res).await;
    assert!(
        row.starts_with("<tr id=\"cert-") && row.contains("value=\"7/1/2026\"") && row.contains(">Certificado</a>")
    );
    assert!(row.contains("Graduado"));
    let res = app.patch(&url, &staff, "graduation_date=32%2F1%2F2026", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body(res).await.contains("value=\"32/1/2026\""));
    let row = body(app.patch(&url, &staff, "graduation_date=", true).await).await;
    assert!(!row.contains("Certificado"));
    assert_eq!(app.patch(&url, &staff, "total=9", true).await.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

/// OP-STU-03 / BR-STU-05: add an interest (200, the block with the date), a duplicate is 422, remove (AP-06), the
/// plain form redirects to the details tab.
#[tokio::test]
async fn op_stu_03_interests() {
    let app = App::new();
    let staff = app.staff().await;
    let id = elena(&app);
    let cpp = one(&app, "SELECT uuid FROM certifications WHERE certification = 'CPP'");
    let res =
        hx(&app, "POST", &format!("/students/{id}/interests"), &staff, &format!("certification_uuid={cpp}")).await;
    assert_eq!(res.status(), StatusCode::OK);
    let frag = body(res).await;
    assert!(frag.starts_with("<div class=\"field-wrap\" id=\"stu-interests\">") && frag.contains("(CPP)"));
    assert_eq!(
        app.count(&format!("SELECT COUNT(*) FROM student_certification_interests WHERE student_uuid = '{id}'")),
        3
    );
    let res =
        hx(&app, "POST", &format!("/students/{id}/interests"), &staff, &format!("certification_uuid={cpp}")).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body(res).await.contains("Ya está en la lista."));

    let interest =
        one(&app, &format!("SELECT uuid FROM student_certification_interests WHERE certification_uuid = '{cpp}'"));
    let res = hx(&app, "DELETE", &format!("/students/{id}/interests/{interest}"), &staff, "").await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        app.count(&format!("SELECT COUNT(*) FROM student_certification_interests WHERE student_uuid = '{id}'")),
        2
    );

    let res = app
        .post_form(
            &format!("/students/{id}/interests"),
            Some(&staff),
            &format!("_csrf={}&certification_uuid={cpp}", staff.csrf),
        )
        .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert!(location(&res).ends_with("?tab=details"));
}

/// OP-STU-02 duplicate and OP-STU-04 delete: staff 403; admin with enrollments 409 with the refusal dialog; admin
/// without enrollments 303 to the list.
#[tokio::test]
async fn op_stu_02_04_duplicate_and_delete() {
    let app = App::new();
    let staff = app.staff().await;
    let admin = app.admin().await;
    let id = elena(&app);
    let before = app.count("SELECT COUNT(*) FROM students");
    let res = app.post_form(&format!("/students/{id}/duplicate"), Some(&staff), &format!("_csrf={}", staff.csrf)).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let copy = location(&res).trim_start_matches("/students/").trim_end_matches("?tab=details").to_owned();
    assert_eq!(app.count("SELECT COUNT(*) FROM students"), before + 1);
    assert_eq!(one(&app, &format!("SELECT nombre FROM students WHERE uuid = '{copy}'")), "Elena Muestra Ficticia");
    assert_eq!(app.count(&format!("SELECT COUNT(*) FROM student_certifications WHERE student_uuid = '{copy}'")), 4);

    assert_eq!(delete(&app, &format!("/students/{copy}"), &staff).await.status(), StatusCode::FORBIDDEN);
    let res = delete(&app, &format!("/students/{id}"), &admin).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let html = body(res).await;
    assert!(
        html.contains("data-open=\"dlg-borrar-stu\"")
            && html.contains("409 · Conflicto.")
            && html.contains("tiene 10 matrículas")
    );
    let res = delete(&app, &format!("/students/{copy}"), &admin).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&res), "/students?hecho=borrado");
    assert_eq!(app.count(&format!("SELECT COUNT(*) FROM students WHERE uuid = '{copy}'")), 0);
    let fresh = new_student(&app, &staff, "Borrable").await;
    assert_eq!(delete(&app, &format!("/students/{fresh}"), &admin).await.status(), StatusCode::SEE_OTHER);
}

/// UI-16 / UI-17 / OP-AFF-01: list with counts and filters, create dialog 422/303, autosave with the head out of band
/// and the map link (EXT-02), 422 bad type, delete staff 403, admin 303 and the students unlinked keep their company.
#[tokio::test]
async fn ui16_17_affiliates() {
    let app = App::new();
    assert_eq!(app.get("/affiliates", None).await.status(), StatusCode::SEE_OTHER);
    let staff = app.staff().await;
    let admin = app.admin().await;
    let html = body(app.get("/affiliates", Some(&staff)).await).await;
    assert!(html.contains("<b>Afiliados</b> <span class=\"sub\">· 5 activos de 7</span>") && html.contains("A-1001"));
    assert!(!html.contains("Borrar afiliado"));
    let html = body(app.get("/affiliates?tipo=INACTIVO", Some(&staff)).await).await;
    assert!(html.contains("Capital Ficticio Lending") && !html.contains(">Banco Ejemplo<"));
    assert!(body(app.get("/affiliates?q=zzz", Some(&staff)).await).await.contains("Sin resultados"));

    assert_eq!(
        app.post_form("/affiliates", Some(&staff), &format!("_csrf={}&socio=", staff.csrf)).await.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let res =
        app.post_form("/affiliates", Some(&staff), &format!("_csrf={}&socio=Nuevo+Afiliado+Demo", staff.csrf)).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let fresh = location(&res).trim_start_matches("/affiliates/").to_owned();
    let html = body(app.get(&format!("/affiliates/{fresh}"), Some(&staff)).await).await;
    assert!(
        html.contains("A-1008") && html.contains("Sin estudiantes") && html.contains("Sin dirección para el mapa.")
    );

    let banco = affiliate(&app, "Banco Ejemplo");
    let url = format!("/affiliates/{banco}");
    let html = body(app.get(&url, Some(&staff)).await).await;
    assert!(html.contains("Elena Muestra Ficticia") && html.contains("query=Avenida+Ficticia+400"));
    let res = app.patch(&url, &staff, "tipo_de_negocio=Banco+de+ahorros", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body(res).await.contains("hx-swap-oob=\"innerHTML:#afi-title\""));
    let res = app.patch(&url, &staff, "city=Ponce", true).await;
    assert!(body(res).await.contains("hx-swap-oob=\"innerHTML:#afi-map\""));
    assert_eq!(app.patch(&url, &staff, "cust_type=OTRO", true).await.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(app.patch(&url, &staff, "account_num=1", true).await.status(), StatusCode::UNPROCESSABLE_ENTITY);

    assert_eq!(delete(&app, &url, &staff).await.status(), StatusCode::FORBIDDEN);
    let linked = app.count(&format!("SELECT COUNT(*) FROM students WHERE member_uuid = '{banco}'"));
    let res = delete(&app, &url, &admin).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&res), format!("/affiliates?hecho=borrado&n={linked}"));
    assert_eq!(one(&app, "SELECT compania FROM students WHERE nombre = 'Elena Muestra Ficticia'"), "Banco Ejemplo");
    assert_eq!(
        app.count("SELECT COUNT(*) FROM students WHERE nombre = 'Elena Muestra Ficticia' AND member_uuid IS NULL"),
        1
    );
    let html = body(app.get(&location(&res), Some(&admin)).await).await;
    assert!(html.contains("estudiantes quedaron sin afiliado"));
}
