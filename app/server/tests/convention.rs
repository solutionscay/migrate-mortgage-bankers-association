//! I2 gate: server-side invariants and negative authorization at the domain level, before any screen is wired.
//! T-07-01..06, T-08-05..10, T-08-13, T-03-04; domain parts of T-05-03, -04, -06, -10, -13, -14, -15; the audited
//! rules BR-REG-01, BR-REG-06 (Cancel changes nothing), BR-EVT-01.

use mba_server::auth::Role;
use mba_server::autosave::{OneField, Outcome};
use mba_server::convention::registries::{patch, Patched, Resource, Target};
use mba_server::convention::{begin, events, people, plans, policy, registrations as regs, sponsors, DomainError, Who};
use mba_server::{db, seed};
use rusqlite::Connection;

fn seeded() -> Connection {
    let mut conn = db::open_in_memory().unwrap();
    seed::run(&mut conn, seed::DEFAULT_SEED).unwrap();
    conn
}

fn admin() -> Who {
    Who::new(1, "mvega", Role::Admin)
}
fn staff() -> Who {
    Who::new(3, "equintana", Role::Staff)
}

fn one<T: rusqlite::types::FromSql>(conn: &Connection, sql: &str, arg: &str) -> T {
    conn.query_row(sql, [arg], |r| r.get(0)).unwrap()
}

/// An active event with a Hotel, a Cena and a Foro item, a plan with Hotel + Cena included, and a person.
struct Fx {
    event: String,
    hotel: String,
    cena: String,
    foro: String,
    plan: String,
    person: String,
}

fn fixture(conn: &mut Connection) -> Fx {
    let w = staff();
    let tx = begin(conn).unwrap();
    let event = events::create_event(
        &tx,
        &w,
        &events::NewEvent { name: "Evento de prueba".into(), date_start: "2027-06-01".into(), ..Default::default() },
    )
    .unwrap();
    let item = |name: &str, kind: &str, price: &str| events::NewEventItem {
        name: name.into(),
        kind: kind.into(),
        date_start: "2027-06-01".into(),
        price: price.into(),
        ..Default::default()
    };
    let hotel = events::add_event_item(&tx, &w, &event, &item("Hotel", "Hotel", "289.00")).unwrap();
    let cena = events::add_event_item(&tx, &w, &event, &item("Cena", "Cena", "110")).unwrap();
    let foro = events::add_event_item(&tx, &w, &event, &item("Foro", "Foro", "0")).unwrap();
    let (plan, added) = plans::create_plan(&tx, &w, &event, "Plan A").unwrap();
    assert_eq!(added, 3);
    tx.execute(
        "UPDATE package_items SET active = 1 WHERE package_uuid = ?1 AND event_item_uuid IN (?2, ?3)",
        [&plan, &hotel, &cena],
    )
    .unwrap();
    let person = people::create_person(
        &tx,
        &w,
        &people::NewPerson { name_first: "Ana".into(), name_last: "Prueba".into(), ..Default::default() },
    )
    .unwrap();
    tx.commit().unwrap();
    Fx { event, hotel, cena, foro, plan, person }
}

/// A registration of the fixture person with the event set and (optionally) the plan picked.
fn registration(conn: &mut Connection, fx: &Fx, plan: bool) -> String {
    let w = staff();
    let tx = begin(conn).unwrap();
    let r = regs::create_registration(&tx, &w, &fx.person).unwrap();
    regs::select_event(&tx, &w, &r, &fx.event).unwrap();
    if plan {
        regs::select_plan(&tx, &w, &r, &fx.plan, false).unwrap();
    }
    tx.commit().unwrap();
    r
}

fn item_uuids(conn: &Connection, reg: &str) -> Vec<String> {
    conn.prepare("SELECT uuid FROM registration_items WHERE registration_uuid = ?1 ORDER BY uuid")
        .unwrap()
        .query_map([reg], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn qtys(conn: &Connection, reg: &str) -> Vec<(String, i64, i64)> {
    conn.prepare("SELECT event_item_uuid, ticket_qty, qty_manual FROM registration_items WHERE registration_uuid = ?1 ORDER BY event_item_uuid")
        .unwrap()
        .query_map([reg], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn patch_item(conn: &mut Connection, reg: &str, item: &str, field: &str, value: &str) -> Outcome {
    match patch(conn, &staff(), Some("t"), Target::child(Resource::Item, item, reg), field, value).unwrap() {
        Patched::Done(o) => o,
        Patched::Unknown => panic!("unknown field {field}"),
    }
}

// --- T-07-01, T-05-03, T-08-13 ------------------------------------------------------------------------------------

#[test]
fn t07_01_plan_change_replaces_only_this_registrations_items() {
    let mut conn = seeded();
    let fx = fixture(&mut conn);
    let a = registration(&mut conn, &fx, true);
    let b = registration(&mut conn, &fx, true);
    assert_eq!(item_uuids(&conn, &a).len(), 2, "inactive plan item (Foro) skipped");
    let b_before = item_uuids(&conn, &b);
    let a_before = item_uuids(&conn, &a);

    // A second plan with only the Foro included.
    let tx = begin(&mut conn).unwrap();
    let (q, _) = plans::create_plan(&tx, &staff(), &fx.event, "Plan B").unwrap();
    tx.execute("UPDATE package_items SET active = 1 WHERE package_uuid = ?1 AND event_item_uuid = ?2", [&q, &fx.foro])
        .unwrap();
    tx.commit().unwrap();

    // T-05-03: a plan is set, no confirm -> ConfirmRequired, nothing written ("cancel keeps old items").
    let tx = begin(&mut conn).unwrap();
    assert_eq!(regs::select_plan(&tx, &staff(), &a, &q, false), Err(DomainError::ConfirmRequired));
    drop(tx);
    assert_eq!(item_uuids(&conn, &a), a_before);

    let tx = begin(&mut conn).unwrap();
    assert_eq!(regs::select_plan(&tx, &staff(), &a, &q, true).unwrap(), 1);
    tx.commit().unwrap();
    let a_lines = qtys(&conn, &a);
    assert_eq!(a_lines.len(), 1);
    assert_eq!(a_lines[0].0, fx.foro);
    assert_eq!(item_uuids(&conn, &b), b_before, "T-08-13: the other registration is untouched");
    assert_eq!(one::<String>(&conn, "SELECT package_uuid FROM registration WHERE uuid = ?1", &a), q);
    assert_eq!(
        one::<i64>(
            &conn,
            "SELECT COUNT(*) FROM audit_log WHERE action = 'registration.plan_changed' AND entity_uuid = ?1",
            &a
        ),
        2
    );
}

/// T-08-13 (crafted scope): a plan or an event item of another event is refused; scope comes from the server.
#[test]
fn t08_13_scope_is_fixed_by_the_server() {
    let mut conn = seeded();
    let fx = fixture(&mut conn);
    let r = registration(&mut conn, &fx, false);
    let other_plan: String = one(&conn, "SELECT uuid FROM packages WHERE event_uuid <> ?1 LIMIT 1", &fx.event);
    let other_item: String = one(&conn, "SELECT uuid FROM event_items WHERE event_uuid <> ?1 LIMIT 1", &fx.event);
    let tx = begin(&mut conn).unwrap();
    assert_eq!(regs::select_plan(&tx, &staff(), &r, &other_plan, true), Err(DomainError::NotOfEvent));
    assert_eq!(regs::add_item(&tx, &staff(), &r, &other_item), Err(DomainError::NotOfEvent));
    let fresh = regs::create_registration(&tx, &staff(), &fx.person).unwrap();
    assert_eq!(regs::select_plan(&tx, &staff(), &fresh, &fx.plan, true), Err(DomainError::NoEvent));
}

// --- T-07-02, T-05-10 -----------------------------------------------------------------------------------------------

#[test]
fn t07_02_guest_qty_and_hand_set_qty_d88() {
    let mut conn = seeded();
    let fx = fixture(&mut conn);
    let r = registration(&mut conn, &fx, true);
    assert!(qtys(&conn, &r).iter().all(|(_, q, m)| *q == 1 && *m == 0), "zero guests -> 1");

    let tx = begin(&mut conn).unwrap();
    let g1 = regs::add_guest(&tx, &staff(), &r, &regs::NewGuest { name_first: "Luis".into(), ..Default::default() })
        .unwrap();
    tx.commit().unwrap();
    assert!(qtys(&conn, &r).iter().all(|(_, q, _)| *q == 2));

    // T-05-10: a qty PATCH sets qty_manual = 1; the next guest change skips that row.
    let cena_line: String = one(&conn, "SELECT uuid FROM registration_items WHERE event_item_uuid = (SELECT uuid FROM event_items WHERE name = 'Cena' AND event_uuid = (SELECT event_uuid FROM registration WHERE uuid = ?1))", &r);
    assert!(matches!(patch_item(&mut conn, &r, &cena_line, "qty", "5"), Outcome::Saved { changed: true, .. }));
    let tx = begin(&mut conn).unwrap();
    let g2 = regs::add_guest(&tx, &staff(), &r, &regs::NewGuest { name_first: "Rosa".into(), ..Default::default() })
        .unwrap();
    tx.commit().unwrap();
    for (ei, q, m) in qtys(&conn, &r) {
        if ei == fx.cena {
            assert_eq!((q, m), (5, 1), "hand-set qty kept");
        } else {
            assert_eq!((q, m), (3, 0));
        }
    }
    // Edit a guest: recompute runs, nothing drifts.
    assert!(matches!(
        patch(
            &mut conn,
            &staff(),
            None,
            Target { resource: Resource::Guest, key: &g1, parent: Some(&r) },
            "name_badge",
            "Luis P."
        )
        .unwrap(),
        Patched::Done(Outcome::Saved { changed: true, .. })
    ));
    let tx = begin(&mut conn).unwrap();
    regs::remove_guest(&tx, &staff(), &r, &g1).unwrap();
    regs::remove_guest(&tx, &staff(), &r, &g2).unwrap();
    tx.commit().unwrap();
    for (ei, q, _) in qtys(&conn, &r) {
        assert_eq!(q, if ei == fx.cena { 5 } else { 1 });
    }
    // A plan change rebuilds the items with qty_manual = 0 and guests + 1.
    let tx = begin(&mut conn).unwrap();
    regs::select_plan(&tx, &staff(), &r, &fx.plan, true).unwrap();
    tx.commit().unwrap();
    assert!(qtys(&conn, &r).iter().all(|(_, q, m)| *q == 1 && *m == 0));
}

// --- T-07-03, T-05-13, T-05-14 --------------------------------------------------------------------------------------

#[test]
fn t07_03_registration_totals_from_the_views() {
    let mut conn = seeded();
    let fx = fixture(&mut conn);
    // No items: total 0.
    let empty = registration(&mut conn, &fx, false);
    assert_eq!(regs::totals(&conn, &empty).unwrap().total, 0);

    // Without plan: sum of qty x price; a no_charge line counts 0.
    let r = registration(&mut conn, &fx, false);
    let tx = begin(&mut conn).unwrap();
    let hotel = regs::add_item(&tx, &staff(), &r, &fx.hotel).unwrap();
    let cena = regs::add_item(&tx, &staff(), &r, &fx.cena).unwrap();
    tx.commit().unwrap();
    let t = regs::totals(&conn, &r).unwrap();
    assert_eq!(t.total, 28_900 + 11_000);
    assert_eq!(t.registration_type, "Convencionista", "BR-REG-07: a Hotel item");

    // T-05-13: qty=3 saves, one field.change row, the total follows.
    let audit_sql = "SELECT COUNT(*) FROM audit_log WHERE action = 'field.change' AND entity_uuid = ?1";
    let before: i64 = one(&conn, audit_sql, &cena);
    assert!(matches!(patch_item(&mut conn, &r, &cena, "qty", "3"), Outcome::Saved { changed: true, .. }));
    assert_eq!(one::<i64>(&conn, audit_sql, &cena), before + 1);
    assert_eq!(regs::totals(&conn, &r).unwrap().total, 28_900 + 3 * 11_000);
    // T-05-14: qty=0 refused; nothing written; totals unchanged.
    assert!(matches!(patch_item(&mut conn, &r, &cena, "qty", "0"), Outcome::Invalid(_)));
    assert_eq!(one::<i64>(&conn, "SELECT ticket_qty FROM registration_items WHERE uuid = ?1", &cena), 3);
    assert_eq!(regs::totals(&conn, &r).unwrap().total, 28_900 + 3 * 11_000);
    assert!(matches!(patch_item(&mut conn, &r, &hotel, "no_charge", "1"), Outcome::Saved { .. }));
    assert_eq!(regs::totals(&conn, &r).unwrap().total, 3 * 11_000);

    // With plan: plan price + lines not from the plan. Plan price = Hotel + Cena (active), x qty 1.
    let p = registration(&mut conn, &fx, true);
    let tx = begin(&mut conn).unwrap();
    regs::add_item(&tx, &staff(), &p, &fx.foro).unwrap();
    let extra = regs::add_item(&tx, &staff(), &p, &fx.cena).unwrap();
    tx.commit().unwrap();
    assert_eq!(plans::plan_price(&conn, &fx.plan).unwrap(), 39_900);
    assert_eq!(regs::totals(&conn, &p).unwrap().total, 39_900 + 11_000);

    // Payments; a voided payment does not count (BR-REG-08, -09).
    let tx = begin(&mut conn).unwrap();
    regs::add_payment(&tx, &staff(), &p, "100.00", "Cheque", "1001").unwrap();
    let pay2 = regs::add_payment(&tx, &staff(), &p, "50", "VISA", "").unwrap();
    regs::void_payment(&tx, &admin(), &pay2, "Error de cobro").unwrap();
    tx.commit().unwrap();
    let t = regs::totals(&conn, &p).unwrap();
    assert_eq!((t.payment_total, t.balance), (10_000, 39_900 + 11_000 - 10_000));
    let _ = extra;
}

// --- T-07-04, T-05-06, T-08-08 --------------------------------------------------------------------------------------

#[test]
fn t07_04_void_payment_admin_with_reason() {
    let mut conn = seeded();
    let fx = fixture(&mut conn);
    let r = registration(&mut conn, &fx, true);
    let tx = begin(&mut conn).unwrap();
    let pay = regs::add_payment(&tx, &staff(), &r, "200", "Efectivo", "").unwrap();
    assert_eq!(
        regs::add_payment(&tx, &staff(), &r, "0", "Efectivo", "").unwrap_err().status().as_u16(),
        422,
        "amount 0"
    );
    let e = regs::void_payment(&tx, &staff(), &pay, "x").unwrap_err();
    assert_eq!((e.status().as_u16(), e), (403, DomainError::Forbidden), "T-05-06 staff");
    let e = regs::void_payment(&tx, &admin(), &pay, "  ").unwrap_err();
    assert_eq!(e.status().as_u16(), 422, "T-05-06 admin without reason");
    tx.commit().unwrap();
    let balance_before = regs::totals(&conn, &r).unwrap().balance;
    let tx = begin(&mut conn).unwrap();
    regs::void_payment(&tx, &admin(), &pay, "Cheque devuelto").unwrap();
    assert_eq!(regs::void_payment(&tx, &admin(), &pay, "otra vez"), Err(DomainError::AlreadyVoided));
    tx.commit().unwrap();
    assert_eq!(regs::totals(&conn, &r).unwrap().balance, balance_before + 20_000);
    assert_eq!(one::<i64>(&conn, "SELECT COUNT(*) FROM payments WHERE uuid = ?1", &pay), 1, "never deleted");
    assert_eq!(
        one::<i64>(&conn, "SELECT COUNT(*) FROM audit_log WHERE action = 'payment.void' AND entity_uuid = ?1", &pay),
        1
    );
}

// --- T-07-05 --------------------------------------------------------------------------------------------------------

#[test]
fn t07_05_build_plan_items() {
    let mut conn = seeded();
    let w = staff();
    let tx = begin(&mut conn).unwrap();
    let ev = events::create_event(&tx, &w, &events::NewEvent { name: "Sin actividades".into(), ..Default::default() })
        .unwrap();
    let (plan, added) = plans::create_plan(&tx, &w, &ev, "Vacío").unwrap();
    assert_eq!(added, 0);
    assert_eq!(
        plans::build_plan_items(&tx, &w, &plan),
        Err(DomainError::NoEventItems),
        "event without items -> message"
    );
    for n in ["Uno", "Dos"] {
        events::add_event_item(
            &tx,
            &w,
            &ev,
            &events::NewEventItem { name: n.into(), kind: "Foro".into(), ..Default::default() },
        )
        .unwrap();
    }
    assert_eq!(plans::build_plan_items(&tx, &w, &plan).unwrap(), 2);
    assert_eq!(plans::build_plan_items(&tx, &w, &plan).unwrap(), 0, "second run adds nothing");
    events::add_event_item(
        &tx,
        &w,
        &ev,
        &events::NewEventItem { name: "Tres".into(), kind: "Cena".into(), price: "10".into(), ..Default::default() },
    )
    .unwrap();
    assert_eq!(plans::build_plan_items(&tx, &w, &plan).unwrap(), 1, "only the missing one");
    let (price, qty): (i64, i64) = tx
        .query_row("SELECT pi.price, pi.ticket_qty FROM package_items pi JOIN event_items ei ON ei.uuid = pi.event_item_uuid WHERE ei.name = 'Tres' AND pi.package_uuid = ?1", [&plan], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!((price, qty), (1_000, 1), "BR-EVT-07 copies price and qty");
    // An inactive event cannot take a plan (BR-EVT-02).
    tx.execute("UPDATE events SET status = 'Inactivo' WHERE uuid = ?1", [&ev]).unwrap();
    assert!(matches!(plans::create_plan(&tx, &w, &ev, "X"), Err(DomainError::Validation(..))));
}

// --- T-07-06 --------------------------------------------------------------------------------------------------------

#[test]
fn t07_06_event_totals_and_plan_price_override() {
    let mut conn = seeded();
    let fx = fixture(&mut conn);
    let zero = events::event_totals(&conn, &fx.event).unwrap();
    assert_eq!((zero.count_registrations, zero.total, zero.balance), (0, 0, 0));
    assert_eq!(zero.price, 28_900 + 11_000);

    let r1 = registration(&mut conn, &fx, true);
    let one_reg = events::event_totals(&conn, &fx.event).unwrap();
    assert_eq!((one_reg.count_registrations, one_reg.total), (1, 39_900));
    let r2 = registration(&mut conn, &fx, true);
    let tx = begin(&mut conn).unwrap();
    regs::add_guest(&tx, &staff(), &r2, &regs::NewGuest { name_first: "Eva".into(), ..Default::default() }).unwrap();
    regs::add_payment(&tx, &staff(), &r1, "399", "Cheque", "").unwrap();
    let sp = sponsors::create_sponsor(&tx, &staff(), &fx.event, "Banco Ejemplo", "Oro").unwrap();
    tx.commit().unwrap();
    for (f, v) in [("promised", "1000"), ("amount_received", "250")] {
        patch(&mut conn, &staff(), None, Target { resource: Resource::Sponsor, key: &sp, parent: None }, f, v).unwrap();
    }
    let n = events::event_totals(&conn, &fx.event).unwrap();
    assert_eq!((n.count_registrations, n.count_guests, n.count_total), (2, 1, 3));
    assert_eq!((n.total, n.payments, n.balance), (79_800, 39_900, 39_900));
    assert_eq!((n.sponsor_promised, n.sponsor_received, n.sponsor_total), (100_000, 25_000, 125_000));

    // Override: 0 means free, empty means computed.
    patch(
        &mut conn,
        &staff(),
        None,
        Target { resource: Resource::Plan, key: &fx.plan, parent: None },
        "price_override",
        "0",
    )
    .unwrap();
    assert_eq!(plans::plan_price(&conn, &fx.plan).unwrap(), 0);
    patch(
        &mut conn,
        &staff(),
        None,
        Target { resource: Resource::Plan, key: &fx.plan, parent: None },
        "price_override",
        "",
    )
    .unwrap();
    assert_eq!(plans::plan_price(&conn, &fx.plan).unwrap(), 39_900);
}

// --- T-08-05 .. T-08-10 ---------------------------------------------------------------------------------------------

#[test]
fn t08_05_staff_creates_and_edits() {
    let mut conn = seeded();
    let fx = fixture(&mut conn);
    let r = registration(&mut conn, &fx, true);
    assert!(matches!(
        patch(
            &mut conn,
            &staff(),
            None,
            Target { resource: Resource::Registration, key: &r, parent: None },
            "status",
            "Registrado"
        )
        .unwrap(),
        Patched::Done(Outcome::Saved { changed: true, .. })
    ));
    let tx = begin(&mut conn).unwrap();
    regs::add_payment(&tx, &staff(), &r, "10", "Efectivo", "").unwrap();
    tx.commit().unwrap();
}

#[test]
fn t08_06_staff_cannot_delete_parents() {
    let mut conn = seeded();
    let fx = fixture(&mut conn);
    let tx = begin(&mut conn).unwrap();
    assert_eq!(people::delete_person(&tx, &staff(), &fx.person), Err(DomainError::Forbidden));
    assert_eq!(events::delete_event(&tx, &staff(), &fx.event), Err(DomainError::Forbidden));
    assert_eq!(plans::delete_plan(&tx, &staff(), &fx.plan), Err(DomainError::Forbidden));
    // Admin may delete a parent without dependents.
    people::delete_person(&tx, &admin(), &fx.person).unwrap();
}

#[test]
fn t08_07_admin_delete_with_dependents_is_409() {
    let mut conn = seeded();
    let fx = fixture(&mut conn);
    registration(&mut conn, &fx, true);
    let tx = begin(&mut conn).unwrap();
    let e = events::delete_event(&tx, &admin(), &fx.event).unwrap_err();
    assert_eq!((e.status().as_u16(), &e), (409, &DomainError::HasRegistrations(1)));
    assert_eq!(people::delete_person(&tx, &admin(), &fx.person), Err(DomainError::HasRegistrations(1)));
    assert_eq!(plans::delete_plan(&tx, &admin(), &fx.plan), Err(DomainError::HasRegistrations(1)));
    assert_eq!(one::<i64>(&tx, "SELECT COUNT(*) FROM events WHERE uuid = ?1", &fx.event), 1, "no change");
    // OP-EVT-03: an event item used by a registration item is refused (AP-06 + FK).
    assert_eq!(events::delete_event_item(&tx, &staff(), &fx.hotel), Err(DomainError::InUse(1)));
    events::delete_event_item(&tx, &staff(), &fx.foro).unwrap();
}

#[test]
fn t08_08_void_rules() {
    let mut conn = seeded();
    let pay: String = one(&conn, "SELECT p.uuid FROM payments p JOIN registration r ON r.uuid = p.registration_uuid WHERE r.registration_number = ?1 AND p.voided_at IS NULL", "1283");
    let tx = begin(&mut conn).unwrap();
    assert_eq!(regs::void_payment(&tx, &staff(), &pay, "x"), Err(DomainError::Forbidden));
    assert!(matches!(regs::void_payment(&tx, &admin(), &pay, ""), Err(DomainError::Validation("void_reason", _))));
    // DELETE of a payment: there is no such command for any role (policy has no delete; routes come in phase 2).
    assert_eq!(policy::ap05_payment_void(&staff(), "x"), Err(DomainError::Forbidden));
}

#[test]
fn t08_09_registration_with_payment_is_cancelled_not_deleted() {
    let mut conn = seeded();
    let fx = fixture(&mut conn);
    let r = registration(&mut conn, &fx, true);
    let tx = begin(&mut conn).unwrap();
    regs::add_payment(&tx, &staff(), &r, "25", "Efectivo", "").unwrap();
    // T-05-04: 409 with the cancel offer, for staff and admin alike.
    for who in [staff(), admin()] {
        let e = regs::delete_registration(&tx, &who, &r).unwrap_err();
        assert_eq!((e.status().as_u16(), &e), (409, &DomainError::HasPayments(1)));
    }
    regs::cancel_registration(&tx, &staff(), &r).unwrap();
    assert_eq!(one::<String>(&tx, "SELECT status FROM registration WHERE uuid = ?1", &r), "Canceló");
    tx.commit().unwrap();
    // Without payments: staff 403 (money row), admin deletes; items and guests cascade.
    let r2 = registration(&mut conn, &fx, true);
    let tx = begin(&mut conn).unwrap();
    regs::add_guest(&tx, &staff(), &r2, &regs::NewGuest { name_first: "X".into(), ..Default::default() }).unwrap();
    assert_eq!(regs::delete_registration(&tx, &staff(), &r2), Err(DomainError::Forbidden));
    regs::delete_registration(&tx, &admin(), &r2).unwrap();
    assert_eq!(one::<i64>(&tx, "SELECT COUNT(*) FROM registration_items WHERE registration_uuid = ?1", &r2), 0);
    assert_eq!(one::<i64>(&tx, "SELECT COUNT(*) FROM registration_guests WHERE registration_uuid = ?1", &r2), 0);
}

#[test]
fn t08_10_sent_batch_is_not_editable() {
    let conn = seeded();
    let sent: String = one(&conn, "SELECT uuid FROM mail_batches WHERE serial = ?1", "47");
    let draft: String = one(&conn, "SELECT uuid FROM mail_batches WHERE serial = ?1", "44");
    assert!(matches!(policy::ap06_batch_editable(&conn, &sent), Err(DomainError::InUse(_))));
    assert_eq!(policy::ap06_batch_editable(&conn, &draft), Ok(()));
}

// --- T-03-04 --------------------------------------------------------------------------------------------------------

/// Hand sum of a registration from the base tables (BR-REG-04, -05, -08), independent of the views.
fn hand_sum(conn: &Connection, number: i64) -> (i64, i64) {
    let (reg, plan): (String, Option<String>) = conn
        .query_row("SELECT uuid, package_uuid FROM registration WHERE registration_number = ?1", [number], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    let lines: Vec<(Option<String>, i64, i64, i64)> = conn
        .prepare("SELECT package_item_uuid, price, ticket_qty, no_charge FROM registration_items WHERE registration_uuid = ?1")
        .unwrap()
        .query_map([&reg], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let charged: i64 = lines.iter().filter(|l| l.0.is_none() && l.3 == 0).map(|l| l.1 * l.2).sum();
    let plan_price = match plan {
        Some(p) => {
            let over: Option<i64> = one(conn, "SELECT price_override FROM packages WHERE uuid = ?1", &p);
            over.unwrap_or_else(|| one(conn, "SELECT COALESCE(SUM(price * ticket_qty), 0) FROM package_items WHERE package_uuid = ?1 AND active = 1", &p))
        }
        None => 0,
    };
    let paid: i64 = one(
        conn,
        "SELECT COALESCE(SUM(payment), 0) FROM payments WHERE registration_uuid = ?1 AND voided_at IS NULL",
        &reg,
    );
    (plan_price + charged, paid)
}

#[test]
fn t03_04_seeded_totals_match_a_hand_sum() {
    let conn = seeded();
    for number in [1283, 1284, 1288, 1286] {
        let reg: String =
            one(&conn, "SELECT uuid FROM registration WHERE registration_number = ?1", &number.to_string());
        let t = regs::totals(&conn, &reg).unwrap();
        let (total, paid) = hand_sum(&conn, number);
        assert_eq!((t.total, t.payment_total, t.balance), (total, paid, total - paid), "#{number}");
    }
    // The design's registration #1283: P2(B) · 15 marzo, $1,218.00, paid $500.00 (one payment voided), balance $718.00.
    let reg: String = one(&conn, "SELECT uuid FROM registration WHERE registration_number = ?1", "1283");
    let t = regs::totals(&conn, &reg).unwrap();
    assert_eq!((t.total, t.payment_total, t.balance, t.guests_count), (121_800, 50_000, 71_800, 1));
    // #1284: Cena de Gala x2 set by hand (D-88): $220.00.
    let reg: String = one(&conn, "SELECT uuid FROM registration WHERE registration_number = ?1", "1284");
    assert_eq!(regs::totals(&conn, &reg).unwrap().total, 22_000);
}

// --- Audited rules ----------------------------------------------------------------------------------------------

/// BR-REG-01: the picker opens only while the registration has no event; only active events; once.
#[test]
fn br_reg_01_event_picker() {
    let mut conn = seeded();
    let fx = fixture(&mut conn);
    let tx = begin(&mut conn).unwrap();
    let r = regs::create_registration(&tx, &staff(), &fx.person).unwrap();
    let list = regs::event_picker(&tx, &r).unwrap();
    assert!(list.iter().any(|(u, _, _)| *u == fx.event));
    let inactive: String = one(&tx, "SELECT uuid FROM events WHERE status = ?1 LIMIT 1", "Inactivo");
    assert!(!list.iter().any(|(u, _, _)| *u == inactive));
    assert!(matches!(regs::select_event(&tx, &staff(), &r, &inactive), Err(DomainError::Validation(..))));
    regs::select_event(&tx, &staff(), &r, &fx.event).unwrap();
    assert_eq!(regs::event_picker(&tx, &r), Err(DomainError::EventAlreadySet));
    assert_eq!(regs::select_event(&tx, &staff(), &r, &fx.event), Err(DomainError::EventAlreadySet));
    let r2 = regs::create_registration(&tx, &staff(), &fx.person).unwrap();
    tx.execute("UPDATE events SET status = 'Inactivo'", []).unwrap();
    let e = regs::event_picker(&tx, &r2).unwrap_err();
    assert_eq!(e, DomainError::NoActiveEvent);
    assert_eq!(e.message(), "Al momento no hay eventos activos.");
}

/// BR-REG-06: opening the attributes dialog (and Cancel) writes nothing; attributes follow the item type.
#[test]
fn br_reg_06_attributes() {
    let mut conn = seeded();
    let fx = fixture(&mut conn);
    let r = registration(&mut conn, &fx, true);
    let hotel: String = one(&conn, "SELECT ri.uuid FROM registration_items ri JOIN event_items ei ON ei.uuid = ri.event_item_uuid WHERE ei.type = 'Hotel' AND ri.registration_uuid = ?1", &r);
    let cena: String = one(&conn, "SELECT ri.uuid FROM registration_items ri JOIN event_items ei ON ei.uuid = ri.event_item_uuid WHERE ei.type = 'Cena' AND ri.registration_uuid = ?1", &r);
    let snapshot = |c: &Connection| -> (String, i64) {
        (
            one(
                c,
                "SELECT modified_at || COALESCE(hotel_reservation_num, '') FROM registration_items WHERE uuid = ?1",
                &hotel,
            ),
            one(c, "SELECT COUNT(*) FROM audit_log WHERE ?1 <> ''", "x"),
        )
    };
    let before = snapshot(&conn);
    let a = regs::item_attributes(&conn, &r, &hotel).unwrap();
    assert_eq!(a.kind, regs::AttrKind::Hotel);
    assert_eq!(regs::item_attributes(&conn, &r, &cena).unwrap().kind, regs::AttrKind::Seating);
    assert_eq!(snapshot(&conn), before, "open + Cancel changes nothing");
    assert!(matches!(patch_item(&mut conn, &r, &hotel, "mesa", "4"), Outcome::Invalid(_)), "no table on a Hotel item");
    assert!(matches!(patch_item(&mut conn, &r, &cena, "hotel_reservation_num", "X"), Outcome::Invalid(_)));
    assert!(matches!(
        patch_item(&mut conn, &r, &hotel, "hotel_reservation_num", "DEMO-01"),
        Outcome::Saved { changed: true, .. }
    ));
    assert!(matches!(patch_item(&mut conn, &r, &cena, "mesa", "12"), Outcome::Saved { changed: true, .. }));
}

/// BR-EVT-01: event end date defaults to the start; an item's end date always equals its start.
#[test]
fn br_evt_01_dates() {
    let mut conn = seeded();
    let fx = fixture(&mut conn);
    assert_eq!(one::<String>(&conn, "SELECT date_end FROM events WHERE uuid = ?1", &fx.event), "2027-06-01");
    assert_eq!(one::<String>(&conn, "SELECT date_end FROM event_items WHERE uuid = ?1", &fx.hotel), "2027-06-01");
    patch(
        &mut conn,
        &staff(),
        None,
        Target { resource: Resource::EventItem, key: &fx.hotel, parent: None },
        "date_start",
        "6/3/2027",
    )
    .unwrap();
    assert_eq!(
        one::<String>(&conn, "SELECT date_start || ' ' || date_end FROM event_items WHERE uuid = ?1", &fx.hotel),
        "2027-06-03 2027-06-03"
    );
    assert!(
        matches!(
            patch(
                &mut conn,
                &staff(),
                None,
                Target { resource: Resource::EventItem, key: &fx.hotel, parent: None },
                "date_end",
                "2027-06-09"
            )
            .unwrap(),
            Patched::Unknown
        ),
        "not editable"
    );
    patch(
        &mut conn,
        &staff(),
        None,
        Target { resource: Resource::Event, key: &fx.event, parent: None },
        "date_end",
        "2027-06-05",
    )
    .unwrap();
    patch(
        &mut conn,
        &staff(),
        None,
        Target { resource: Resource::Event, key: &fx.event, parent: None },
        "date_end",
        "",
    )
    .unwrap();
    assert_eq!(one::<String>(&conn, "SELECT date_end FROM events WHERE uuid = ?1", &fx.event), "2027-06-01");
}

// --- T-05-15 and the registries --------------------------------------------------------------------------------

/// T-05-15 (domain part): two fields in one PATCH body are refused before any write; unknown fields too.
#[test]
fn t05_15_one_field_and_allow_lists() {
    assert!(OneField::parse(b"name_first=Ana&name_last=Prueba").is_err());
    let mut conn = seeded();
    let fx = fixture(&mut conn);
    assert!(matches!(
        patch(
            &mut conn,
            &staff(),
            None,
            Target { resource: Resource::Person, key: &fx.person, parent: None },
            "uuid",
            "x"
        )
        .unwrap(),
        Patched::Unknown
    ));
    assert!(matches!(
        patch(
            &mut conn,
            &staff(),
            None,
            Target { resource: Resource::Person, key: &fx.person, parent: None },
            "email",
            "ana@ejemplo"
        )
        .unwrap(),
        Patched::Done(Outcome::Invalid(_))
    ));
    assert!(matches!(
        patch(
            &mut conn,
            &staff(),
            None,
            Target { resource: Resource::Registration, key: "nope", parent: None },
            "status",
            "Nuevo"
        ),
        Err(mba_server::error::AppError::NotFound)
    ));
    // An item PATCH through the wrong registration is not found.
    let r = registration(&mut conn, &fx, true);
    let other = registration(&mut conn, &fx, true);
    let item = item_uuids(&conn, &r).remove(0);
    assert!(patch(
        &mut conn,
        &staff(),
        None,
        Target { resource: Resource::Item, key: &item, parent: Some(&other) },
        "qty",
        "2"
    )
    .is_err());
    // Every registry field is a real column (a typo would only fail at the first PATCH).
    for res in [
        Resource::Person,
        Resource::Registration,
        Resource::Item,
        Resource::Guest,
        Resource::Event,
        Resource::EventItem,
        Resource::Sponsor,
        Resource::Plan,
        Resource::PlanItem,
    ] {
        let reg = res.registry();
        for f in reg.fields {
            conn.prepare(&format!("SELECT {} FROM {} LIMIT 0", f.column, reg.table))
                .unwrap_or_else(|e| panic!("{}.{}: {e}", reg.table, f.column));
            for m in f.mirror {
                conn.prepare(&format!("SELECT {m} FROM {} LIMIT 0", reg.table)).unwrap();
            }
        }
    }
}

/// Plan event change (PATCH /plans/{id} event_uuid): active events only, then the build runs (OP-EVT-04).
#[test]
fn plan_event_change_builds_items() {
    let mut conn = seeded();
    let fx = fixture(&mut conn);
    let other: String = one(&conn, "SELECT uuid FROM events WHERE name = ?1", "Foro de Otoño Demo 2027");
    let inactive: String = one(&conn, "SELECT uuid FROM events WHERE name = ?1", "Foro Demo 2026");
    assert!(matches!(
        patch(
            &mut conn,
            &staff(),
            None,
            Target { resource: Resource::Plan, key: &fx.plan, parent: None },
            "event_uuid",
            &inactive
        )
        .unwrap(),
        Patched::Done(Outcome::Invalid(_))
    ));
    let before: i64 = one(&conn, "SELECT COUNT(*) FROM package_items WHERE package_uuid = ?1", &fx.plan);
    patch(
        &mut conn,
        &staff(),
        None,
        Target { resource: Resource::Plan, key: &fx.plan, parent: None },
        "event_uuid",
        &other,
    )
    .unwrap();
    let after: i64 = one(&conn, "SELECT COUNT(*) FROM package_items WHERE package_uuid = ?1", &fx.plan);
    assert_eq!(after, before + 3, "the Foro's 3 items added");
}
