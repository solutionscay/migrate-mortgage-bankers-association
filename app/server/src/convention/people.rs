//! PPL: convention attendees (OP-PPL-01, OP-PPL-02; BR-PPL-01, BR-PPL-02).

use super::{count, new_uuid, optional, policy, DResult, Who};
use crate::clock;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

/// The create form of UI-04 ("Nuevo convencionista"). Every other field is edited by autosave
/// (`registries::PERSON`).
#[derive(Debug, Default, Clone)]
pub struct NewPerson {
    pub name_first: String,
    pub name_last: String,
    pub company: String,
}

/// OP-PPL-01: create a person (AP-03). At least a first or last name.
pub fn create_person(tx: &Transaction, who: &Who, p: &NewPerson) -> DResult<String> {
    policy::ap03_domain(who)?;
    let first = optional("name_first", &p.name_first, 100)?;
    let last = optional("name_last", &p.name_last, 100)?;
    if first.is_none() && last.is_none() {
        return Err(super::DomainError::Validation("name_first", "Escriba el nombre o el apellido.".into()));
    }
    let company = optional("company", &p.company, 200)?;
    let uuid = new_uuid();
    let now = clock::now_text();
    let badge = [first.as_deref(), last.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(" ");
    tx.execute(
        "INSERT INTO people (uuid, name_first, name_last, company, name_badge, created_at, created_by, modified_at, modified_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?6, ?7)",
        params![uuid, first, last, company, badge, now, who.actor.username],
    )?;
    Ok(uuid)
}

/// Registrations of a person (BR-PPL-02 count).
pub fn registration_count(conn: &Connection, person_uuid: &str) -> rusqlite::Result<i64> {
    count(conn, "SELECT COUNT(*) FROM registration WHERE person_uuid = ?1", person_uuid)
}

/// OP-PPL-02: delete a person. ROLE-ADMIN (AP-04); refused while the person has registrations (BR-PPL-02).
pub fn delete_person(tx: &Transaction, who: &Who, person_uuid: &str) -> DResult<()> {
    exists(tx, person_uuid)?;
    policy::ap04_parent_delete(who, registration_count(tx, person_uuid)?)?;
    tx.execute("DELETE FROM people WHERE uuid = ?1", [person_uuid])?;
    crate::audit::log(tx, who.actor.user_id, "person.delete", Some("people"), Some(person_uuid), None)?;
    Ok(())
}

fn exists(conn: &Connection, uuid: &str) -> DResult<()> {
    conn.query_row("SELECT 1 FROM people WHERE uuid = ?1", [uuid], |_| Ok(()))
        .optional()?
        .ok_or(super::DomainError::NotFound)
}

/// BR-PPL-01: full name = first + " " + last.
pub fn full_name(first: Option<&str>, last: Option<&str>) -> String {
    [first, last].into_iter().flatten().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
}

/// BR-PPL-01: address block = address, then "city state zip".
pub fn address_block(address: Option<&str>, city: Option<&str>, state: Option<&str>, zip: Option<&str>) -> String {
    let line2 = [city, state, zip].into_iter().flatten().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
    [address.unwrap_or_default(), line2.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n")
}
