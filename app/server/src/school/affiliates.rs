//! AFF: member companies (OP-AFF-01; BR-AFF-01, BR-AFF-02) and the Google Maps link (EXT-02).

use super::{count, new_uuid, next_serial, policy, DResult, DomainError, Who};
use crate::clock;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

/// `affiliates.cust_type` values (value list `activo_inactivo`). ACTIVO gives member prices (BR-AFF-01).
pub const CUST_TYPES: &[&str] = &["ACTIVO", "INACTIVO", "No Afiliado"];
/// `affiliates.miembro` values (value list `affiliates_membresia`).
pub const MEMBERSHIPS: &[&str] = &["REGULAR", "ASOCIADO", "CORRESPONSAL"];

/// OP-AFF-01 create: the member company name; next account number. Other fields by autosave.
pub fn create_affiliate(tx: &Transaction, who: &Who, socio: &str) -> DResult<String> {
    policy::ap03_domain(who)?;
    let socio = crate::convention::required("socio", socio, 200)?;
    let uuid = new_uuid();
    let account = next_serial(tx, "affiliates", "account_num", 1000)?;
    let now = clock::now_text();
    tx.execute(
        "INSERT INTO affiliates (uuid, account_num, socio, cust_type, created_at, created_by, modified_at, modified_by)
         VALUES (?1, ?2, ?3, 'ACTIVO', ?4, ?5, ?4, ?5)",
        params![uuid, account, socio, now, who.actor.username],
    )?;
    Ok(uuid)
}

/// OP-AFF-01 delete: ROLE-ADMIN (AP-04). Its students are unlinked (`member_uuid` NULL by the 04 foreign key) and
/// keep their typed company name (BR-AFF-02). Returns the number of students unlinked.
pub fn delete_affiliate(tx: &Transaction, who: &Who, uuid: &str) -> DResult<i64> {
    tx.query_row("SELECT 1 FROM affiliates WHERE uuid = ?1", [uuid], |_| Ok(()))
        .optional()?
        .ok_or(DomainError::NotFound)?;
    policy::ap04_admin(who)?;
    let unlinked = count(tx, "SELECT COUNT(*) FROM students WHERE member_uuid = ?1", uuid)?;
    tx.execute("DELETE FROM affiliates WHERE uuid = ?1", [uuid])?;
    crate::audit::log(
        tx,
        who.actor.user_id,
        "affiliate.delete",
        Some("affiliates"),
        Some(uuid),
        Some(&serde_json::json!({ "students_unlinked": unlinked })),
    )?;
    Ok(unlinked)
}

/// BR-AFF-01: the affiliate is active (its students pay member prices).
pub fn is_active(conn: &Connection, uuid: &str) -> rusqlite::Result<bool> {
    Ok(conn
        .query_row("SELECT cust_type FROM affiliates WHERE uuid = ?1", [uuid], |r| r.get::<_, Option<String>>(0))
        .optional()?
        .flatten()
        .as_deref()
        == Some("ACTIVO"))
}

/// `GET /affiliates/options?q=`: typeahead of the student form (uuid, name, active), up to 20.
pub fn options(conn: &Connection, q: &str) -> rusqlite::Result<Vec<(String, String, bool)>> {
    conn.prepare(
        "SELECT uuid, socio, cust_type = 'ACTIVO' FROM affiliates
          WHERE ?1 = '' OR socio LIKE '%' || ?1 || '%' OR other_name LIKE '%' || ?1 || '%'
          ORDER BY socio LIMIT 20",
    )?
    .query_map([q.trim()], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, Option<bool>>(2)?.unwrap_or(false))))?
    .collect()
}

/// "A-1001" (the account number as the design shows it).
pub fn account_label(n: Option<i64>) -> String {
    n.map(|n| format!("A-{n}")).unwrap_or_default()
}

/// EXT-02: a Google Maps search link for an address. The server builds the query; no server call. `None` when the
/// address is empty.
pub fn maps_url(parts: &[Option<&str>]) -> Option<String> {
    let addr = parts.iter().flatten().map(|s| s.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ");
    if addr.is_empty() {
        return None;
    }
    let q: String = addr
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "+".to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect();
    Some(format!("https://www.google.com/maps/search/?api=1&query={q}"))
}
