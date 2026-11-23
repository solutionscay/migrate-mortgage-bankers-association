//! I2 phase 2, registrations pane: route-level tests of UI-05, UI-06 and UI-07 (OP-REG-01..10). Route versions of
//! T-05-03, T-05-04, T-05-06, T-05-10, T-05-13, T-05-14, T-05-15, plus BR-REG-01, BR-REG-06, AP-05, AP-06 and the
//! door check-in.

#[path = "autosave_support.rs"]
mod support;

use axum::http::StatusCode;
use mba_server::auth::Role;
use mba_server::convention::{begin, registrations as regs, Who as DWho};
use support::{body, location, App, Who};

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

impl App {
    fn uuid_of(&self, number: i64) -> String {
        self.conn()
            .query_row("SELECT uuid FROM registration WHERE registration_number = ?1", [number], |r| r.get(0))
            .unwrap()
    }
    fn one(&self, sql: &str, arg: &str) -> String {
        self.conn().query_row(sql, [arg], |r| r.get(0)).unwrap()
    }
    fn one0(&self, sql: &str) -> String {
        self.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }
    fn num(&self, sql: &str, arg: &str) -> i64 {
        self.conn().query_row(sql, [arg], |r| r.get(0)).unwrap()
    }
    /// A plain dialog form post with the CSRF token in the body.
    async fn form(&self, path: &str, who: &Who, fields: &[(&str, &str)]) -> axum::http::Response<axum::body::Body> {
        let mut b = format!("_csrf={}", enc(&who.csrf));
        for (k, v) in fields {
            b.push_str(&format!("&{k}={}", enc(v)));
        }
        self.post_form(path, Some(who), &b).await
    }
    /// A registration of María (#1283's person) with no event, created through the domain.
    fn new_registration(&self) -> String {
        let mut conn = self.conn();
        let person = conn
            .query_row("SELECT person_uuid FROM registration WHERE registration_number = 1283", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap();
        let tx = begin(&mut conn).unwrap();
        let r = regs::create_registration(&tx, &DWho::new(3, "equintana", Role::Staff), &person).unwrap();
        tx.commit().unwrap();
        r
    }
    fn item_of(&self, reg: &str, item_type: &str) -> String {
        self.conn()
            .query_row(
                "SELECT ri.uuid FROM registration_items ri JOIN event_items ei ON ei.uuid = ri.event_item_uuid
                  WHERE ri.registration_uuid = ?1 AND ei.type = ?2 ORDER BY ei.date_start LIMIT 1",
                [reg, item_type],
                |r| r.get(0),
            )
            .unwrap()
    }
    fn field_changes(&self) -> i64 {
        self.count("SELECT COUNT(*) FROM audit_log WHERE action = 'field.change'")
    }
}

/// UI-05, UI-06, UI-07 open for both roles; a registration number also opens UI-06; the default list event is the
/// first Activo event; filters apply.
#[tokio::test]
async fn pages_open_for_both_roles() {
    let app = App::new();
    let staff = app.staff().await;
    let admin = app.admin().await;
    for who in [&staff, &admin] {
        let res = app.get("/registrations", Some(who)).await;
        assert_eq!(res.status(), StatusCode::OK);
        let html = body(res).await;
        assert!(html.contains("Convención Demo 2027") && html.contains("María Ejemplo Demo"));
        let res = app.get("/registrations/1283", Some(who)).await;
        assert_eq!(res.status(), StatusCode::OK);
        let html = body(res).await;
        assert!(html.contains("Registro</b> #1283") && html.contains("id=\"totales\"") && html.contains("$1,218.00"));
    }
    // AP-05: the void menu and the delete dialog are rendered for ROLE-ADMIN only.
    let staff_html = body(app.get("/registrations/1283", Some(&staff)).await).await;
    assert!(!staff_html.contains("dlg-anular-") && !staff_html.contains("id=\"dlg-borrar\""));
    let admin_html = body(app.get("/registrations/1283", Some(&admin)).await).await;
    assert!(admin_html.contains("dlg-anular-") && admin_html.contains("id=\"dlg-borrar\""));

    let canc = body(app.get("/registrations?event=todos&status=Cancel%C3%B3", Some(&staff)).await).await;
    assert!(canc.contains("Canceló") && !canc.contains(">Nuevo</span>"));
    let q = body(app.get("/registrations?q=Fulano", Some(&staff)).await).await;
    assert!(q.contains("Fulano de Tal") && !q.contains("María Ejemplo Demo"));

    let event = app.one("SELECT uuid FROM events WHERE name = ?1", "Convención Demo 2027");
    let res = app.get(&format!("/events/{event}/door"), Some(&staff)).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body(res).await.contains("Estatus en puerta de #1283"));
    assert_eq!(app.get("/registrations/nada", Some(&staff)).await.status(), StatusCode::NOT_FOUND);
    // AP-01: no session -> sign-in page.
    assert_eq!(app.get("/registrations", None).await.status(), StatusCode::SEE_OTHER);
}

/// T-05-03: a plan change without `confirm` answers 200 with the confirm dialog open and changes nothing; with
/// `confirm` the items are replaced (303).
#[tokio::test]
async fn t05_03_plan_change_needs_confirm() {
    let app = App::new();
    let staff = app.staff().await;
    let reg = app.uuid_of(1283);
    let event = app.one("SELECT event_uuid FROM registration WHERE uuid = ?1", &reg);
    let p1 = app.one("SELECT uuid FROM packages WHERE event_uuid = ?1 AND name LIKE 'P1%'", &event);
    let before = app.num("SELECT COUNT(*) FROM registration_items WHERE registration_uuid = ?1", &reg);

    let res = app.form(&format!("/registrations/{reg}/plan"), &staff, &[("plan_uuid", &p1)]).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body(res).await;
    assert!(html.contains(&format!("data-open=\"dlg-plan-confirm-{p1}\"")));
    assert_eq!(app.num("SELECT COUNT(*) FROM registration_items WHERE registration_uuid = ?1", &reg), before);
    assert_ne!(app.one("SELECT package_uuid FROM registration WHERE uuid = ?1", &reg), p1);

    let res = app.form(&format!("/registrations/{reg}/plan"), &staff, &[("plan_uuid", &p1), ("confirm", "1")]).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert!(location(&res).contains("hecho=plan"));
    assert_eq!(app.one("SELECT package_uuid FROM registration WHERE uuid = ?1", &reg), p1);
    let active = app.num("SELECT COUNT(*) FROM package_items WHERE package_uuid = ?1 AND active = 1", &p1);
    assert_eq!(app.num("SELECT COUNT(*) FROM registration_items WHERE registration_uuid = ?1", &reg), active);
}

/// T-05-04 and AP-05: delete with a payment -> 409 with the cancel dialog open; staff -> 403; admin without
/// payments -> 303 and gone; cancel -> Canceló for both roles.
#[tokio::test]
async fn t05_04_delete_with_payment_offers_cancel() {
    let app = App::new();
    let staff = app.staff().await;
    let admin = app.admin().await;
    let reg = app.uuid_of(1283);
    let res = app.form(&format!("/registrations/{reg}"), &admin, &[("_method", "DELETE")]).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let html = body(res).await;
    assert!(html.contains("data-open=\"dlg-cancelar\"") && html.contains("puede cancelarlo"));
    assert_eq!(app.num("SELECT COUNT(*) FROM registration WHERE uuid = ?1", &reg), 1);

    let res = app.form(&format!("/registrations/{reg}/cancel"), &staff, &[]).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(app.one("SELECT status FROM registration WHERE uuid = ?1", &reg), "Canceló");

    let fresh = app.new_registration();
    let res = app.form(&format!("/registrations/{fresh}"), &staff, &[("_method", "DELETE")]).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let res = app.form(&format!("/registrations/{fresh}"), &admin, &[("_method", "DELETE")]).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&res), "/registrations?hecho=borrado");
    assert_eq!(app.num("SELECT COUNT(*) FROM registration WHERE uuid = ?1", &fresh), 0);
}

/// T-05-06: void as staff -> 403; admin without reason -> 422 with the dialog open; admin with reason -> 303,
/// voided, one `payment.void` audit row, balance up.
#[tokio::test]
async fn t05_06_void_payment() {
    let app = App::new();
    let staff = app.staff().await;
    let admin = app.admin().await;
    let reg = app.uuid_of(1283);
    let pay = app.one("SELECT uuid FROM payments WHERE registration_uuid = ?1 AND voided_at IS NULL", &reg);
    let url = format!("/payments/{pay}/void");
    assert_eq!(app.form(&url, &staff, &[("void_reason", "x")]).await.status(), StatusCode::FORBIDDEN);
    let res = app.form(&url, &admin, &[("void_reason", "  ")]).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = body(res).await;
    assert!(html.contains(&format!("data-open=\"dlg-anular-{pay}\"")) && html.contains("Escriba la razón"));
    assert_eq!(app.num("SELECT COUNT(*) FROM payments WHERE uuid = ?1 AND voided_at IS NOT NULL", &pay), 0);

    let res = app.form(&url, &admin, &[("void_reason", "Cheque devuelto")]).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(app.one("SELECT void_reason FROM payments WHERE uuid = ?1", &pay), "Cheque devuelto");
    assert_eq!(app.count("SELECT COUNT(*) FROM audit_log WHERE action = 'payment.void'"), 1);
    assert_eq!(app.num("SELECT balance FROM v_registration_totals WHERE registration_uuid = ?1", &reg), 121_800);
    // Once only.
    assert_eq!(app.form(&url, &admin, &[("void_reason", "otra vez")]).await.status(), StatusCode::CONFLICT);
}

/// T-05-13: qty=3 on a line -> 200, the line with its new total and `#totales` out of band, one field.change row.
/// T-05-14: qty=0 -> 422, the typed value and the message, nothing written, totals unchanged.
#[tokio::test]
async fn t05_13_14_line_patch() {
    let app = App::new();
    let staff = app.staff().await;
    let reg = app.uuid_of(1283);
    // A line that is not from the plan, so its total is qty x price (BR-REG-05).
    let event = app.one("SELECT event_uuid FROM registration WHERE uuid = ?1", &reg);
    let ei = app.one("SELECT uuid FROM event_items WHERE event_uuid = ?1 AND price > 0 ORDER BY price LIMIT 1", &event);
    let ei_price = app.num("SELECT price FROM event_items WHERE uuid = ?1", &ei);
    let res = app.form(&format!("/registrations/{reg}/items"), &staff, &[("event_item_uuid", &ei)]).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let item = app.one("SELECT uuid FROM registration_items WHERE registration_uuid = ?1 AND package_item_uuid IS NULL ORDER BY created_at DESC LIMIT 1", &reg);
    let url = format!("/registrations/{reg}/items/{item}");
    let total_before = app.num("SELECT total FROM v_registration_totals WHERE registration_uuid = ?1", &reg);
    let audits = app.field_changes();

    let res = app.patch(&url, &staff, "qty=3", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body(res).await;
    assert!(html.contains(&format!("id=\"li-{item}\"")) && html.contains("name=\"qty\" value=\"3\""));
    assert!(html.contains(&mba_server::seed::usd(3 * ei_price)));
    assert!(html.contains("hx-swap-oob=\"innerHTML:#totales\""));
    assert_eq!(app.field_changes(), audits + 1);
    assert_eq!(app.num("SELECT qty_manual FROM registration_items WHERE uuid = ?1", &item), 1);
    let total = app.num("SELECT total FROM v_registration_totals WHERE registration_uuid = ?1", &reg);

    let res = app.patch(&url, &staff, "qty=0", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = body(res).await;
    assert!(html.contains("value=\"0\"") && html.contains("aria-invalid=\"true\"") && html.contains("row-error"));
    assert!(html.contains("data-unsaved") && html.contains("No se guardó") && !html.contains("hx-swap-oob"));
    assert_eq!(app.num("SELECT ticket_qty FROM registration_items WHERE uuid = ?1", &item), 3);
    assert_eq!(app.num("SELECT total FROM v_registration_totals WHERE registration_uuid = ?1", &reg), total);
    assert_eq!(app.field_changes(), audits + 1);
    assert!(total > total_before);

    // Price and no charge go through the same route.
    assert_eq!(app.patch(&url, &staff, "price=12.50", true).await.status(), StatusCode::OK);
    assert_eq!(app.num("SELECT price FROM registration_items WHERE uuid = ?1", &item), 1250);
    assert_eq!(app.patch(&url, &staff, "no_charge=1", true).await.status(), StatusCode::OK);
    assert_eq!(app.num("SELECT total FROM v_registration_item_lines WHERE uuid = ?1", &item), 0);
    // Another registration's item -> 404 (scope fixed by the server).
    let other = app.uuid_of(1284);
    assert_eq!(
        app.patch(&format!("/registrations/{other}/items/{item}"), &staff, "qty=2", true).await.status(),
        StatusCode::NOT_FOUND
    );
    // AP-02: no CSRF -> 403.
    assert_eq!(app.patch(&url, &staff, "qty=2", false).await.status(), StatusCode::FORBIDDEN);
}

/// T-05-15 on the registration routes: two fields or an unknown field -> 422, nothing written.
#[tokio::test]
async fn t05_15_one_field_per_patch() {
    let app = App::new();
    let staff = app.staff().await;
    let reg = app.uuid_of(1283);
    let item = app.item_of(&reg, "Cena");
    let audits = app.field_changes();
    for (url, b) in [
        (format!("/registrations/{reg}"), "status=Registrado&comments=x"),
        (format!("/registrations/{reg}"), "registration_number=5"),
        (format!("/registrations/{reg}/items/{item}"), "qty=2&price=1"),
        (format!("/registrations/{reg}/items/{item}"), ""),
    ] {
        let res = app.patch(&url, &staff, b, true).await;
        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY, "{b}");
    }
    assert_eq!(app.one("SELECT status FROM registration WHERE uuid = ?1", &reg), "Nuevo");
    assert_eq!(app.field_changes(), audits);

    // Header fields save one at a time.
    let res = app.patch(&format!("/registrations/{reg}"), &staff, "status=Registrado", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body(res).await.contains("data-saved=\"Registrado\""));
    let res = app.patch(&format!("/registrations/{reg}"), &staff, "costeado_por=Individuo", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body(res).await.contains("value=\"Individuo\" checked"));
    let res = app.patch(&format!("/registrations/{reg}"), &staff, "costeado_por=Otro", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(app.one("SELECT costeado_por FROM registration WHERE uuid = ?1", &reg), "Individuo");
}

/// T-05-10: a qty PATCH sets `qty_manual`; a guest add then changes only the other items (D-88); guest PATCH and
/// DELETE recompute too.
#[tokio::test]
async fn t05_10_guest_add_keeps_hand_set_qty() {
    let app = App::new();
    let staff = app.staff().await;
    let reg = app.uuid_of(1283);
    let cena = app.item_of(&reg, "Cena");
    assert_eq!(
        app.patch(&format!("/registrations/{reg}/items/{cena}"), &staff, "qty=5", true).await.status(),
        StatusCode::OK
    );
    let guests = app.num("SELECT COUNT(*) FROM registration_guests WHERE registration_uuid = ?1", &reg);

    let res = app
        .form(&format!("/registrations/{reg}/guests"), &staff, &[("name_first", "Rosa"), ("name_last", "Ejemplo")])
        .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(app.num("SELECT ticket_qty FROM registration_items WHERE uuid = ?1", &cena), 5);
    let others = app.num(
        "SELECT COUNT(*) FROM registration_items WHERE registration_uuid = ?1 AND qty_manual = 0 AND ticket_qty <> (SELECT COUNT(*) FROM registration_guests WHERE registration_uuid = ?1) + 1",
        &reg,
    );
    assert_eq!(others, 0);
    let guest =
        app.one("SELECT uuid FROM registration_guests WHERE registration_uuid = ?1 AND name_first = 'Rosa'", &reg);
    let hotel = app.item_of(&reg, "Hotel");
    assert_eq!(app.num("SELECT ticket_qty FROM registration_items WHERE uuid = ?1", &hotel), guests + 2);

    let res = app.patch(&format!("/registrations/{reg}/guests/{guest}"), &staff, "name_badge=Rosita", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body(res).await.contains("data-saved=\"Rosita\""));
    // An empty guest -> 422, nothing added.
    let res = app.form(&format!("/registrations/{reg}/guests"), &staff, &[("name_first", "")]).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let res = app.form(&format!("/registrations/{reg}/guests/{guest}"), &staff, &[("_method", "DELETE")]).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(app.num("SELECT ticket_qty FROM registration_items WHERE uuid = ?1", &hotel), guests + 1);
    assert_eq!(app.num("SELECT ticket_qty FROM registration_items WHERE uuid = ?1", &cena), 5);
}

/// BR-REG-01: the event picker opens only while the registration has no event and lists active events only; the
/// event is set once.
#[tokio::test]
async fn br_reg_01_event_picker_route() {
    let app = App::new();
    let staff = app.staff().await;
    let set = app.uuid_of(1283);
    assert_eq!(
        app.get(&format!("/registrations/{set}/event-picker"), Some(&staff)).await.status(),
        StatusCode::CONFLICT
    );
    let fresh = app.new_registration();
    let res = app.get(&format!("/registrations/{fresh}/event-picker"), Some(&staff)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body(res).await;
    assert!(html.contains("Convención Demo 2027") && !html.contains("Convención Demo 2026"));
    // No plan before the event.
    assert_eq!(
        app.get(&format!("/registrations/{fresh}/plan-picker"), Some(&staff)).await.status(),
        StatusCode::CONFLICT
    );

    let inactive = app.one0("SELECT uuid FROM events WHERE status = 'Inactivo' LIMIT 1");
    let res = app.form(&format!("/registrations/{fresh}/event"), &staff, &[("event_uuid", &inactive)]).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let event = app.one("SELECT uuid FROM events WHERE name = ?1", "Convención Demo 2027");
    let res = app.form(&format!("/registrations/{fresh}/event"), &staff, &[("event_uuid", &event)]).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(app.one("SELECT event_uuid FROM registration WHERE uuid = ?1", &fresh), event);
    let res = app.form(&format!("/registrations/{fresh}/event"), &staff, &[("event_uuid", &event)]).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    // The page now offers the plan picker of that event, and the empty items state.
    let html = body(app.get(&format!("/registrations/{fresh}"), Some(&staff)).await).await;
    assert!(
        html.contains("id=\"dlg-plan\"") && html.contains("Sin actividades") && !html.contains("id=\"dlg-evento\"")
    );
    assert_eq!(app.get(&format!("/registrations/{fresh}/plan-picker"), Some(&staff)).await.status(), StatusCode::OK);
}

/// BR-REG-06 on the routes: opening the dialog writes nothing; a seating field on a hotel item is refused; Aceptar
/// saves; items without attributes have no dialog.
#[tokio::test]
async fn br_reg_06_attributes_route() {
    let app = App::new();
    let staff = app.staff().await;
    let reg = app.uuid_of(1283);
    let hotel = app.item_of(&reg, "Hotel");
    let stamp = app.one("SELECT modified_at FROM registration_items WHERE uuid = ?1", &hotel);
    let audits = app.count("SELECT COUNT(*) FROM audit_log");
    let res = app.get(&format!("/registrations/{reg}/items/{hotel}/attributes"), Some(&staff)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body(res).await;
    assert!(
        html.contains("name=\"hotel_reservation_num\"")
            && html.contains(">Cancelar<")
            && !html.contains("name=\"mesa\"")
    );
    assert_eq!(app.one("SELECT modified_at FROM registration_items WHERE uuid = ?1", &hotel), stamp);
    assert_eq!(app.count("SELECT COUNT(*) FROM audit_log"), audits);

    let url = format!("/registrations/{reg}/items/{hotel}/attributes");
    let res = app.form(&url, &staff, &[("mesa", "4")]).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body(res).await.contains(&format!("data-open=\"dlg-attr-{hotel}\"")));
    let res = app.form(&url, &staff, &[("hotel_reservation_num", "DEMO-77"), ("adicional", "2")]).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(app.one("SELECT hotel_reservation_num FROM registration_items WHERE uuid = ?1", &hotel), "DEMO-77");
    assert_eq!(app.one("SELECT adicional FROM registration_items WHERE uuid = ?1", &hotel), "2");

    let cocktail = app.item_of(&reg, "Cocktail");
    assert_eq!(
        app.get(&format!("/registrations/{reg}/items/{cocktail}/attributes"), Some(&staff)).await.status(),
        StatusCode::NOT_FOUND
    );
}

/// OP-REG-04 and AP-06: add an item of the same event; removal needs `confirm` (200 with the dialog open, then 303).
#[tokio::test]
async fn items_add_and_remove_with_confirm() {
    let app = App::new();
    let staff = app.staff().await;
    let reg = app.uuid_of(1283);
    let foreign = app.one0("SELECT ei.uuid FROM event_items ei JOIN events e ON e.uuid = ei.event_uuid WHERE e.name <> 'Convención Demo 2027' LIMIT 1");
    let res = app.form(&format!("/registrations/{reg}/items"), &staff, &[("event_item_uuid", &foreign)]).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);

    let cena = app.item_of(&reg, "Cena");
    let url = format!("/registrations/{reg}/items/{cena}");
    let res = app.form(&url, &staff, &[("_method", "DELETE")]).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body(res).await.contains(&format!("data-open=\"dlg-quitar-{cena}\"")));
    assert_eq!(app.num("SELECT COUNT(*) FROM registration_items WHERE uuid = ?1", &cena), 1);
    let res = app.form(&url, &staff, &[("_method", "DELETE"), ("confirm", "1")]).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(app.num("SELECT COUNT(*) FROM registration_items WHERE uuid = ?1", &cena), 0);
}

/// OP-REG-07: amount 0 -> 422 with the dialog open and the typed value; a valid payment -> 303, balance down.
#[tokio::test]
async fn payments_dialog() {
    let app = App::new();
    let staff = app.staff().await;
    let reg = app.uuid_of(1283);
    let url = format!("/registrations/{reg}/payments");
    let res =
        app.form(&url, &staff, &[("payment", "0"), ("payment_type", "Cheque"), ("payment_number", "DEMO-03")]).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = body(res).await;
    assert!(
        html.contains("data-open=\"dlg-pago\"")
            && html.contains("mayor que $0.00")
            && html.contains("value=\"DEMO-03\"")
    );
    let res = app
        .form(&url, &staff, &[("payment", "718.00"), ("payment_type", "Cheque"), ("payment_number", "DEMO-03")])
        .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(app.num("SELECT balance FROM v_registration_totals WHERE registration_uuid = ?1", &reg), 0);
}

/// OP-REG-10: door buttons. No Asistió is stored as No Canceló and shown pressed again; htmx gets the row and the
/// attended count out of band; a bad value is 422; the search filters the rows.
#[tokio::test]
async fn door_check_in() {
    let app = App::new();
    let staff = app.staff().await;
    let reg = app.uuid_of(1290);
    let event = app.one("SELECT event_uuid FROM registration WHERE uuid = ?1", &reg);
    let url = format!("/registrations/{reg}/attendance");
    let res = app.form(&url, &staff, &[("status", "No Asistió")]).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&res), format!("/events/{event}/door"));
    assert_eq!(app.one("SELECT status FROM registration WHERE uuid = ?1", &reg), "No Canceló");

    let req = axum::http::Request::post(&url)
        .header("content-type", "application/x-www-form-urlencoded")
        .header("cookie", &staff.cookie)
        .header("hx-request", "true")
        .header("x-csrf-token", &staff.csrf)
        .body(axum::body::Body::from(format!("status={}", enc("Asistió"))))
        .unwrap();
    let res = app.send(req).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body(res).await;
    assert!(html.starts_with(&format!("<tr id=\"door-{reg}\"")));
    assert!(
        html.contains("value=\"Asistió\" aria-pressed=\"true\"") && html.contains("id=\"door-attended\" hx-swap-oob")
    );
    assert_eq!(app.form(&url, &staff, &[("status", "Tal vez")]).await.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let html = body(app.get(&format!("/events/{event}/door?q=Fulano"), Some(&staff)).await).await;
    assert!(html.contains("Fulano de Tal") && !html.contains("Estatus en puerta de #1283"));
    // AP-02 on a form post: no CSRF -> 403.
    assert_eq!(app.post_form(&url, Some(&staff), "status=Cancel%C3%B3").await.status(), StatusCode::FORBIDDEN);
}

/// UI-04 "Ver registros" (`/registrations?q=<name>`): a search with no event given looks in every event, so a
/// registration of a past (Inactivo) event shows; without `q` the list still opens on the first active event.
#[tokio::test]
async fn search_without_event_looks_in_every_event() {
    let app = App::new();
    let staff = app.staff().await;
    let (name, number): (String, i64) = app
        .conn()
        .query_row(
            "SELECT TRIM(COALESCE(p.name_first, '') || ' ' || COALESCE(p.name_last, '')), r.registration_number
               FROM registration r JOIN people p ON p.uuid = r.person_uuid JOIN events e ON e.uuid = r.event_uuid
              WHERE e.status = 'Inactivo' ORDER BY r.registration_number LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let html = body(app.get(&format!("/registrations?q={}", enc(&name)), Some(&staff)).await).await;
    assert!(html.contains(&number.to_string()), "#{number} of {name} in the search result");
    let plain = body(app.get("/registrations", Some(&staff)).await).await;
    assert!(!plain.contains(&format!(">{number}<")), "the plain list opens on the active event");
}
