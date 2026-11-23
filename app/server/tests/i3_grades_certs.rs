//! Route-level tests of UI-22 Importar notas (OP-ENR-07, T-05-12 at the route) and UI-32 Certificaciones y
//! profesores (OP-CRT-01, OP-CRS-04; AP-08): status codes, the preview that writes nothing, the token rules, staff
//! 403, autosave 200/422 with the out-of-band name, and the 409 refusals.

#[path = "autosave_support.rs"]
mod support;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use mba_server::school::grades;
use support::{body, location, App, Who};

fn one(app: &App, sql: &str) -> String {
    app.conn().query_row(sql, [], |r| r.get(0)).unwrap()
}

async fn delete(app: &App, path: &str, who: &Who) -> axum::http::Response<Body> {
    app.post_form(path, Some(who), &format!("_method=DELETE&_csrf={}", who.csrf)).await
}

/// The htmx upload of UI-22: multipart `file`, CSRF in the header (unless `csrf` is false).
async fn upload(app: &App, who: &Who, name: &str, bytes: &[u8], csrf: bool) -> axum::http::Response<Body> {
    let boundary = "----mbaTestBoundary7";
    let mut b = Vec::new();
    b.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: application/vnd.openxmlformats-officedocument.spreadsheetml.sheet\r\n\r\n").as_bytes());
    b.extend_from_slice(bytes);
    b.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let mut req = Request::post("/grades/import")
        .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
        .header(header::COOKIE, &who.cookie)
        .header("hx-request", "true");
    if csrf {
        req = req.header("x-csrf-token", &who.csrf);
    }
    app.send(req.body(Body::from(b)).unwrap()).await
}

/// The apply form of the dialog (plain form, CSRF in the body).
async fn apply(app: &App, who: &Who, token: &str) -> axum::http::Response<Body> {
    app.post_form(&format!("/grades/import/{token}/apply"), Some(who), &format!("_csrf={}", who.csrf)).await
}

fn token_of(html: &str) -> String {
    let marker = "action=\"/grades/import/";
    let start = html.find(marker).expect("apply form") + marker.len();
    html[start..start + html[start..].find('/').unwrap()].to_owned()
}

/// Every grade column of every course enrollment (the preview must not change it).
fn dump(app: &App) -> Vec<String> {
    let conn = app.conn();
    let cols =
        grades::COLUMNS.iter().map(|(_, c)| format!("COALESCE({c}, '')")).collect::<Vec<_>>().join(" || '|' || ");
    let mut stmt = conn
        .prepare(&format!(
            "SELECT uuid || ':' || {cols} || ':' || COALESCE(puntuacion, '') || modified_at FROM enrollments_course ORDER BY uuid"
        ))
        .unwrap();
    stmt.query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap()
}

/// The test input: the grades XLSX of the FHA 9/14/2026 calendar row as the export writes it (the same
/// `grades::export_xlsx` that `GET /exports/grades.xlsx` serves), with grades typed in for 10484 (Héctor, no grades yet), one serial that
/// has no enrollment (10511) and one row without a serial.
fn filled_export(app: &App) -> Vec<u8> {
    use calamine::{Data, Reader, Xlsx};
    let conn = app.conn();
    let cal: String = conn
        .query_row("SELECT course_calendar_uuid FROM enrollments_course WHERE serial = 10484", [], |r| r.get(0))
        .unwrap();
    let export = grades::export_xlsx(&conn, Some(&cal)).unwrap();
    let mut wb: Xlsx<_> = Xlsx::new(std::io::Cursor::new(export)).unwrap();
    let range = wb.worksheet_range_at(0).unwrap().unwrap();
    let rows: Vec<Vec<Data>> = range.rows().map(|r| r.to_vec()).collect();
    let header: Vec<String> = rows[0].iter().map(|c| c.to_string()).collect();
    let ix = |h: &str| header.iter().position(|x| x == h).unwrap();
    let (serial, examen, fin) = (ix("Serial"), ix("Examen"), ix("Promedio final"));

    let mut out = rust_xlsxwriter::Workbook::new();
    let ws = out.add_worksheet();
    for (c, h) in header.iter().enumerate() {
        ws.write_string(0, c as u16, h).unwrap();
    }
    let mut r_out = 1u32;
    for r in rows.iter().skip(1) {
        let is_hector = matches!(r[serial], Data::Float(f) if f as i64 == 10484);
        for (c, cell) in r.iter().enumerate() {
            let v = match (is_hector, c) {
                (true, c) if c == examen => Data::Float(81.0),
                (true, c) if c == fin => Data::Float(84.0),
                _ => cell.clone(),
            };
            match v {
                Data::Float(f) => ws.write_number(r_out, c as u16, f).map(|_| ()).unwrap(),
                Data::Int(n) => ws.write_number(r_out, c as u16, n as f64).map(|_| ()).unwrap(),
                Data::String(s) => ws.write_string(r_out, c as u16, &s).map(|_| ()).unwrap(),
                _ => {}
            }
        }
        r_out += 1;
    }
    ws.write_number(r_out, serial as u16, 10511.0).unwrap();
    ws.write_number(r_out, examen as u16, 77.0).unwrap();
    ws.write_string(r_out + 1, 1, "Sin serial").unwrap();
    ws.write_number(r_out + 1, examen as u16, 70.0).unwrap();
    out.save_to_buffer().unwrap()
}

// ===============================================================================================================
// UI-22
// ===============================================================================================================

/// UI-22 step 1: AP-01, both roles (AP-09), a bad file and no file give 422 with the message, CSRF on the upload.
#[tokio::test]
async fn ui22_upload_page_and_bad_files() {
    let app = App::new();
    assert_eq!(app.get("/grades/import", None).await.status(), StatusCode::SEE_OTHER);
    let staff = app.staff().await;
    let res = app.get("/grades/import", Some(&staff)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body(res).await;
    assert!(html.contains("paso 1 de 3") && html.contains("Escoger archivo") && html.contains("aria-current=\"step\""));
    assert!(html.contains("data-nav=\"importar-notas\" aria-current=\"page\""));

    let res = upload(&app, &staff, "notas.xlsx", b"not a workbook", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = body(res).await;
    assert!(html.contains("El archivo no es un XLSX válido.") && html.contains("paso 1 de 3"), "{html}");
    let res = upload(&app, &staff, "", b"", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body(res).await.contains("Escoja el archivo XLSX"));
    let res = upload(&app, &staff, "notas.xlsx", &filled_export(&app), false).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN, "AP-02: the upload carries the CSRF token");
}

/// T-05-12 at the route (and T-07-12): the export XLSX in, a preview that writes nothing, the token is the owner's
/// for 30 minutes, apply in one transaction, Puntuacion / NOTA / is_passed follow the final average, unmatched
/// rows listed and never created.
#[tokio::test]
async fn t05_12_route_preview_then_apply() {
    let app = App::new();
    let staff = app.staff().await;
    let admin = app.admin().await;
    let file = filled_export(&app);
    let enrollments = app.count("SELECT COUNT(*) FROM enrollments_course");
    let before = dump(&app);

    let res = upload(&app, &staff, "notas-fha-2026-09-14.xlsx", &file, true).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body(res).await;
    assert!(html.contains("paso 2 de 3") && html.contains("notas-fha-2026-09-14.xlsx"));
    assert!(html.contains("Suscripción de Préstamos FHA · 9/14/2026"), "scope of the file");
    assert!(html.contains("1 cambio · 17 sin cambio"), "one changed row");
    assert!(html.contains("<td class=\"num\">10484</td>") && html.contains("<span class=\"diff-new\">84</span>"));
    assert!(html.contains("10511 <span class=\"tag warn\">Sin pareja</span>"));
    assert!(html.contains("<span class=\"muted\">vacío</span> <span class=\"tag warn\">Sin pareja</span>"));
    assert!(html.contains("hx-swap-oob=\"innerHTML:#grd-title\""), "title follows the step");
    assert!(html.contains("Aplicar 1 cambio"));
    assert_eq!(dump(&app), before, "the preview changes nothing");
    let token = token_of(&html);

    // Another user's token: 404, nothing written.
    let res = apply(&app, &admin, &token).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert!(body(res).await.contains("La vista previa ya no existe"));
    assert_eq!(dump(&app), before);

    let res = apply(&app, &staff, &token).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&res), "/grades/import?aplicado=1&sin_pareja=2");
    let (examen, fin, score, nota, passed): (f64, f64, i64, String, i64) = app
        .conn()
        .query_row(
            "SELECT e.examen, e.promedio_final, e.puntuacion, v.nota, v.is_passed FROM enrollments_course e
               JOIN v_enrollments_course_lines v ON v.serial = e.serial WHERE e.serial = 10484",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!((examen, fin, score, nota.as_str(), passed), (81.0, 84.0, 84, "B", 1), "BR-ENR-06");
    assert_eq!(app.count("SELECT COUNT(*) FROM enrollments_course"), enrollments, "no enrollment created");
    assert_eq!(app.count("SELECT COUNT(*) FROM audit_log WHERE action = 'grades.import'"), 1);
    let changed = dump(&app).iter().zip(&before).filter(|(a, b)| a != b).count();
    assert_eq!(changed, 1, "only the matched, changed row");

    let html = body(app.get("/grades/import?aplicado=1&sin_pareja=2", Some(&staff)).await).await;
    assert!(html.contains("1 matrícula actualizada.") && html.contains("2 filas sin pareja no cambiaron nada."));
    assert!(html.contains("paso 3 de 3"));

    assert_eq!(apply(&app, &staff, &token).await.status(), StatusCode::NOT_FOUND, "the token is spent");

    // Expired after 30 minutes.
    let html = body(upload(&app, &staff, "notas.xlsx", &file, true).await).await;
    assert!(html.contains("0 cambios · ") && html.contains("Nada que aplicar"), "same grades again: no change");
    assert!(!html.contains("action=\"/grades/import/"), "no apply form without changes");
    let html = body(upload(&app, &staff, "notas.xlsx", &file_with_change(&app), true).await).await;
    let token = token_of(&html);
    grades::age_preview_for_tests(&token, grades::PREVIEW_MINUTES + 1);
    assert_eq!(apply(&app, &staff, &token).await.status(), StatusCode::NOT_FOUND, "expired");
}

/// A one-row file that changes 10487's exam.
fn file_with_change(_app: &App) -> Vec<u8> {
    let mut wb = rust_xlsxwriter::Workbook::new();
    let ws = wb.add_worksheet();
    ws.write_string(0, 0, "Serial").unwrap();
    ws.write_string(0, 1, "Examen").unwrap();
    ws.write_number(1, 0, 10487.0).unwrap();
    ws.write_number(1, 1, 55.0).unwrap();
    wb.save_to_buffer().unwrap()
}

// ===============================================================================================================
// UI-32
// ===============================================================================================================

/// AP-08: every UI-32 route is ROLE-ADMIN only; staff gets 403 and nothing changes.
#[tokio::test]
async fn ui32_staff_403() {
    let app = App::new();
    let staff = app.staff().await;
    let crt = one(&app, "SELECT uuid FROM certifications ORDER BY certification LIMIT 1");
    let prof = one(&app, "SELECT uuid FROM professors ORDER BY name LIMIT 1");
    for path in ["/admin/certifications".to_owned(), "/admin/professors".into(), format!("/admin/certifications/{crt}")]
    {
        let res = app.get(&path, Some(&staff)).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN, "{path}");
        assert!(body(res).await.contains("Sin permiso"));
    }
    let res = app.patch(&format!("/admin/certifications/{crt}"), &staff, "description=X", true).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let res = app.patch(&format!("/admin/professors/{prof}"), &staff, "name=X", true).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let res =
        app.post_form("/admin/certifications", Some(&staff), &format!("certification=ZZZ&_csrf={}", staff.csrf)).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let res = app.post_form("/admin/professors", Some(&staff), &format!("name=Prof.+X&_csrf={}", staff.csrf)).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(delete(&app, &format!("/admin/professors/{prof}"), &staff).await.status(), StatusCode::FORBIDDEN);
    assert_eq!(app.count("SELECT COUNT(*) FROM certifications"), 4);
    assert_eq!(app.count("SELECT COUNT(*) FROM professors"), 6);
}

/// UI-32 page, certification autosave (200 with the name out of band, 422), create (every student gets a row),
/// requirements add / type / remove, and delete (409 while students have progress).
#[tokio::test]
async fn ui32_certifications() {
    let app = App::new();
    let admin = app.admin().await;
    let res = app.get("/admin/certifications", Some(&admin)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body(res).await;
    assert!(html.contains("<span class=\"tag draft\">borrador</span>") && html.contains("Pantalla de inventario"));
    assert!(html.contains("<h3>Certificaciones</h3><span class=\"count\">4</span>"));
    assert!(html.contains("<h3>Profesores</h3><span class=\"count\">6</span>"));
    assert!(html.contains("aria-selected=\"true\"") && html.contains("data-save="), "a record is selected");
    assert_eq!(app.get("/admin/professors", Some(&admin)).await.status(), StatusCode::OK);
    assert_eq!(app.get("/admin/certifications/nope", Some(&admin)).await.status(), StatusCode::NOT_FOUND);

    let coh = one(&app, "SELECT uuid FROM certifications WHERE certification = 'COH'");
    let url = format!("/admin/certifications/{coh}");
    let res = app.patch(&url, &admin, "description=Originador+Hipotecario+Certificado", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body(res).await;
    assert!(html.contains(&format!("hx-swap-oob=\"innerHTML:#crt-name-{coh}\"")) && html.contains("#crt-title"));
    assert_eq!(
        one(&app, &format!("SELECT description FROM certifications WHERE uuid = '{coh}'")),
        "Originador Hipotecario Certificado"
    );
    let res = app.patch(&url, &admin, "certification=", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body(res).await.contains("aria-invalid=\"true\""));
    let res = app.patch(&url, &admin, "number_of_required_electives=abc", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let res = app.patch(&url, &admin, "number_of_required_electives=3", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(!body(res).await.contains("hx-swap-oob"), "electives do not touch the name");
    assert_eq!(app.patch(&url, &admin, "description=X", false).await.status(), StatusCode::FORBIDDEN, "AP-02");

    // Create: 422 without a code, then 303 and one progress row per student.
    let res =
        app.post_form("/admin/certifications", Some(&admin), &format!("certification=&_csrf={}", admin.csrf)).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body(res).await.contains("data-open=\"dlg-nueva-crt\""));
    let res = app
        .post_form(
            "/admin/certifications",
            Some(&admin),
            &format!("certification=CPR&description=Prueba+de+Ruta&_csrf={}", admin.csrf),
        )
        .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let new = one(&app, "SELECT uuid FROM certifications WHERE certification = 'CPR'");
    assert_eq!(location(&res), format!("/admin/certifications/{new}"));
    assert_eq!(
        app.count(&format!("SELECT COUNT(*) FROM student_certifications WHERE certification_uuid = '{new}'")),
        app.count("SELECT COUNT(*) FROM students")
    );
    let html = body(app.get(&location(&res), Some(&admin)).await).await;
    assert!(html.contains("<h3 id=\"crt-title\">Prueba de Ruta</h3>") && html.contains("Sin cursos todavía"));

    // Requirements.
    let course = one(&app, "SELECT uuid FROM courses ORDER BY course LIMIT 1");
    let req_url = format!("/admin/certifications/{new}/requirements");
    let res = app
        .post_form(&req_url, Some(&admin), &format!("course_uuid={course}&type=Requisito&_csrf={}", admin.csrf))
        .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let res = app
        .post_form(&req_url, Some(&admin), &format!("course_uuid={course}&type=Electiva&_csrf={}", admin.csrf))
        .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY, "one row per course");
    assert!(body(res).await.contains("Ese curso ya es requisito."));
    let res =
        app.post_form(&req_url, Some(&admin), &format!("course_uuid={course}&type=Otro&_csrf={}", admin.csrf)).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY, "BR-CRT-01 type");
    let req = one(&app, &format!("SELECT uuid FROM certification_requirements WHERE certification_uuid = '{new}'"));
    let res = app.patch(&format!("{req_url}/{req}"), &admin, "type=Electiva", true).await;
    assert_eq!(res.status(), StatusCode::OK, "htmx gets the content");
    assert!(body(res).await.contains("<span class=\"tag plain\">Electiva</span>"));
    let res = app.patch(&format!("{req_url}/{req}"), &admin, "type=Otro", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let other_req =
        one(&app, &format!("SELECT uuid FROM certification_requirements WHERE certification_uuid = '{coh}' LIMIT 1"));
    assert_eq!(
        delete(&app, &format!("{req_url}/{other_req}"), &admin).await.status(),
        StatusCode::NOT_FOUND,
        "a requirement of another certification"
    );
    assert_eq!(delete(&app, &format!("{req_url}/{req}"), &admin).await.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        app.count(&format!("SELECT COUNT(*) FROM certification_requirements WHERE certification_uuid = '{new}'")),
        0
    );

    // Delete: COH has a graduate (409); the new one goes.
    let res = delete(&app, &url, &admin).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let html = body(res).await;
    assert!(html.contains("data-open=\"dlg-refusal\"") && html.contains("No se puede borrar"));
    let res = delete(&app, &format!("/admin/certifications/{new}"), &admin).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&res), "/admin/certifications?hecho=borrada");
    assert_eq!(app.count("SELECT COUNT(*) FROM certifications"), 4);
    assert!(body(app.get("/admin/certifications?hecho=borrada", Some(&admin)).await)
        .await
        .contains("Certificación borrada."));
}

/// Professors (OP-CRS-04): create (unique name), the edit row, autosave 200/422, delete (409 while named on a
/// class date).
#[tokio::test]
async fn ui32_professors() {
    let app = App::new();
    let admin = app.admin().await;
    let res =
        app.post_form("/admin/professors", Some(&admin), &format!("name=Prof.+Nueva+Ruta&_csrf={}", admin.csrf)).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&res), "/admin/professors");
    let res =
        app.post_form("/admin/professors", Some(&admin), &format!("name=Prof.+Nueva+Ruta&_csrf={}", admin.csrf)).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = body(res).await;
    assert!(html.contains("data-open=\"dlg-nuevo-prof\"") && html.contains("Ya existe un profesor con ese nombre."));

    let new = one(&app, "SELECT uuid FROM professors WHERE name = 'Prof. Nueva Ruta'");
    let res = app.get(&format!("/admin/professors/{new}/edit"), Some(&admin)).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body(res).await.contains(&format!("hx-patch=\"/admin/professors/{new}\"")));
    let res = app.patch(&format!("/admin/professors/{new}"), &admin, "name=Prof.+Ruta+Cambiada", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(one(&app, &format!("SELECT name FROM professors WHERE uuid = '{new}'")), "Prof. Ruta Cambiada");
    let taken = one(&app, "SELECT name FROM professors WHERE uuid <> (SELECT uuid FROM professors WHERE name = 'Prof. Ruta Cambiada') ORDER BY name LIMIT 1");
    let res = app
        .patch(&format!("/admin/professors/{new}"), &admin, &format!("name={}", taken.replace(' ', "+")), true)
        .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body(res).await.contains("Ya existe un profesor con ese nombre."));
    let res = app.patch(&format!("/admin/professors/{new}"), &admin, "name=", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let used = one(&app, "SELECT professor_uuid FROM calendar_course_dates WHERE professor_uuid IS NOT NULL LIMIT 1");
    let res = delete(&app, &format!("/admin/professors/{used}"), &admin).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert!(body(res).await.contains("No se puede borrar: el profesor está en"));
    let res = delete(&app, &format!("/admin/professors/{new}"), &admin).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(app.count("SELECT COUNT(*) FROM professors"), 6);
}
