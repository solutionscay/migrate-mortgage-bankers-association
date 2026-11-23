//! I2 phase 2, events pane: route-level checks of UI-08 Convenciones, UI-09 Convención (tabs, header autosave,
//! PDF, activities, sponsors, billing, delete), UI-10 Detalle de actividad and UI-11 Auspiciador.
//! OP-EVT-01..03, OP-SPN-01..02; AP-03, AP-04, AP-06; BR-EVT-01, BR-EVT-03.

#[path = "autosave_support.rs"]
mod support;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use support::{body, location, App, Who};

fn one(app: &App, sql: &str) -> String {
    app.conn().query_row(sql, [], |r| r.get(0)).unwrap()
}

fn demo_event(app: &App) -> String {
    one(app, "SELECT uuid FROM events WHERE name = 'Convención Demo 2027'")
}

/// A form post with `_method=DELETE` and the CSRF token, as the confirm dialogs send it.
async fn form_delete(app: &App, path: &str, who: &Who) -> axum::http::Response<Body> {
    app.post_form(path, Some(who), &format!("_csrf={}&_method=DELETE", who.csrf)).await
}

fn stored_file(app: &App, mime: &str, name: &str) -> String {
    let uuid = format!("f-{name}");
    app.conn()
        .execute(
            "INSERT INTO stored_files (uuid, filename, mime_type, size_bytes, sha256, storage_path, created_at, created_by)
             VALUES (?1, ?2, ?3, 10, 'x', 'files/xx/x', '2026-11-23 10:00:00', 'test')",
            [&uuid, name, mime],
        )
        .unwrap();
    uuid
}

async fn upload(app: &App, path: &str, who: &Who, filename: &str, bytes: &[u8]) -> axum::http::Response<Body> {
    let b = "XBOUNDARYX";
    let mut data = format!(
        "--{b}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
    )
    .into_bytes();
    data.extend_from_slice(bytes);
    data.extend_from_slice(format!("\r\n--{b}--\r\n").as_bytes());
    let req = Request::post(path)
        .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={b}"))
        .header(header::COOKIE, &who.cookie)
        .header("x-csrf-token", &who.csrf)
        .header("hx-request", "true")
        .body(Body::from(data))
        .unwrap();
    app.send(req).await
}

/// UI-08: list, filter, both roles; the delete item is admin only.
#[tokio::test]
async fn ui08_list_filter_and_roles() {
    let app = App::new();
    let admin = app.admin().await;
    let staff = app.staff().await;
    assert_eq!(app.get("/events", None).await.status(), StatusCode::SEE_OTHER);
    let html = body(app.get("/events", Some(&admin)).await).await;
    assert!(html.contains("<b>Convenciones</b> <span class=\"sub\">· 6</span>"), "{html}");
    assert!(html.contains("Convención Demo 2027") && html.contains("Borrar convención"));
    let html = body(app.get("/events?status=Inactivo", Some(&staff)).await).await;
    assert!(html.contains("· 4</span>") && !html.contains("Convención Demo 2027"));
    assert!(!html.contains("Borrar convención"), "staff sees no delete (AP-04)");
    let html = body(app.get("/events?q=Foro", Some(&staff)).await).await;
    assert!(html.contains("· 3</span>"));
}

/// UI-09: tabs by `?tab=`, htmx fragment, totals from `v_event_totals`, 404.
#[tokio::test]
async fn ui09_tabs_totals_and_fragment() {
    let app = App::new();
    let staff = app.staff().await;
    let ev = demo_event(&app);
    let res = app.get(&format!("/events/{ev}"), Some(&staff)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body(res).await;
    assert!(html.contains("id=\"tp-act\"") && html.contains("Cena de Gala Aniversario"));
    assert!(html.contains("programa-convencion-2027.pdf") && html.contains("$2,105.00"));
    assert!(!html.contains("dlg-borrar-ev"), "no delete for ROLE-STAFF");
    let html = body(app.get(&format!("/events/{ev}?tab=sponsors"), Some(&staff)).await).await;
    assert!(html.contains("id=\"tp-aus\"") && html.contains("<b>$37,000.00</b>"));
    assert!(html.contains("$40,000.00") && html.contains("$24,000.00") && html.contains("$13,000.00"));
    let html = body(app.get(&format!("/events/{ev}?tab=billing"), Some(&staff)).await).await;
    assert!(html.contains("id=\"tp-fac\"") && html.contains("Lista_Facturacion_2027-05-04.pdf"));
    // htmx tab load: the content block only.
    let req = Request::get(format!("/events/{ev}?tab=sponsors"))
        .header(header::COOKIE, &staff.cookie)
        .header("hx-request", "true")
        .body(Body::empty())
        .unwrap();
    let html = body(app.send(req).await).await;
    assert!(!html.contains("<html") && html.contains("id=\"tp-aus\""));
    assert_eq!(app.get("/events/no-such", Some(&staff)).await.status(), StatusCode::NOT_FOUND);
}

/// OP-EVT-02 per-field autosave: 200 with the stat tiles out of band, 422 with the message, contract errors, CSRF.
/// BR-EVT-01: an emptied end date takes the start date.
#[tokio::test]
async fn evt02_header_autosave() {
    let app = App::new();
    let staff = app.staff().await;
    let ev = demo_event(&app);
    let url = format!("/events/{ev}");
    let res = app.patch(&url, &staff, "name=Convenci%C3%B3n+Demo+2027+B", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body(res).await;
    assert!(html.contains("id=\"w-e-nombre\"") && html.contains("hx-swap-oob=\"innerHTML:#ev-stats\""));
    assert!(html.contains("innerHTML:#ev-name") && !html.contains("data-unsaved"));
    let res = app.patch(&url, &staff, "date_end=fecha+mala", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = body(res).await;
    assert!(html.contains("data-unsaved") && html.contains("Use una fecha como 5/6/2027."));
    assert_eq!(app.patch(&url, &staff, "name=", true).await.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let res = app.patch(&url, &staff, "nombre=x", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(res.headers().get("hx-reswap").unwrap(), "none");
    assert_eq!(app.patch(&url, &staff, "name=a&location=b", true).await.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(app.patch(&url, &staff, "name=x", false).await.status(), StatusCode::FORBIDDEN);
    // BR-EVT-01.
    let res = app.patch(&url, &staff, "date_end=", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body(res).await;
    assert!(html.contains("value=\"5/6/2027\""), "{html}");
    assert_eq!(one(&app, &format!("SELECT date_end FROM events WHERE uuid = '{ev}'")), "2027-05-06");
    let res = app.patch(&url, &staff, "status=Inactivo", true).await;
    assert!(body(res).await.contains("innerHTML:#ev-tag"));
}

/// OP-EVT-01 PDF: the widget autosaves the uuid; non-PDF is refused; `x` removes it; POST/DELETE `/pdf` too.
#[tokio::test]
async fn evt01_pdf_widget() {
    let app = App::new();
    let staff = app.staff().await;
    let ev = demo_event(&app);
    let url = format!("/events/{ev}");
    let pdf = one(&app, &format!("SELECT pdf_file_uuid FROM events WHERE uuid = '{ev}'"));
    let png = stored_file(&app, "image/png", "logo.png");
    let res = app.patch(&url, &staff, &format!("pdf_file_uuid={png}"), true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body(res).await.contains("Use un archivo PDF."));
    let res = app.patch(&url, &staff, "pdf_file_uuid=", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body(res).await.contains("Escoger archivo"));
    let res = app.patch(&url, &staff, &format!("pdf_file_uuid={pdf}"), true).await;
    assert!(body(res).await.contains("programa-convencion-2027.pdf"));
    let req = Request::delete(format!("{url}/pdf"))
        .header(header::COOKIE, &staff.cookie)
        .header("x-csrf-token", &staff.csrf)
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.send(req).await.status(), StatusCode::OK);
    let res =
        app.post_form(&format!("{url}/pdf"), Some(&staff), &format!("_csrf={}&pdf_file_uuid={pdf}", staff.csrf)).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(one(&app, &format!("SELECT pdf_file_uuid FROM events WHERE uuid = '{ev}'")), pdf);
}

/// OP-EVT-01 create and delete: Activo, end = start; staff 403, with registrations 409, else 303.
#[tokio::test]
async fn evt01_create_and_delete() {
    let app = App::new();
    let admin = app.admin().await;
    let staff = app.staff().await;
    assert_eq!(app.get("/events/new", Some(&staff)).await.status(), StatusCode::OK);
    let res = app.post_form("/events", Some(&staff), &format!("_csrf={}&name=&date_start=", staff.csrf)).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body(res).await.contains("No se creó la convención."));
    let res = app
        .post_form(
            "/events",
            Some(&staff),
            &format!("_csrf={}&name=Foro+Nuevo&location=Sala&date_start=3%2F4%2F2028&date_end=", staff.csrf),
        )
        .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let new = location(&res).trim_start_matches("/events/").to_owned();
    let row: (String, String) = app
        .conn()
        .query_row("SELECT status, date_end FROM events WHERE uuid = ?1", [&new], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!(row, ("Activo".into(), "2028-03-04".into()));

    assert_eq!(form_delete(&app, &format!("/events/{new}"), &staff).await.status(), StatusCode::FORBIDDEN);
    let ev = demo_event(&app);
    let res = form_delete(&app, &format!("/events/{ev}"), &admin).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let html = body(res).await;
    assert!(html.contains("data-open=\"dlg-borrar-ev\"") && html.contains("No se puede borrar"));
    let res = form_delete(&app, &format!("/events/{new}"), &admin).await;
    assert_eq!((res.status(), location(&res)), (StatusCode::SEE_OTHER, "/events?borrada=1".into()));
    assert_eq!(app.count(&format!("SELECT COUNT(*) FROM events WHERE uuid = '{new}'")), 0);
}

/// OP-EVT-03: add (BR-EVT-03 order, BR-EVT-01 end date), item autosave with the end date out of band, UI-10, delete
/// refused while in use (409, AP-06).
#[tokio::test]
async fn evt03_activities() {
    let app = App::new();
    let staff = app.staff().await;
    let ev = demo_event(&app);
    let n = app.count(&format!("SELECT COUNT(*) FROM event_items WHERE event_uuid = '{ev}'"));
    let add = |body: String| {
        let app = &app;
        let staff = &staff;
        let ev = ev.clone();
        async move { app.post_form(&format!("/events/{ev}/items"), Some(staff), &body).await }
    };
    let res = add(format!("_csrf={}&type=Nada&name=X", staff.csrf)).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body(res).await.contains("data-open=\"dlg-act-nueva\""));
    let res = add(format!(
        "_csrf={}&type=Cena&name=Cena+Nueva&date_start=5%2F7%2F2027&time_start=7%3A00+PM&time_end=&price=99.50&qty=1",
        staff.csrf
    ))
    .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let item = location(&res).trim_start_matches("/event-items/").to_owned();
    let row: (i64, String, String, i64) = app
        .conn()
        .query_row("SELECT sort_order, date_end, time_start, price FROM event_items WHERE uuid = ?1", [&item], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .unwrap();
    assert_eq!(row, (n + 1, "2027-05-07".into(), "19:00".into(), 9950));

    let res = app.get(&format!("/event-items/{item}"), Some(&staff)).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body(res).await.contains("Ningún registro tiene esta actividad."));
    let res = app.patch(&format!("/event-items/{item}"), &staff, "date_start=5%2F8%2F2027", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body(res).await;
    assert!(html.contains("id=\"w-a-f2\" hx-swap-oob=\"true\"") && html.contains("value=\"5/8/2027\""));
    let res = app.patch(&format!("/event-items/{item}"), &staff, "price=abc", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        app.patch(&format!("/event-items/{item}"), &staff, "date_end=x", true).await.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );

    // In use: 409 with the refusal dialog open; unused: 303 back to the tab.
    let used = one(
        &app,
        &format!(
            "SELECT ei.uuid FROM event_items ei WHERE ei.event_uuid = '{ev}'
               AND EXISTS (SELECT 1 FROM registration_items ri WHERE ri.event_item_uuid = ei.uuid) LIMIT 1"
        ),
    );
    let page = body(app.get(&format!("/event-items/{used}"), Some(&staff)).await).await;
    assert!(page.contains("Registros con esta actividad") && page.contains("/registrations/"));
    let res = form_delete(&app, &format!("/event-items/{used}"), &staff).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert!(body(res).await.contains(&format!("data-open=\"dlg-quitar-act-{used}\"")));
    let res = form_delete(&app, &format!("/event-items/{item}"), &staff).await;
    assert_eq!((res.status(), location(&res)), (StatusCode::SEE_OTHER, format!("/events/{ev}?tab=items")));
    assert_eq!(app.count(&format!("SELECT COUNT(*) FROM event_items WHERE uuid = '{item}'")), 0);
}

/// OP-SPN-01: inline add, UI-11 autosave (money, category tag out of band, logo image only), delete (AP-06).
#[tokio::test]
async fn spn01_sponsors() {
    let app = App::new();
    let staff = app.staff().await;
    let ev = demo_event(&app);
    let url = format!("/events/{ev}/sponsors");
    let res = app
        .post_form(
            &url,
            Some(&staff),
            &format!("_csrf={}&sponsor_name=Nuevo+Demo&type=Oro&requested_amount=abc", staff.csrf),
        )
        .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(app.count("SELECT COUNT(*) FROM sponsor WHERE sponsor_name = 'Nuevo Demo'"), 0);
    let res = app
        .post_form(
            &url,
            Some(&staff),
            &format!("_csrf={}&sponsor_name=Nuevo+Demo&type=Oro&contact_name=Ana&requested_amount=1%2C000", staff.csrf),
        )
        .await;
    assert_eq!((res.status(), location(&res)), (StatusCode::SEE_OTHER, format!("/events/{ev}?tab=sponsors")));
    let (s, contact, req): (String, String, i64) = app
        .conn()
        .query_row(
            "SELECT uuid, contact_name, requested_amount FROM sponsor WHERE sponsor_name = 'Nuevo Demo'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((contact.as_str(), req), ("Ana", 100_000));

    assert_eq!(app.get(&format!("/sponsors/{s}"), Some(&staff)).await.status(), StatusCode::OK);
    let surl = format!("/sponsors/{s}");
    let res = app.patch(&surl, &staff, "promised=abc", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let res = app.patch(&surl, &staff, "promised=1500", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body(res).await.contains("value=\"1,500.00\""));
    let res = app.patch(&surl, &staff, "type=Platino", true).await;
    assert!(body(res).await.contains("innerHTML:#sp-tag"));
    let pdf = stored_file(&app, "application/pdf", "x.pdf");
    let res = app.patch(&surl, &staff, &format!("logo_file_uuid={pdf}"), true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let png = stored_file(&app, "image/png", "logo.png");
    let res = app.patch(&surl, &staff, &format!("logo_file_uuid={png}"), true).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body(res).await.contains("logo.png"));

    let res = form_delete(&app, &surl, &staff).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(app.count(&format!("SELECT COUNT(*) FROM sponsor WHERE uuid = '{s}'")), 0);
}

/// OP-SPN-02: billing upload (PDF only, D-16) and remove (AP-06).
#[tokio::test]
async fn spn02_billing_files() {
    let app = App::new();
    let staff = app.staff().await;
    let ev = demo_event(&app);
    let n = app.count(&format!("SELECT COUNT(*) FROM billing WHERE event_uuid = '{ev}'"));
    let res = upload(&app, &format!("/events/{ev}/billing"), &staff, "notas.png", b"\x89PNGxx").await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body(res).await.contains("Use un archivo PDF."));
    let res = upload(&app, &format!("/events/{ev}/billing"), &staff, "Lista_2027-05-10.pdf", b"%PDF-1.4 demo").await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body(res).await;
    assert!(html.contains("PDF subido") && html.contains("Lista_2027-05-10.pdf") && !html.contains("<html"));
    assert_eq!(app.count(&format!("SELECT COUNT(*) FROM billing WHERE event_uuid = '{ev}'")), n + 1);
    let b = one(&app, &format!("SELECT uuid FROM billing WHERE event_uuid = '{ev}' ORDER BY timestamp DESC LIMIT 1"));
    let res = form_delete(&app, &format!("/billing/{b}"), &staff).await;
    assert_eq!((res.status(), location(&res)), (StatusCode::SEE_OTHER, format!("/events/{ev}?tab=billing")));
    assert_eq!(app.count(&format!("SELECT COUNT(*) FROM billing WHERE event_uuid = '{ev}'")), n);
}
