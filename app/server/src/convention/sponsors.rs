//! SPN: sponsors of an event (OP-SPN-01, BR-SPN-01) and billing files (OP-SPN-02, BR-SPN-02, D-16).

use super::{new_uuid, policy, required, DResult, DomainError, Who};
use crate::clock;
use rusqlite::{params, OptionalExtension, Transaction};

/// Sponsor categories (`sponsor.type` CHECK, value list `categoria_auspiciador`).
pub const CATEGORIES: &[&str] = &["Platino", "Oro", "Plata", "Bronce", "Otro"];

/// OP-SPN-01 add (the "Añadir" row of the Auspiciadores tab): name and category; amounts by autosave.
pub fn create_sponsor(tx: &Transaction, who: &Who, event_uuid: &str, name: &str, category: &str) -> DResult<String> {
    policy::ap03_domain(who)?;
    let name = required("sponsor_name", name, 200)?;
    let category = if category.trim().is_empty() { None } else { Some(category.trim()) };
    if category.is_some_and(|c| !CATEGORIES.contains(&c)) {
        return Err(DomainError::Validation("type", "Escoja una categoría de la lista.".into()));
    }
    tx.query_row("SELECT 1 FROM events WHERE uuid = ?1", [event_uuid], |_| Ok(()))
        .optional()?
        .ok_or(DomainError::NotFound)?;
    let uuid = new_uuid();
    let now = clock::now_text();
    tx.execute(
        "INSERT INTO sponsor (uuid, sponsor_name, type, event_uuid, created_at, created_by, modified_at, modified_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?5, ?6)",
        params![uuid, name, category, event_uuid, now, who.actor.username],
    )?;
    Ok(uuid)
}

/// OP-SPN-01 delete (AP-06: a sponsor row carries promises, not payments).
pub fn delete_sponsor(tx: &Transaction, who: &Who, sponsor_uuid: &str) -> DResult<()> {
    policy::ap06_child_remove(who)?;
    if tx.execute("DELETE FROM sponsor WHERE uuid = ?1", [sponsor_uuid])? == 0 {
        return Err(DomainError::NotFound);
    }
    Ok(())
}

/// OP-SPN-02 upload: a billing row for a stored PDF (OP-SYS-04 stores the bytes first).
pub fn add_billing(tx: &Transaction, who: &Who, event_uuid: &str, file_uuid: &str) -> DResult<String> {
    policy::ap03_domain(who)?;
    tx.query_row("SELECT 1 FROM events WHERE uuid = ?1", [event_uuid], |_| Ok(()))
        .optional()?
        .ok_or(DomainError::NotFound)?;
    let mime: Option<String> =
        tx.query_row("SELECT mime_type FROM stored_files WHERE uuid = ?1", [file_uuid], |r| r.get(0)).optional()?;
    if mime.as_deref() != Some("application/pdf") {
        return Err(DomainError::Validation("file_uuid", "Use un archivo PDF.".into()));
    }
    let uuid = new_uuid();
    let now = clock::now_text();
    tx.execute(
        "INSERT INTO billing (uuid, event_uuid, timestamp, file_uuid, created_at, created_by, modified_at, modified_by)
         VALUES (?1, ?2, ?3, ?4, ?3, ?5, ?3, ?5)",
        params![uuid, event_uuid, now, file_uuid, who.actor.username],
    )?;
    Ok(uuid)
}

/// OP-SPN-02 remove (AP-06).
pub fn delete_billing(tx: &Transaction, who: &Who, billing_uuid: &str) -> DResult<()> {
    policy::ap06_child_remove(who)?;
    if tx.execute("DELETE FROM billing WHERE uuid = ?1", [billing_uuid])? == 0 {
        return Err(DomainError::NotFound);
    }
    Ok(())
}
