//! Plans (packages) and plan items: OP-EVT-04 build, OP-EVT-05 create/edit/delete; BR-EVT-05..07. The plan price is
//! `v_package_price` (BR-EVT-05), never stored.

use super::{count, new_uuid, policy, required, DResult, DomainError, Who};
use crate::clock;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

/// OP-EVT-05 create: a plan of an active event (BR-EVT-02), then its plan items (OP-EVT-04, as the source trigger
/// does when the event is set). Returns (plan uuid, items added).
pub fn create_plan(tx: &Transaction, who: &Who, event_uuid: &str, name: &str) -> DResult<(String, usize)> {
    policy::ap03_domain(who)?;
    let name = required("name", name, 200)?;
    check_active_event(tx, event_uuid)?;
    let uuid = new_uuid();
    let now = clock::now_text();
    tx.execute(
        "INSERT INTO packages (uuid, event_uuid, name, created_at, created_by, modified_at, modified_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?4, ?5)",
        params![uuid, event_uuid, name, now, who.actor.username],
    )?;
    let added = match build_plan_items(tx, who, &uuid) {
        Ok(n) => n,
        Err(DomainError::NoEventItems) => 0, // the plan exists; UI-13 shows the "sin detalles" message
        Err(e) => return Err(e),
    };
    Ok((uuid, added))
}

/// The event of a plan must exist and be Activo (BR-EVT-02: only active events are offered to plans).
pub fn check_active_event(conn: &Connection, event_uuid: &str) -> DResult<()> {
    let status: Option<String> =
        conn.query_row("SELECT status FROM events WHERE uuid = ?1", [event_uuid], |r| r.get(0)).optional()?;
    match status.as_deref() {
        Some("Activo") => Ok(()),
        Some(_) => Err(DomainError::Validation("event_uuid", "Escoja un evento activo.".into())),
        None => Err(DomainError::NoEvent),
    }
}

/// OP-EVT-04 / BR-EVT-06: one plan item per event item of the plan's event that the plan does not have yet, in
/// event-item order; price and qty copied from the event item (BR-EVT-07), inactive until ticked. Returns the
/// number added; a second run adds nothing (UNIQUE pair). AP-09: the scope is the plan, fixed by the server.
pub fn build_plan_items(tx: &Transaction, who: &Who, plan_uuid: &str) -> DResult<usize> {
    policy::ap09_bulk(who)?;
    let event: Option<String> = tx
        .query_row("SELECT event_uuid FROM packages WHERE uuid = ?1", [plan_uuid], |r| r.get(0))
        .optional()?
        .ok_or(DomainError::NotFound)?;
    let event = event.filter(|e| !e.is_empty()).ok_or(DomainError::NoEvent)?;
    if count(tx, "SELECT COUNT(*) FROM event_items WHERE event_uuid = ?1", &event)? == 0 {
        return Err(DomainError::NoEventItems);
    }
    let missing: Vec<(String, i64, i64)> = tx
        .prepare(
            "SELECT ei.uuid, ei.price, ei.qty FROM event_items ei
              WHERE ei.event_uuid = ?1
                AND NOT EXISTS (SELECT 1 FROM package_items pi WHERE pi.package_uuid = ?2 AND pi.event_item_uuid = ei.uuid)
              ORDER BY ei.sort_order, ei.date_start, ei.time_start, ei.uuid",
        )?
        .query_map(params![event, plan_uuid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    let now = clock::now_text();
    for (ei, price, qty) in &missing {
        tx.execute(
            "INSERT INTO package_items (uuid, package_uuid, event_item_uuid, active, price, ticket_qty, created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6, ?7, ?6, ?7)",
            params![new_uuid(), plan_uuid, ei, price, qty, now, who.actor.username],
        )?;
    }
    Ok(missing.len())
}

/// `PATCH /plans/{id}` with `event_uuid`, after the write: build the plan items of the new event (the OnObjectSave
/// trigger of layout 40). `registries::patch` calls it right after `autosave::save` returns `Saved { changed: true }`.
/// Plan items of the old event stay (the source only adds); see the open point in the I2 result.
pub fn after_event_change(tx: &Transaction, who: &Who, plan_uuid: &str) -> DResult<usize> {
    match build_plan_items(tx, who, plan_uuid) {
        Err(DomainError::NoEventItems) => Ok(0),
        r => r,
    }
}

/// OP-EVT-05: remove a plan item (AP-06). Registration items that came from it keep their row and lose the link
/// (04 ON DELETE SET NULL), so they turn into charged lines; the UI confirms first.
pub fn delete_plan_item(tx: &Transaction, who: &Who, plan_item_uuid: &str) -> DResult<()> {
    policy::ap06_child_remove(who)?;
    let n = tx.execute("DELETE FROM package_items WHERE uuid = ?1", [plan_item_uuid])?;
    if n == 0 {
        return Err(DomainError::NotFound);
    }
    Ok(())
}

pub fn registration_count(conn: &Connection, plan_uuid: &str) -> rusqlite::Result<i64> {
    count(conn, "SELECT COUNT(*) FROM registration WHERE package_uuid = ?1", plan_uuid)
}

/// OP-EVT-05 delete: ROLE-ADMIN, refused when a registration uses the plan (AP-04). Plan items cascade.
pub fn delete_plan(tx: &Transaction, who: &Who, plan_uuid: &str) -> DResult<()> {
    tx.query_row("SELECT 1 FROM packages WHERE uuid = ?1", [plan_uuid], |_| Ok(()))
        .optional()?
        .ok_or(DomainError::NotFound)?;
    policy::ap04_parent_delete(who, registration_count(tx, plan_uuid)?)?;
    tx.execute("DELETE FROM packages WHERE uuid = ?1", [plan_uuid])?;
    crate::audit::log(tx, who.actor.user_id, "plan.delete", Some("packages"), Some(plan_uuid), None)?;
    Ok(())
}

/// BR-EVT-05: `v_package_price.price` in cents.
pub fn plan_price(conn: &Connection, plan_uuid: &str) -> DResult<i64> {
    Ok(conn.query_row("SELECT price FROM v_package_price WHERE package_uuid = ?1", [plan_uuid], |r| r.get(0))?)
}

/// Plans of an event, with their live price (the plan picker of UI-06, UI-12).
pub fn plans_of_event(conn: &Connection, event_uuid: &str) -> rusqlite::Result<Vec<(String, String, i64)>> {
    conn.prepare(
        "SELECT p.uuid, p.name, v.price FROM packages p JOIN v_package_price v ON v.package_uuid = p.uuid
          WHERE p.event_uuid = ?1 ORDER BY p.name",
    )?
    .query_map([event_uuid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
    .collect()
}
