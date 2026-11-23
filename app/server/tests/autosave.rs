//! The per-field autosave contract (05 "Per-field autosave (PATCH)"): one field per request, 422 on 0 or more
//! fields (T-05-15), CSRF required, one `field.change` audit row, the 422 fragment, and T-05-10 / T-05-13 / T-05-14
//! (D-88 qty_manual) through the registration item registry that I2's item route will use.

#[path = "autosave_support.rs"]
mod support;

use axum::http::StatusCode;
use mba_server::audit::Actor;
use mba_server::autosave::{self, registration, Ctx, Outcome};
use rusqlite::types::Value;
use support::{body, App};

/// T-05-15 (on the users resource; `/people` is I2): two fields -> 422, nothing written; no field -> 422.
#[tokio::test]
async fn one_field_per_request() {
    let app = App::new();
    let admin = app.admin().await;
    let id = app.user_id("equintana");
    let before = app.count("SELECT COUNT(*) FROM audit_log WHERE action = 'field.change'");

    let res = app.patch(&format!("/admin/users/{id}"), &admin, "display_name=X+Y&role=admin", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(res.headers()["hx-reswap"], "none");
    let res = app.patch(&format!("/admin/users/{id}"), &admin, "", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    // A field outside the allow list (passwords use their own POST route).
    let res = app.patch(&format!("/admin/users/{id}"), &admin, "password_hash=x", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let (name, role): (String, String) = app
        .conn()
        .query_row("SELECT display_name, role FROM users WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!((name.as_str(), role.as_str()), ("Edwin Quintana Roldán", "staff"));
    assert_eq!(app.count("SELECT COUNT(*) FROM audit_log WHERE action = 'field.change'"), before);
}

/// AP-02: a PATCH without the CSRF header -> 403, nothing written.
#[tokio::test]
async fn csrf_missing_is_refused() {
    let app = App::new();
    let admin = app.admin().await;
    let id = app.user_id("equintana");
    let res = app.patch(&format!("/admin/users/{id}"), &admin, "display_name=Otro+Nombre", false).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let res = app.patch("/admin/settings", &admin, "org.name=Otra", false).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let name: String =
        app.conn().query_row("SELECT display_name FROM users WHERE id = ?1", [id], |r| r.get(0)).unwrap();
    assert_eq!(name, "Edwin Quintana Roldán");
}

/// 200: the field fragment with the saved, normalised value; one `field.change` row with old, new, actor and
/// request id. 422: value as typed, aria-invalid, message, data-unsaved; nothing written.
#[tokio::test]
async fn save_and_refuse_one_field() {
    let app = App::new();
    let admin = app.admin().await;
    let id = app.user_id("equintana");
    let before = app.count("SELECT COUNT(*) FROM audit_log WHERE action = 'field.change'");

    let res = app.patch(&format!("/admin/users/{id}"), &admin, "display_name=++Edwin+Q.+Rold%C3%A1n++", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body(res).await;
    assert!(html.starts_with("<div class=\"field-wrap\""), "{html}");
    assert!(html.contains("value=\"Edwin Q. Roldán\"") && html.contains("data-saved=\"Edwin Q. Roldán\""));
    assert!(!html.contains("data-unsaved"));
    let detail: String = app
        .conn()
        .query_row("SELECT detail FROM audit_log WHERE action = 'field.change' ORDER BY id DESC LIMIT 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    let d: serde_json::Value = serde_json::from_str(&detail).unwrap();
    assert_eq!(d["table"], "users");
    assert_eq!(d["field"], "display_name");
    assert_eq!(d["old"], "Edwin Quintana Roldán");
    assert_eq!(d["new"], "Edwin Q. Roldán");
    assert_eq!(d["actor"], "mvega");
    assert!(d["request_id"].as_str().is_some_and(|r| !r.is_empty()));

    let res = app.patch(&format!("/admin/users/{id}"), &admin, "display_name=+++", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = body(res).await;
    assert!(
        html.contains("data-unsaved") && html.contains("aria-invalid=\"true\"") && html.contains("Escriba un valor.")
    );
    assert!(html.contains("value=\"   \"") && html.contains("data-saved=\"Edwin Q. Roldán\""), "{html}");
    assert_eq!(app.count("SELECT COUNT(*) FROM audit_log WHERE action = 'field.change'"), before + 1);
}

/// BR-SYS-03: known setting keys only; a known key saves and is normalised (dollars -> cents).
#[tokio::test]
async fn settings_known_keys_only() {
    let app = App::new();
    let admin = app.admin().await;
    for bad in ["org.timezone=UTC", "logo_file_uuid=x", "no.such.key=1"] {
        let res = app.patch("/admin/settings", &admin, bad, true).await;
        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
    }
    let res = app.patch("/admin/settings", &admin, "nmls_cost_x_hour_cents=%242.25", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body(res).await.contains("value=\"2.25\""));
    let v: String = app
        .conn()
        .query_row("SELECT value FROM app_settings WHERE key = 'nmls_cost_x_hour_cents'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, "225");
    let res = app.patch("/admin/settings", &admin, "session.idle_minutes=0", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let tz: String =
        app.conn().query_row("SELECT value FROM app_settings WHERE key = 'org.timezone'", [], |r| r.get(0)).unwrap();
    assert_eq!(tz, "America/Puerto_Rico");
}

/// T-05-13 (through the registry I2's item route uses): qty=3 saves and writes one field.change row; the totals
/// helper builds the out-of-band fragment. T-05-14: qty=0 is refused. T-05-10 / D-88: the qty patch sets
/// qty_manual = 1 and the guest recompute skips that row.
#[tokio::test]
async fn item_qty_sets_qty_manual_and_survives_guest_recompute() {
    let app = App::new();
    let mut conn = app.conn();
    let (reg, item, other): (String, String, String) = conn
        .query_row(
            "SELECT a.registration_uuid, a.uuid, b.uuid FROM registration_items a
               JOIN registration_items b ON b.registration_uuid = a.registration_uuid AND b.uuid <> a.uuid
              LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    let actor = Actor::user(1, "mvega");
    let ctx = Ctx { actor: &actor, request_id: Some("t"), record: "Registro" };
    let def = registration::ITEM.get("qty").unwrap();
    let key = Value::Text(item.clone());

    let out = autosave::save(&mut conn, &registration::ITEM, &key, def, "0", &ctx, |_, _, _| Ok(())).unwrap();
    assert!(matches!(out, Outcome::Invalid(_)));
    let out = autosave::save(&mut conn, &registration::ITEM, &key, def, "3", &ctx, |_, _, _| Ok(())).unwrap();
    assert!(matches!(out, Outcome::Saved { .. }));
    let (qty, manual, by): (i64, i64, String) = conn
        .query_row("SELECT ticket_qty, qty_manual, modified_by FROM registration_items WHERE uuid = ?1", [&item], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!((qty, manual, by.as_str()), (3, 1, "mvega"));
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE action = 'field.change' AND entity = 'registration_items' AND entity_uuid = ?1",
            [&item],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
    assert!(autosave::totales("<b>$10.00</b>").contains("hx-swap-oob=\"innerHTML:#totales\""));

    // Two guests: unflagged rows get 3, the hand-set row keeps its qty.
    conn.execute("DELETE FROM registration_guests WHERE registration_uuid = ?1", [&reg]).unwrap();
    for g in ["g-test-1", "g-test-2"] {
        conn.execute(
            "INSERT INTO registration_guests (uuid, registration_uuid, name_badge) VALUES (?1, ?2, 'Inv')",
            [g, &reg],
        )
        .unwrap();
    }
    conn.execute("UPDATE registration_items SET ticket_qty = 9 WHERE uuid = ?1", [&item]).unwrap();
    registration::recompute_guest_qty(&conn, &reg, &actor).unwrap();
    let q = |u: &str| -> i64 {
        conn.query_row("SELECT ticket_qty FROM registration_items WHERE uuid = ?1", [u], |r| r.get(0)).unwrap()
    };
    assert_eq!(q(&item), 9, "qty_manual = 1 row untouched");
    assert_eq!(q(&other), 3, "guests + 1");
}
