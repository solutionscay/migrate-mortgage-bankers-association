//! EVT: events and event items (OP-EVT-01..03; BR-EVT-01..04), event totals from `v_event_totals`.

use super::{count, iso_date, new_uuid, optional, policy, required, DResult, DomainError, Who};
use crate::autosave;
use crate::clock;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

/// Event item types (`event_items.type` CHECK, value list `event_item_types`).
pub const ITEM_TYPES: &[&str] = &["Hotel", "Seminario", "Foro", "Desayuno", "Almuerzo", "Cena", "Cocktail", "Ajuste"];

/// Seating items take a table number (BR-REG-06; `v_registration_item_lines.is_seating`).
pub fn is_seating(kind: &str) -> bool {
    matches!(kind, "Cena" | "Almuerzo" | "Seminario" | "Desayuno")
}

#[derive(Debug, Default, Clone)]
pub struct NewEvent {
    pub name: String,
    /// `YYYY-MM-DD`, optional.
    pub date_start: String,
    pub date_end: String,
    pub location: String,
}

/// OP-EVT-01 create: status Activo (BR-EVT-02); an empty end date takes the start date (BR-EVT-01).
pub fn create_event(tx: &Transaction, who: &Who, e: &NewEvent) -> DResult<String> {
    policy::ap03_domain(who)?;
    let name = required("name", &e.name, 200)?;
    let start = if e.date_start.trim().is_empty() { None } else { Some(iso_date("date_start", &e.date_start)?) };
    let end = if e.date_end.trim().is_empty() { start.clone() } else { Some(iso_date("date_end", &e.date_end)?) };
    let location = optional("location", &e.location, 200)?;
    let uuid = new_uuid();
    let now = clock::now_text();
    tx.execute(
        "INSERT INTO events (uuid, name, date_start, date_end, location, status, created_at, created_by, modified_at, modified_by)
         VALUES (?1, ?2, ?3, ?4, ?5, 'Activo', ?6, ?7, ?6, ?7)",
        params![uuid, name, start, end, location, now, who.actor.username],
    )?;
    Ok(uuid)
}

/// BR-EVT-01 after an autosave of `date_start` or `date_end`: an empty end date takes the start date. Returns true
/// when it wrote. The event PATCH handler calls it in the same request, right after `autosave::save`.
pub fn default_end_date(conn: &Connection, who: &Who, event_uuid: &str) -> rusqlite::Result<bool> {
    let n = conn.execute(
        "UPDATE events SET date_end = date_start WHERE uuid = ?1 AND (date_end IS NULL OR date_end = '') AND date_start IS NOT NULL",
        [event_uuid],
    )?;
    if n > 0 {
        crate::audit::stamp_modified(conn, "events", "uuid", &event_uuid, &who.actor)?;
    }
    Ok(n > 0)
}

pub fn registration_count(conn: &Connection, event_uuid: &str) -> rusqlite::Result<i64> {
    count(conn, "SELECT COUNT(*) FROM registration WHERE event_uuid = ?1", event_uuid)
}

/// OP-EVT-01 delete: ROLE-ADMIN, only without registrations (AP-04). Items, sponsors and billing rows cascade
/// (04 ON DELETE CASCADE); plans block the delete through their foreign key, so they go first.
pub fn delete_event(tx: &Transaction, who: &Who, event_uuid: &str) -> DResult<()> {
    exists(tx, event_uuid)?;
    policy::ap04_parent_delete(who, registration_count(tx, event_uuid)?)?;
    tx.execute("DELETE FROM packages WHERE event_uuid = ?1", [event_uuid])?;
    tx.execute("DELETE FROM events WHERE uuid = ?1", [event_uuid])?;
    crate::audit::log(tx, who.actor.user_id, "event.delete", Some("events"), Some(event_uuid), None)?;
    Ok(())
}

/// OP-EVT-01: attach the event PDF (a `stored_files` uuid from OP-SYS-04). PDF only.
pub fn set_event_pdf(tx: &Transaction, who: &Who, event_uuid: &str, file_uuid: &str) -> DResult<()> {
    policy::ap03_domain(who)?;
    exists(tx, event_uuid)?;
    let mime: Option<String> =
        tx.query_row("SELECT mime_type FROM stored_files WHERE uuid = ?1", [file_uuid], |r| r.get(0)).optional()?;
    if mime.as_deref() != Some("application/pdf") {
        return Err(DomainError::Validation("pdf_file_uuid", "Use un archivo PDF.".into()));
    }
    tx.execute("UPDATE events SET pdf_file_uuid = ?1 WHERE uuid = ?2", params![file_uuid, event_uuid])?;
    crate::audit::stamp_modified(tx, "events", "uuid", &event_uuid, &who.actor)?;
    Ok(())
}

/// OP-EVT-01: remove the event PDF (the `stored_files` row stays; files are never deleted in the demo).
pub fn remove_event_pdf(tx: &Transaction, who: &Who, event_uuid: &str) -> DResult<()> {
    policy::ap03_domain(who)?;
    exists(tx, event_uuid)?;
    tx.execute("UPDATE events SET pdf_file_uuid = NULL WHERE uuid = ?1", [event_uuid])?;
    crate::audit::stamp_modified(tx, "events", "uuid", &event_uuid, &who.actor)?;
    Ok(())
}

/// The "Añadir actividad" row of UI-09.
#[derive(Debug, Default, Clone)]
pub struct NewEventItem {
    pub name: String,
    pub kind: String,
    pub date_start: String,
    pub time_start: String,
    pub time_end: String,
    pub location_room: String,
    /// Dollar text.
    pub price: String,
    pub qty: String,
}

/// OP-EVT-03 add: `sort_order` = items of the event + 1 (BR-EVT-03); `date_end` = `date_start` (BR-EVT-01).
pub fn add_event_item(tx: &Transaction, who: &Who, event_uuid: &str, it: &NewEventItem) -> DResult<String> {
    policy::ap03_domain(who)?;
    exists(tx, event_uuid)?;
    let name = required("name", &it.name, 200)?;
    if !ITEM_TYPES.contains(&it.kind.as_str()) {
        return Err(DomainError::Validation("type", "Escoja un tipo de la lista.".into()));
    }
    let date = if it.date_start.trim().is_empty() { None } else { Some(iso_date("date_start", &it.date_start)?) };
    let price = if it.price.trim().is_empty() {
        0
    } else {
        autosave::cents(&it.price).map_err(|m| DomainError::Validation("price", m))?
    };
    let qty = if it.qty.trim().is_empty() {
        1
    } else {
        autosave::int_in(&it.qty, 1, 999).map_err(|m| DomainError::Validation("qty", m))?
    };
    let sort = count(tx, "SELECT COUNT(*) FROM event_items WHERE event_uuid = ?1", event_uuid)? + 1;
    let uuid = new_uuid();
    let now = clock::now_text();
    tx.execute(
        "INSERT INTO event_items (uuid, event_uuid, name, date_start, date_end, time_start, time_end, location_room, qty,
           price, type, sort_order, created_at, created_by, modified_at, modified_by)
         VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?12, ?13)",
        params![
            uuid,
            event_uuid,
            name,
            date,
            optional("time_start", &it.time_start, 10)?,
            optional("time_end", &it.time_end, 10)?,
            optional("location_room", &it.location_room, 100)?,
            qty,
            price,
            it.kind,
            sort,
            now,
            who.actor.username
        ],
    )?;
    Ok(uuid)
}

/// OP-EVT-03 delete (AP-06): refused while a registration item uses it (FK; 409 with the count). Its plan items
/// go with it (a plan item of a deleted activity has no meaning).
pub fn delete_event_item(tx: &Transaction, who: &Who, item_uuid: &str) -> DResult<()> {
    policy::ap06_child_remove(who)?;
    tx.query_row("SELECT 1 FROM event_items WHERE uuid = ?1", [item_uuid], |_| Ok(()))?;
    let used = count(tx, "SELECT COUNT(*) FROM registration_items WHERE event_item_uuid = ?1", item_uuid)?;
    if used > 0 {
        return Err(DomainError::InUse(used));
    }
    tx.execute("DELETE FROM package_items WHERE event_item_uuid = ?1", [item_uuid])?;
    tx.execute("DELETE FROM event_items WHERE uuid = ?1", [item_uuid])?;
    Ok(())
}

/// Events with status Activo (BR-EVT-02), by start date: the registration event picker and the plan event select.
pub fn active_events(conn: &Connection) -> rusqlite::Result<Vec<(String, String, Option<String>)>> {
    conn.prepare("SELECT uuid, name, date_start FROM events WHERE status = 'Activo' ORDER BY date_start, name")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect()
}

/// `v_event_totals` (BR-EVT-04, BR-SPN-01). Money in cents.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventTotals {
    pub count_registrations: i64,
    pub count_guests: i64,
    pub count_total: i64,
    pub total: i64,
    pub payments: i64,
    pub balance: i64,
    pub price: i64,
    pub sponsor_promised: i64,
    pub sponsor_received: i64,
    pub sponsor_total: i64,
}

pub fn event_totals(conn: &Connection, event_uuid: &str) -> DResult<EventTotals> {
    Ok(conn.query_row(
        "SELECT count_registrations, count_guests, count_total, total, payments, balance, price,
                sponsor_promised, sponsor_received, sponsor_total
           FROM v_event_totals WHERE event_uuid = ?1",
        [event_uuid],
        |r| {
            Ok(EventTotals {
                count_registrations: r.get(0)?,
                count_guests: r.get(1)?,
                count_total: r.get(2)?,
                total: r.get(3)?,
                payments: r.get(4)?,
                balance: r.get(5)?,
                price: r.get(6)?,
                sponsor_promised: r.get(7)?,
                sponsor_received: r.get(8)?,
                sponsor_total: r.get(9)?,
            })
        },
    )?)
}

fn exists(conn: &Connection, uuid: &str) -> DResult<()> {
    conn.query_row("SELECT 1 FROM events WHERE uuid = ?1", [uuid], |_| Ok(())).optional()?.ok_or(DomainError::NotFound)
}
