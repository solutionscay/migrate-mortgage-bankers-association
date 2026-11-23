//! Registry entries for the registration screen (UI-06 is I2; these are the parts I1 owns).
//!
//! D-88 / BR-REG-03: a PATCH of an item's qty sets `registration_items.qty_manual = 1`; the guest recompute writes
//! only items with `qty_manual = 0`, so a hand-set qty survives a guest change. I2's item and guest routes call
//! [`ITEM`] through [`super::save`] and [`recompute_guest_qty`] inside their guest transaction.

use super::{int_in, FieldDef, Registry, Stamp};
use crate::audit::{self, Actor};
use rusqlite::types::Value;
use rusqlite::Connection;

/// `PATCH /registrations/{id}/items/{item_id}`: the line fields (OP-REG-05).
pub static ITEM: Registry = Registry {
    table: "registration_items",
    key_col: "uuid",
    stamp: Stamp::Full,
    fields: &[
        FieldDef::new("qty", "ticket_qty", "Qty.", parse_qty).also(&[("qty_manual", 1)]),
        // OP-REG-05: price, no charge and the BR-REG-06 attributes. The type check of the attributes (hotel fields on
        // Hotel items, the table on seating items) runs in `convention::registries::patch`.
        FieldDef::new("price", "price", "Precio", parse_price).show(show_price),
        FieldDef::new("no_charge", "no_charge", "Sin cargo", super::flag),
        FieldDef::new("hotel_reservation_num", "hotel_reservation_num", "Reserva", attr_text),
        FieldDef::new("adicional", "adicional", "Noches adicionales", attr_text),
        FieldDef::new("mesa", "mesa", "Mesa", attr_text),
    ],
};

/// Line price in cents (BR-REG-05: stored once; a hand edit replaces it).
fn parse_price(s: &str) -> Result<Value, String> {
    super::cents(s).map(Value::Integer)
}

fn show_price(v: &Value) -> String {
    match v {
        Value::Integer(c) => super::dollars(*c),
        _ => String::new(),
    }
}

fn attr_text(s: &str) -> Result<Value, String> {
    super::optional_text(s, 60)
}

/// Qty of a line: a whole number from 1 to 999 (05 T-05-14: 0 is refused).
fn parse_qty(s: &str) -> Result<Value, String> {
    int_in(s, 1, 999).map(Value::Integer)
}

/// BR-REG-03: every item of the registration with `qty_manual = 0` gets qty = guests + 1. Items set by hand keep
/// their qty (D-88). Call it inside the guest change's transaction. Returns the number of items written.
pub fn recompute_guest_qty(conn: &Connection, registration_uuid: &str, actor: &Actor) -> rusqlite::Result<usize> {
    let uuids: Vec<String> = conn
        .prepare(
            "SELECT uuid FROM registration_items
              WHERE registration_uuid = ?1 AND qty_manual = 0
                AND ticket_qty <> (SELECT COUNT(*) FROM registration_guests WHERE registration_uuid = ?1) + 1",
        )?
        .query_map([registration_uuid], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for uuid in &uuids {
        conn.execute(
            "UPDATE registration_items
                SET ticket_qty = (SELECT COUNT(*) FROM registration_guests WHERE registration_uuid = ?1) + 1
              WHERE uuid = ?2",
            [registration_uuid, uuid],
        )?;
        audit::stamp_modified(conn, "registration_items", "uuid", uuid, actor)?;
    }
    Ok(uuids.len())
}
