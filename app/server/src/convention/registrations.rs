//! REG: registrations, their items, guests and payments (OP-REG-01..10; BR-REG-01..11). Totals are
//! `v_registration_totals` and `v_registration_item_lines` (BR-REG-04, -05, -07, -08), read with [`totals`] and
//! [`lines`].

use super::{count, events, new_uuid, next_serial, optional, policy, DResult, DomainError, Who};
use crate::audit;
use crate::autosave::{self, registration::recompute_guest_qty};
use crate::clock;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::json;

/// `registration.status` values (BR-REG-10; 04 CHECK). New registrations start Nuevo.
pub const STATUSES: &[&str] = &["Nuevo", "Registrado", "Asistió", "Canceló", "No Canceló"];
/// Door check-in buttons (BR-REG-10, value list `registro_puerta_status`).
pub const DOOR_STATUSES: &[&str] = &["Asistió", "No Asistió", "Canceló"];
/// `payments.payment_type` values (value list `tipo_de_pago`, D-28).
pub const PAYMENT_TYPES: &[&str] = &["VISA", "MC", "AMEX", "Efectivo", "Cheque"];
/// First registration number when the table is empty (the seed continues the design's numbers).
const FIRST_NUMBER: i64 = 1000;
const FIRST_INVOICE: i64 = 30000;

/// The registration row as commands need it.
#[derive(Debug, Clone)]
pub struct Reg {
    pub uuid: String,
    pub person_uuid: String,
    pub event_uuid: Option<String>,
    pub package_uuid: Option<String>,
    pub number: Option<i64>,
    pub status: String,
}

pub fn get(conn: &Connection, uuid: &str) -> DResult<Reg> {
    Ok(conn.query_row(
        "SELECT uuid, person_uuid, event_uuid, package_uuid, registration_number, status FROM registration WHERE uuid = ?1",
        [uuid],
        |r| {
            Ok(Reg {
                uuid: r.get(0)?,
                person_uuid: r.get(1)?,
                event_uuid: r.get(2)?,
                package_uuid: r.get(3)?,
                number: r.get(4)?,
                status: r.get(5)?,
            })
        },
    )?)
}

/// OP-REG-01: a new registration of a person: next `registration_number` in the same transaction, status Nuevo
/// (BR-REG-10), no event yet. Returns its uuid.
pub fn create_registration(tx: &Transaction, who: &Who, person_uuid: &str) -> DResult<String> {
    policy::ap03_domain(who)?;
    tx.query_row("SELECT 1 FROM people WHERE uuid = ?1", [person_uuid], |_| Ok(()))
        .optional()?
        .ok_or(DomainError::NotFound)?;
    let number = next_serial(tx, "registration", "registration_number", FIRST_NUMBER)?;
    let uuid = new_uuid();
    let now = clock::now_text();
    tx.execute(
        "INSERT INTO registration (uuid, person_uuid, registration_number, status, created_at, created_by, modified_at, modified_by)
         VALUES (?1, ?2, ?3, 'Nuevo', ?4, ?5, ?4, ?5)",
        params![uuid, person_uuid, number, now, who.actor.username],
    )?;
    Ok(uuid)
}

/// BR-REG-01, the "Escoger evento" button: the active events, or why the picker does not open. Read only.
pub fn event_picker(conn: &Connection, registration_uuid: &str) -> DResult<Vec<(String, String, Option<String>)>> {
    let reg = get(conn, registration_uuid)?;
    if reg.event_uuid.is_some() {
        return Err(DomainError::EventAlreadySet);
    }
    let list = events::active_events(conn)?;
    if list.is_empty() {
        return Err(DomainError::NoActiveEvent);
    }
    Ok(list)
}

/// OP-REG-02 / BR-REG-01: set the event once, from the active events. Writes only this registration (AMB-07-01).
pub fn select_event(tx: &Transaction, who: &Who, registration_uuid: &str, event_uuid: &str) -> DResult<()> {
    policy::ap03_domain(who)?;
    let reg = get(tx, registration_uuid)?;
    if reg.event_uuid.is_some() {
        return Err(DomainError::EventAlreadySet);
    }
    let status: Option<String> =
        tx.query_row("SELECT status FROM events WHERE uuid = ?1", [event_uuid], |r| r.get(0)).optional()?;
    match status.as_deref() {
        Some("Activo") => {}
        Some(_) => return Err(DomainError::Validation("event_uuid", "Escoja un evento activo.".into())),
        None => return Err(DomainError::NotFound),
    }
    tx.execute("UPDATE registration SET event_uuid = ?1 WHERE uuid = ?2", params![event_uuid, registration_uuid])?;
    audit::stamp_modified(tx, "registration", "uuid", &registration_uuid, &who.actor)?;
    Ok(())
}

/// OP-REG-03 / BR-REG-02: pick a plan. With a plan already set and no `confirm`: `ConfirmRequired`, nothing written
/// (T-05-03). Else, in this transaction: delete this registration's items only, insert one item per active plan
/// item (price from the plan item, BR-REG-05; qty = guests + 1, `qty_manual` = 0, BR-REG-03 / D-88), set the plan.
/// Returns the number of items created. AP-09: the scope is this registration, fixed by the server (T-08-13).
pub fn select_plan(
    tx: &Transaction,
    who: &Who,
    registration_uuid: &str,
    plan_uuid: &str,
    confirm: bool,
) -> DResult<usize> {
    policy::ap09_bulk(who)?;
    let reg = get(tx, registration_uuid)?;
    let event = reg.event_uuid.clone().ok_or(DomainError::NoEvent)?;
    let plan_event: String = tx
        .query_row("SELECT event_uuid FROM packages WHERE uuid = ?1", [plan_uuid], |r| r.get(0))
        .optional()?
        .ok_or(DomainError::NotFound)?;
    if plan_event != event {
        return Err(DomainError::NotOfEvent);
    }
    if reg.package_uuid.is_some() && !confirm {
        return Err(DomainError::ConfirmRequired);
    }
    let removed = tx.execute("DELETE FROM registration_items WHERE registration_uuid = ?1", [registration_uuid])?;
    let qty = guest_count(tx, registration_uuid)? + 1;
    let items: Vec<(String, String, i64)> = tx
        .prepare(
            "SELECT pi.uuid, pi.event_item_uuid, pi.price FROM package_items pi
               JOIN event_items ei ON ei.uuid = pi.event_item_uuid
              WHERE pi.package_uuid = ?1 AND pi.active = 1
              ORDER BY ei.sort_order, ei.date_start, ei.time_start, pi.uuid",
        )?
        .query_map([plan_uuid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    let now = clock::now_text();
    for (pi, ei, price) in &items {
        let invoice = next_serial(tx, "registration_items", "invoice_number", FIRST_INVOICE)?;
        tx.execute(
            "INSERT INTO registration_items (uuid, registration_uuid, package_item_uuid, event_item_uuid, price, ticket_qty,
               qty_manual, invoice_number, created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8, ?9, ?8, ?9)",
            params![new_uuid(), registration_uuid, pi, ei, price, qty, invoice, now, who.actor.username],
        )?;
    }
    tx.execute(
        "UPDATE registration SET package_uuid = ?1, event_uuid = ?2 WHERE uuid = ?3",
        params![plan_uuid, plan_event, registration_uuid],
    )?;
    audit::stamp_modified(tx, "registration", "uuid", &registration_uuid, &who.actor)?;
    audit::log(
        tx,
        who.actor.user_id,
        "registration.plan_changed",
        Some("registration"),
        Some(registration_uuid),
        Some(
            &json!({ "old": reg.package_uuid, "new": plan_uuid, "items_removed": removed, "items_added": items.len() }),
        ),
    )?;
    Ok(items.len())
}

/// OP-REG-04 add: one event item of the registration's event (relationship 8). Price = event item price
/// (BR-REG-05), qty = guests + 1 (BR-REG-03). Returns the new item uuid.
pub fn add_item(tx: &Transaction, who: &Who, registration_uuid: &str, event_item_uuid: &str) -> DResult<String> {
    policy::ap03_domain(who)?;
    let reg = get(tx, registration_uuid)?;
    let event = reg.event_uuid.ok_or(DomainError::NoEvent)?;
    let (item_event, price): (String, i64) = tx
        .query_row("SELECT event_uuid, price FROM event_items WHERE uuid = ?1", [event_item_uuid], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?
        .ok_or(DomainError::NotFound)?;
    if item_event != event {
        return Err(DomainError::NotOfEvent);
    }
    let qty = guest_count(tx, registration_uuid)? + 1;
    let invoice = next_serial(tx, "registration_items", "invoice_number", FIRST_INVOICE)?;
    let uuid = new_uuid();
    let now = clock::now_text();
    tx.execute(
        "INSERT INTO registration_items (uuid, registration_uuid, event_item_uuid, price, ticket_qty, qty_manual,
           invoice_number, created_at, created_by, modified_at, modified_by)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, ?8, ?7, ?8)",
        params![uuid, registration_uuid, event_item_uuid, price, qty, invoice, now, who.actor.username],
    )?;
    Ok(uuid)
}

/// OP-REG-04 remove (AP-06, two-step confirm of 05 "Confirmations").
pub fn remove_item(
    tx: &Transaction,
    who: &Who,
    registration_uuid: &str,
    item_uuid: &str,
    confirm: bool,
) -> DResult<()> {
    policy::ap06_child_remove(who)?;
    item_of(tx, registration_uuid, item_uuid)?;
    if !confirm {
        return Err(DomainError::ConfirmRequired);
    }
    tx.execute("DELETE FROM registration_items WHERE uuid = ?1", [item_uuid])?;
    Ok(())
}

/// What the attributes dialog of an item edits (BR-REG-06).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttrKind {
    /// Reservation number and extra nights.
    Hotel,
    /// Table number.
    Seating,
    /// Nothing: the dialog does not open.
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemAttributes {
    pub kind: AttrKind,
    pub hotel_reservation_num: Option<String>,
    pub adicional: Option<String>,
    pub mesa: Option<String>,
}

/// `GET .../attributes`: the dialog's values. Read only: opening the dialog and pressing Cancel writes nothing
/// (BR-REG-06 changed, D-31).
pub fn item_attributes(conn: &Connection, registration_uuid: &str, item_uuid: &str) -> DResult<ItemAttributes> {
    item_of(conn, registration_uuid, item_uuid)?;
    Ok(conn.query_row(
        "SELECT ei.type, ri.hotel_reservation_num, ri.adicional, ri.mesa
           FROM registration_items ri JOIN event_items ei ON ei.uuid = ri.event_item_uuid WHERE ri.uuid = ?1",
        [item_uuid],
        |r| {
            let kind: String = r.get(0)?;
            Ok(ItemAttributes {
                kind: attr_kind(&kind),
                hotel_reservation_num: r.get(1)?,
                adicional: r.get(2)?,
                mesa: r.get(3)?,
            })
        },
    )?)
}

pub fn attr_kind(item_type: &str) -> AttrKind {
    if item_type == "Hotel" {
        AttrKind::Hotel
    } else if events::is_seating(item_type) {
        AttrKind::Seating
    } else {
        AttrKind::None
    }
}

/// The type check of an attribute PATCH (BR-REG-06): hotel fields only on Hotel items, the table only on seating
/// items. `autosave::registration::ITEM`'s check closure calls it.
pub fn attribute_allowed(conn: &Connection, item_uuid: &str, field: &str) -> DResult<()> {
    let kind: String = conn.query_row(
        "SELECT ei.type FROM registration_items ri JOIN event_items ei ON ei.uuid = ri.event_item_uuid WHERE ri.uuid = ?1",
        [item_uuid],
        |r| r.get(0),
    )?;
    let ok = match field {
        "hotel_reservation_num" | "adicional" => attr_kind(&kind) == AttrKind::Hotel,
        "mesa" => attr_kind(&kind) == AttrKind::Seating,
        _ => true,
    };
    if ok {
        Ok(())
    } else {
        Err(DomainError::Validation("attributes", "Esta actividad no lleva ese dato.".into()))
    }
}

pub fn guest_count(conn: &Connection, registration_uuid: &str) -> rusqlite::Result<i64> {
    count(conn, "SELECT COUNT(*) FROM registration_guests WHERE registration_uuid = ?1", registration_uuid)
}

#[derive(Debug, Default, Clone)]
pub struct NewGuest {
    pub name_first: String,
    pub name_last: String,
    pub name_badge: String,
    pub company: String,
}

/// OP-REG-06 add (BR-REG-11: a name only, never a person link) and BR-REG-03 in the same transaction: items with
/// `qty_manual` = 0 get guests + 1 (D-88). Returns the guest uuid.
pub fn add_guest(tx: &Transaction, who: &Who, registration_uuid: &str, g: &NewGuest) -> DResult<String> {
    policy::ap03_domain(who)?;
    get(tx, registration_uuid)?;
    let first = optional("name_first", &g.name_first, 100)?;
    let last = optional("name_last", &g.name_last, 100)?;
    let badge = optional("name_badge", &g.name_badge, 100)?
        .or_else(|| Some(super::people::full_name(first.as_deref(), last.as_deref())).filter(|s| !s.is_empty()));
    if first.is_none() && last.is_none() && badge.is_none() {
        return Err(DomainError::Validation("name_first", "Escriba el nombre del acompañante.".into()));
    }
    let uuid = new_uuid();
    let now = clock::now_text();
    tx.execute(
        "INSERT INTO registration_guests (uuid, registration_uuid, name_first, name_last, name_badge, company,
           created_at, created_by, modified_at, modified_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?7, ?8)",
        params![
            uuid,
            registration_uuid,
            first,
            last,
            badge,
            optional("company", &g.company, 200)?,
            now,
            who.actor.username
        ],
    )?;
    recompute_guest_qty(tx, registration_uuid, &who.actor)?;
    Ok(uuid)
}

/// OP-REG-06 remove (AP-06) and BR-REG-03 in the same transaction.
pub fn remove_guest(tx: &Transaction, who: &Who, registration_uuid: &str, guest_uuid: &str) -> DResult<()> {
    policy::ap06_child_remove(who)?;
    let n = tx.execute(
        "DELETE FROM registration_guests WHERE uuid = ?1 AND registration_uuid = ?2",
        params![guest_uuid, registration_uuid],
    )?;
    if n == 0 {
        return Err(DomainError::NotFound);
    }
    recompute_guest_qty(tx, registration_uuid, &who.actor)?;
    Ok(())
}

/// After a guest field PATCH (`registries::GUEST`): BR-REG-03 again (05: the badge edit recomputes). The guest count
/// does not change on an edit, so this normally writes nothing; it repairs any drift.
pub fn after_guest_edit(conn: &Connection, who: &Who, registration_uuid: &str) -> DResult<usize> {
    Ok(recompute_guest_qty(conn, registration_uuid, &who.actor)?)
}

/// OP-REG-07: a payment. Amount > 0 (changed: strengthened), type from the list. Returns the payment uuid.
pub fn add_payment(
    tx: &Transaction,
    who: &Who,
    registration_uuid: &str,
    amount: &str,
    payment_type: &str,
    number: &str,
) -> DResult<String> {
    policy::ap03_domain(who)?;
    get(tx, registration_uuid)?;
    let cents = autosave::cents(amount).map_err(|m| DomainError::Validation("payment", m))?;
    if cents <= 0 {
        return Err(DomainError::Validation("payment", "El pago debe ser mayor que $0.00.".into()));
    }
    if !PAYMENT_TYPES.contains(&payment_type) {
        return Err(DomainError::Validation("payment_type", "Escoja el tipo de pago.".into()));
    }
    let uuid = new_uuid();
    let now = clock::now_text();
    tx.execute(
        "INSERT INTO payments (uuid, registration_uuid, payment, payment_type, payment_number, created_at, created_by, modified_at, modified_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?6, ?7)",
        params![uuid, registration_uuid, cents, payment_type, optional("payment_number", number, 60)?, now, who.actor.username],
    )?;
    Ok(uuid)
}

/// OP-REG-08 / BR-REG-09: void a payment. ROLE-ADMIN with a reason (AP-05); once. The voided payment no longer
/// counts in `v_registration_totals`. Writes `payment.void` to the audit trail.
pub fn void_payment(tx: &Transaction, who: &Who, payment_uuid: &str, reason: &str) -> DResult<()> {
    policy::ap05_payment_void(who, reason)?;
    let voided: Option<String> = tx
        .query_row("SELECT voided_at FROM payments WHERE uuid = ?1", [payment_uuid], |r| r.get(0))
        .optional()?
        .ok_or(DomainError::NotFound)?;
    if voided.is_some() {
        return Err(DomainError::AlreadyVoided);
    }
    let reason = reason.trim().chars().take(500).collect::<String>();
    let now = clock::now_text();
    tx.execute(
        "UPDATE payments SET voided_at = ?1, voided_by = ?2, void_reason = ?3, modified_at = ?1, modified_by = ?2 WHERE uuid = ?4",
        params![now, who.actor.username, reason, payment_uuid],
    )?;
    audit::log(
        tx,
        who.actor.user_id,
        "payment.void",
        Some("payments"),
        Some(payment_uuid),
        Some(&json!({ "reason": reason })),
    )?;
    Ok(())
}

/// OP-REG-09 cancel: status Canceló (AP-05: both roles; the way out when delete is refused).
pub fn cancel_registration(tx: &Transaction, who: &Who, registration_uuid: &str) -> DResult<()> {
    policy::ap05_registration_cancel(who)?;
    get(tx, registration_uuid)?;
    tx.execute("UPDATE registration SET status = 'Canceló' WHERE uuid = ?1", [registration_uuid])?;
    audit::stamp_modified(tx, "registration", "uuid", &registration_uuid, &who.actor)?;
    audit::log(tx, who.actor.user_id, "registration.cancel", Some("registration"), Some(registration_uuid), None)?;
    Ok(())
}

/// OP-REG-09 delete: refused when the registration has any payment row (409 with the cancel offer, T-05-04,
/// T-08-09); else ROLE-ADMIN only (AP-05). Items and guests cascade (04).
pub fn delete_registration(tx: &Transaction, who: &Who, registration_uuid: &str) -> DResult<()> {
    get(tx, registration_uuid)?;
    let payments = count(tx, "SELECT COUNT(*) FROM payments WHERE registration_uuid = ?1", registration_uuid)?;
    policy::ap05_registration_delete(who, payments)?;
    tx.execute("DELETE FROM registration WHERE uuid = ?1", [registration_uuid])?;
    audit::log(tx, who.actor.user_id, "registration.delete", Some("registration"), Some(registration_uuid), None)?;
    Ok(())
}

/// OP-REG-10 door check-in (UI-07): Asistió / No Asistió / Canceló. `registration.status` has no "No Asistió"
/// (04 CHECK), so it is stored as "No Canceló" (did not cancel, did not come); see the open point in the I2 result.
/// Returns the stored status.
pub fn set_attendance(
    tx: &Transaction,
    who: &Who,
    registration_uuid: &str,
    door_status: &str,
) -> DResult<&'static str> {
    policy::ap03_domain(who)?;
    get(tx, registration_uuid)?;
    let stored = match door_status {
        "Asistió" => "Asistió",
        "No Asistió" => "No Canceló",
        "Canceló" => "Canceló",
        _ => return Err(DomainError::Validation("status", "Escoja Asistió, No Asistió o Canceló.".into())),
    };
    tx.execute("UPDATE registration SET status = ?1 WHERE uuid = ?2", params![stored, registration_uuid])?;
    audit::stamp_modified(tx, "registration", "uuid", &registration_uuid, &who.actor)?;
    Ok(stored)
}

/// The door button that shows a stored status as pressed.
pub fn door_status_of(status: &str) -> Option<&'static str> {
    match status {
        "Asistió" => Some("Asistió"),
        "No Canceló" => Some("No Asistió"),
        "Canceló" => Some("Canceló"),
        _ => None,
    }
}

/// `v_registration_totals` (BR-REG-04, -07, -08). Money in cents.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Totals {
    pub total: i64,
    pub payment_total: i64,
    pub balance: i64,
    pub guests_count: i64,
    pub has_hotel: bool,
    pub registration_type: String,
    pub arrival_date: Option<String>,
    pub departure_date: Option<String>,
}

pub fn totals(conn: &Connection, registration_uuid: &str) -> DResult<Totals> {
    Ok(conn.query_row(
        "SELECT total, payment_total, balance, guests_count, has_hotel, registration_type, arrival_date, departure_date
           FROM v_registration_totals WHERE registration_uuid = ?1",
        [registration_uuid],
        |r| {
            Ok(Totals {
                total: r.get(0)?,
                payment_total: r.get(1)?,
                balance: r.get(2)?,
                guests_count: r.get(3)?,
                has_hotel: r.get::<_, i64>(4)? == 1,
                registration_type: r.get(5)?,
                arrival_date: r.get(6)?,
                departure_date: r.get(7)?,
            })
        },
    )?)
}

/// One line of `v_registration_item_lines` with the registration item's own fields (UI-06 items portal).
#[derive(Debug, Clone)]
pub struct Line {
    pub uuid: String,
    pub event_item_name: String,
    pub event_type: String,
    pub date_start: Option<String>,
    pub date_end: Option<String>,
    pub time_start: Option<String>,
    pub time_end: Option<String>,
    pub from_package: bool,
    pub ticket_qty: i64,
    pub qty_manual: bool,
    pub price: i64,
    pub total: i64,
    pub no_charge: bool,
    pub hotel_reservation_num: Option<String>,
    pub adicional: Option<String>,
    pub mesa: Option<String>,
}

pub fn lines(conn: &Connection, registration_uuid: &str) -> rusqlite::Result<Vec<Line>> {
    conn.prepare(
        "SELECT l.uuid, l.event_item_name, l.event_type, l.date_start, l.date_end, l.time_start, l.time_end,
                l.from_package, l.ticket_qty, ri.qty_manual, l.price, l.total, ri.no_charge,
                ri.hotel_reservation_num, ri.adicional, ri.mesa
           FROM v_registration_item_lines l JOIN registration_items ri ON ri.uuid = l.uuid
           JOIN event_items ei ON ei.uuid = l.event_item_uuid
          WHERE l.registration_uuid = ?1
          ORDER BY l.date_start, l.time_start, ei.sort_order, l.uuid",
    )?
    .query_map([registration_uuid], |r| {
        Ok(Line {
            uuid: r.get(0)?,
            event_item_name: r.get(1)?,
            event_type: r.get(2)?,
            date_start: r.get(3)?,
            date_end: r.get(4)?,
            time_start: r.get(5)?,
            time_end: r.get(6)?,
            from_package: r.get::<_, i64>(7)? == 1,
            ticket_qty: r.get(8)?,
            qty_manual: r.get::<_, i64>(9)? == 1,
            price: r.get(10)?,
            total: r.get(11)?,
            no_charge: r.get::<_, i64>(12)? == 1,
            hotel_reservation_num: r.get(13)?,
            adicional: r.get(14)?,
            mesa: r.get(15)?,
        })
    })?
    .collect()
}

fn item_of(conn: &Connection, registration_uuid: &str, item_uuid: &str) -> DResult<()> {
    conn.query_row(
        "SELECT 1 FROM registration_items WHERE uuid = ?1 AND registration_uuid = ?2",
        params![item_uuid, registration_uuid],
        |_| Ok(()),
    )
    .optional()?
    .ok_or(DomainError::NotFound)
}
