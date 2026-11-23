//! Route-level tests of UI-03, UI-04, UI-12 and UI-13 (OP-PPL-01, OP-PPL-02, OP-REG-01 from the person page,
//! OP-EVT-04, OP-EVT-05): status codes, AP-03/AP-04/AP-06 checks, autosave 200/422 with the out-of-band fragments.

#[path = "autosave_support.rs"]
mod support;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use support::{body, location, App, Who};

fn one(app: &App, sql: &str) -> String {
    app.conn().query_row(sql, [], |r| r.get(0)).unwrap()
}

fn maria(app: &App) -> String {
    one(app, "SELECT uuid FROM people WHERE email = 'maria@ejemplo.com'")
}

fn plan(app: &App, prefix: &str) -> String {
    one(app, &format!("SELECT uuid FROM packages WHERE name LIKE '{prefix}%' ORDER BY name LIMIT 1"))
}

async fn delete(app: &App, path: &str, who: &Who) -> axum::http::Response<Body> {
    app.post_form(path, Some(who), &format!("_method=DELETE&_csrf={}", who.csrf)).await
}

async fn hx_get(app: &App, path: &str, who: &Who, target: &str) -> axum::http::Response<Body> {
    let req = Request::get(path)
        .header(header::COOKIE, &who.cookie)
        .header("hx-request", "true")
        .header("hx-target", target)
        .body(Body::empty())
        .unwrap();
    app.send(req).await
}

/// UI-03: list, search, company filter, no results, the htmx results fragment, and AP-01.
#[tokio::test]
async fn ui03_people_list_search_and_states() {
    let app = App::new();
    assert_eq!(app.get("/people", None).await.status(), StatusCode::SEE_OTHER);
    let staff = app.staff().await;
    let res = app.get("/people", Some(&staff)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body(res).await;
    assert!(html.contains("<b>Convencionistas</b> <span class=\"sub\">· 60</span>"), "count of all people");
    assert!(html.contains("María Ejemplo Demo") && html.contains("1–50 de 60"));
    assert!(!html.contains("Borrar convencionista"), "staff sees no delete (AP-04)");

    let html = body(app.get("/people?q=maria%40ejemplo.com", Some(&staff)).await).await;
    assert!(html.contains("María Ejemplo Demo") && html.contains("1–1 de 1"));
    let html = body(app.get("/people?q=zzz", Some(&staff)).await).await;
    assert!(html.contains("Sin resultados"));
    let html = body(app.get("/people?company=Cooperativa+Muestra&view=list", Some(&staff)).await).await;
    assert!(
        html.contains("Pedro Prueba Demo") && !html.contains("Fulano de Tal") && !html.contains("<th>Teléfono</th>")
    );

    let frag = body(hx_get(&app, "/people?q=Pedro", &staff, "people-results").await).await;
    assert!(frag.starts_with("<div id=\"people-results\">") && !frag.contains("<html"));
}

/// OP-PPL-01 create: the dialog form, 422 without a name, 403 without CSRF.
#[tokio::test]
async fn op_ppl_01_create_person() {
    let app = App::new();
    let staff = app.staff().await;
    assert!(body(app.get("/people/new", Some(&staff)).await).await.contains("data-open=\"dlg-nuevo\""));
    let res =
        app.post_form("/people", Some(&staff), &format!("_csrf={}&name_first=&name_last=&company=X", staff.csrf)).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body(res).await.contains("Escriba el nombre o el apellido."));
    let res = app.post_form("/people", Some(&staff), "name_first=Ana").await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let before = app.count("SELECT COUNT(*) FROM people");
    let res = app
        .post_form(
            "/people",
            Some(&staff),
            &format!("_csrf={}&name_first=Ana&name_last=Nueva+Demo&company=Banco+Ejemplo", staff.csrf),
        )
        .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let uuid = location(&res).trim_start_matches("/people/").to_owned();
    assert_eq!(app.count("SELECT COUNT(*) FROM people"), before + 1);
    let html = body(app.get(&format!("/people/{uuid}"), Some(&staff)).await).await;
    assert!(html.contains("Convencionista</b> · Ana Nueva Demo") && html.contains("Sin registros"));
}

/// UI-04 autosave: 200 with the saved value and one audit row; 422 with the typed value and nothing written;
/// two fields or an unknown field 422 (T-05-15); a name edit returns the title out of band; no CSRF 403.
#[tokio::test]
async fn ui04_person_autosave() {
    let app = App::new();
    let staff = app.staff().await;
    let id = maria(&app);
    let url = format!("/people/{id}");
    let html = body(app.get(&url, Some(&staff)).await).await;
    assert!(html.contains("$1,218.00") && html.contains("Convención Demo 2027") && html.contains("list=\"dl-comp\""));

    let res = app.patch(&url, &staff, "phone2=787-555-0999", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body(res).await.contains("data-saved=\"787-555-0999\""));
    assert_eq!(app.count("SELECT COUNT(*) FROM audit_log WHERE action = 'field.change' AND entity = 'people'"), 1);

    let res = app.patch(&url, &staff, "email=maria%40ejemplo", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let frag = body(res).await;
    assert!(
        frag.contains("data-unsaved")
            && frag.contains("value=\"maria@ejemplo\"")
            && frag.contains("nombre@ejemplo.com")
    );
    assert_eq!(one(&app, &format!("SELECT email FROM people WHERE uuid = '{id}'")), "maria@ejemplo.com");

    assert_eq!(app.patch(&url, &staff, "phone1=1&fax=2", true).await.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(app.patch(&url, &staff, "uuid=x", true).await.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(app.patch(&url, &staff, "phone1=1", false).await.status(), StatusCode::FORBIDDEN);

    let res = app.patch(&url, &staff, "name_last=Ejemplo+Cambiado", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body(res).await.contains("hx-swap-oob=\"innerHTML:#person-title\""));
    // A person keeps a first or a last name.
    assert_eq!(app.patch(&url, &staff, "name_last=", true).await.status(), StatusCode::OK);
    let res = app.patch(&url, &staff, "name_first=", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body(res).await.contains("Escriba el nombre o el apellido."));

    let res = app.patch(&url, &staff, "company=Banco+Ejemplo", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body(res).await.contains("<datalist id=\"dl-comp\">"));
    assert_eq!(app.patch("/people/nope", &staff, "fax=1", true).await.status(), StatusCode::NOT_FOUND);
}

/// OP-PPL-02 / AP-04: staff 403; admin with registrations 409 with the refusal dialog; admin without 303.
/// OP-REG-01 from UI-04: a new registration, status Nuevo, redirect to UI-06.
#[tokio::test]
async fn op_ppl_02_delete_and_add_registration() {
    let app = App::new();
    let staff = app.staff().await;
    let admin = app.admin().await;
    let id = maria(&app);
    assert_eq!(delete(&app, &format!("/people/{id}"), &staff).await.status(), StatusCode::FORBIDDEN);
    let res = delete(&app, &format!("/people/{id}"), &admin).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let html = body(res).await;
    assert!(html.contains("data-open=\"dlg-borrar\"") && html.contains("No se puede borrar"));

    let lone = one(&app, "SELECT uuid FROM people p WHERE NOT EXISTS (SELECT 1 FROM registration r WHERE r.person_uuid = p.uuid) LIMIT 1");
    let res = delete(&app, &format!("/people/{lone}"), &admin).await;
    assert_eq!((res.status(), location(&res).as_str()), (StatusCode::SEE_OTHER, "/people?hecho=borrado"));
    assert_eq!(app.count(&format!("SELECT COUNT(*) FROM people WHERE uuid = '{lone}'")), 0);

    let before = app.count("SELECT MAX(registration_number) FROM registration");
    let res =
        app.post_form(&format!("/people/{id}/registrations"), Some(&staff), &format!("_csrf={}", staff.csrf)).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let reg = location(&res).trim_start_matches("/registrations/").to_owned();
    let (n, status): (i64, String) = app
        .conn()
        .query_row("SELECT registration_number, status FROM registration WHERE uuid = ?1", [&reg], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!((n, status.as_str()), (before + 1, "Nuevo"));
}

/// UI-12: event filter (default the first active event, empty = all), price and its origin from `v_package_price`.
/// OP-EVT-05 create: active event only; the plan items are built (OP-EVT-04).
#[tokio::test]
async fn ui12_plans_list_and_create() {
    let app = App::new();
    let staff = app.staff().await;
    let html = body(app.get("/plans", Some(&staff)).await).await;
    assert!(html.contains("· Convención Demo 2027") && html.contains("$1,640.00") && html.contains("Precio fijo"));
    assert!(html.contains("$1,218.00") && !html.contains("F1 · Foro completo"));
    let html = body(app.get("/plans?event=", Some(&staff)).await).await;
    assert!(html.contains("F1 · Foro completo") && html.contains("Todos los eventos"));
    let html = body(app.get("/plans?event=&q=P3", Some(&staff)).await).await;
    assert!(html.contains("P3 · Solo seminarios") && !html.contains("P1 · Completo"));

    let active = one(&app, "SELECT uuid FROM events WHERE name = 'Convención Demo 2027'");
    let inactive = one(&app, "SELECT uuid FROM events WHERE status = 'Inactivo' LIMIT 1");
    let res =
        app.post_form("/plans", Some(&staff), &format!("_csrf={}&event_uuid={inactive}&name=X", staff.csrf)).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body(res).await.contains("Escoja un evento activo."));
    let res = app.post_form("/plans", Some(&staff), &format!("_csrf={}&event_uuid={active}&name=", staff.csrf)).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let res = app
        .post_form("/plans", Some(&staff), &format!("_csrf={}&event_uuid={active}&name=P9+Prueba", staff.csrf))
        .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let id = location(&res).trim_start_matches("/plans/").to_owned();
    let items = app.count(&format!("SELECT COUNT(*) FROM package_items WHERE package_uuid = '{id}' AND active = 0"));
    assert_eq!(items, app.count(&format!("SELECT COUNT(*) FROM event_items WHERE event_uuid = '{active}'")));
}

/// UI-13 header autosave: price override returns `#plan-price` out of band (empty = computed again, BR-EVT-05);
/// name returns the title; an inactive event 422; an event change builds the new event's items and reloads.
#[tokio::test]
async fn ui13_plan_fields() {
    let app = App::new();
    let staff = app.staff().await;
    let id = plan(&app, "P2(B)");
    let url = format!("/plans/{id}");
    let html = body(app.get(&url, Some(&staff)).await).await;
    assert!(html.contains("$1,218.00") && html.contains("8 de 12") && html.contains("12 actividades del evento"));

    let res = app.patch(&url, &staff, "price_override=1500", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    let frag = body(res).await;
    assert!(
        frag.contains("data-saved=\"1,500.00\"")
            && frag.contains("innerHTML:#plan-price")
            && frag.contains("$1,500.00")
    );
    let frag = body(app.patch(&url, &staff, "price_override=", true).await).await;
    assert!(frag.contains("$1,218.00"));
    let res = app.patch(&url, &staff, "price_override=abc", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let frag = body(app.patch(&url, &staff, "name=P2(B)+·+marzo", true).await).await;
    assert!(frag.contains("innerHTML:#plan-title"));
    assert_eq!(app.patch(&url, &staff, "name=", true).await.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let inactive = one(&app, "SELECT uuid FROM events WHERE status = 'Inactivo' LIMIT 1");
    let res = app.patch(&url, &staff, &format!("event_uuid={inactive}"), true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let foro = one(&app, "SELECT uuid FROM events WHERE name = 'Foro de Otoño Demo 2027'");
    let before = app.count(&format!("SELECT COUNT(*) FROM package_items WHERE package_uuid = '{id}'"));
    let res = app.patch(&url, &staff, &format!("event_uuid={foro}"), true).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers().get("hx-refresh").map(|v| v.to_str().unwrap()), Some("true"));
    let foro_items = app.count(&format!("SELECT COUNT(*) FROM event_items WHERE event_uuid = '{foro}'"));
    // Phase-1 default: an event change only adds items.
    assert_eq!(
        app.count(&format!("SELECT COUNT(*) FROM package_items WHERE package_uuid = '{id}'")),
        before + foro_items
    );
}

/// UI-13 plan items: Incluir and qty return the row and the price out of band; qty 0 is 422 with the row error
/// and nothing written; remove (AP-06) and rebuild (OP-EVT-04, adds only the missing item).
#[tokio::test]
async fn ui13_plan_items_and_rebuild() {
    let app = App::new();
    let staff = app.staff().await;
    let id = plan(&app, "P2(B)");
    let off = one(
        &app,
        &format!(
            "SELECT pi.uuid FROM package_items pi JOIN event_items ei ON ei.uuid = pi.event_item_uuid
              WHERE pi.package_uuid = '{id}' AND pi.active = 0 AND pi.price > 0 ORDER BY pi.price LIMIT 1"
        ),
    );
    let price: i64 = app.count(&format!("SELECT price FROM package_items WHERE uuid = '{off}'"));
    let url = format!("/plan-items/{off}");
    let res = app.patch(&url, &staff, "active=1", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    let frag = body(res).await;
    assert!(
        frag.starts_with(&format!("<tbody id=\"pi-{off}\"><tr class=\"plan-on\""))
            && frag.contains("innerHTML:#plan-price")
    );
    assert!(frag.contains(&mba_server::seed::usd(121_800 + price)) && frag.contains("9 incluidas"));
    assert_eq!(app.count(&format!("SELECT price FROM v_package_price WHERE package_uuid = '{id}'")), 121_800 + price);

    let res = app.patch(&url, &staff, "ticket_qty=0", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let frag = body(res).await;
    assert!(frag.contains("row-error") && frag.contains("value=\"0\"") && !frag.contains("hx-swap-oob"));
    assert_eq!(app.count(&format!("SELECT ticket_qty FROM package_items WHERE uuid = '{off}'")), 1);
    let frag = body(app.patch(&url, &staff, "ticket_qty=2", true).await).await;
    assert!(frag.contains(&mba_server::seed::usd(121_800 + 2 * price)));
    assert_eq!(app.patch(&url, &staff, "price=1", true).await.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let res = delete(&app, &url, &staff).await;
    assert_eq!((res.status(), location(&res)), (StatusCode::SEE_OTHER, format!("/plans/{id}?hecho=quitado")));
    let html = body(app.get(&format!("/plans/{id}"), Some(&staff)).await).await;
    assert!(html.contains("Faltan 1: se añaden sin incluir."));
    let res = app.post_form(&format!("/plans/{id}/build"), Some(&staff), &format!("_csrf={}", staff.csrf)).await;
    assert_eq!(location(&res), format!("/plans/{id}?hecho=reconstruido&n=1"));
    let res = app.post_form(&format!("/plans/{id}/build"), Some(&staff), &format!("_csrf={}", staff.csrf)).await;
    assert_eq!(location(&res), format!("/plans/{id}?hecho=reconstruido&n=0"));
    assert!(body(app.get(&location(&res), Some(&staff)).await).await.contains("0 detalles añadidos."));
}

/// UI-13 states and plan delete (AP-04): an event without activities shows the BR-EVT-06 message and the rebuild
/// is refused (409); staff delete 403; admin delete with registrations 409, without 303 (items cascade).
#[tokio::test]
async fn ui13_states_and_delete() {
    let app = App::new();
    let staff = app.staff().await;
    let admin = app.admin().await;
    app.conn()
        .execute_batch(
            "INSERT INTO events (uuid, name, date_start, date_end, status) VALUES ('ev-x', 'Gala Demo 2028', '2028-03-10', '2028-03-10', 'Activo');",
        )
        .unwrap();
    let res = app.post_form("/plans", Some(&staff), &format!("_csrf={}&event_uuid=ev-x&name=G1", staff.csrf)).await;
    let empty = location(&res).trim_start_matches("/plans/").to_owned();
    let html = body(app.get(&format!("/plans/{empty}"), Some(&staff)).await).await;
    assert!(html.contains("El Evento seleccionado no tiene detalles.") && html.contains("Sin detalles"));
    let res = app.post_form(&format!("/plans/{empty}/build"), Some(&staff), &format!("_csrf={}", staff.csrf)).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);

    let used = plan(&app, "P2(B)");
    assert_eq!(delete(&app, &format!("/plans/{used}"), &staff).await.status(), StatusCode::FORBIDDEN);
    let res = delete(&app, &format!("/plans/{used}"), &admin).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert!(body(res).await.contains("data-open=\"dlg-borrar-plan\""));
    let p9 = app
        .post_form(
            "/plans",
            Some(&staff),
            &format!(
                "_csrf={}&event_uuid={}&name=P9",
                staff.csrf,
                one(
                    &app,
                    "SELECT event_uuid FROM packages WHERE uuid = (SELECT uuid FROM packages WHERE name LIKE 'P1%')"
                )
            ),
        )
        .await;
    let p9 = location(&p9).trim_start_matches("/plans/").to_owned();
    let res = delete(&app, &format!("/plans/{p9}"), &admin).await;
    assert_eq!((res.status(), location(&res).as_str()), (StatusCode::SEE_OTHER, "/plans?hecho=borrado"));
    assert_eq!(app.count(&format!("SELECT COUNT(*) FROM package_items WHERE package_uuid = '{p9}'")), 0);
    assert_eq!(app.get("/plans/nope", Some(&staff)).await.status(), StatusCode::NOT_FOUND);
}
