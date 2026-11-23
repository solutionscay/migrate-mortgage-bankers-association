//! Record policies of 08 that I2 owns: AP-03, AP-04, AP-05, AP-06, AP-09. Each returns `Err(Forbidden)` or a domain
//! refusal; commands call them before they write. Hiding a button is never the check (08).
//!
//! Order of checks (so the tests of 08 read as written):
//! * AP-04 parent delete: role first (staff -> 403, T-08-06), then dependents (admin -> 409 with the count, T-08-07).
//! * AP-05 registration delete: payments first (any role -> 409 with the cancel offer, T-08-09, T-05-04), then role
//!   (staff -> 403: deleting a row that carries money is ROLE-ADMIN only).
//! * AP-05 payment void: role (staff -> 403), then the reason (admin without reason -> 422, T-08-08). No delete of a
//!   payment exists for any role.

use super::{DResult, DomainError, Who};
use crate::auth::Role;
use rusqlite::Connection;

/// AP-03: create, view, edit domain records: ROLE-ADMIN or ROLE-STAFF.
pub fn ap03_domain(who: &Who) -> DResult<()> {
    match who.role {
        Role::Admin | Role::Staff => Ok(()),
    }
}

/// AP-04: hard delete of a parent record (person, event, plan): ROLE-ADMIN, and no dependent rows.
pub fn ap04_parent_delete(who: &Who, dependents: i64) -> DResult<()> {
    if who.role != Role::Admin {
        return Err(DomainError::Forbidden);
    }
    if dependents > 0 {
        return Err(DomainError::HasRegistrations(dependents));
    }
    Ok(())
}

/// AP-05: delete a registration only when it has no payment row; else cancel (status Canceló). ROLE-ADMIN only.
pub fn ap05_registration_delete(who: &Who, payments: i64) -> DResult<()> {
    if payments > 0 {
        return Err(DomainError::HasPayments(payments));
    }
    if who.role != Role::Admin {
        return Err(DomainError::Forbidden);
    }
    Ok(())
}

/// AP-05: void a payment: ROLE-ADMIN with a reason.
pub fn ap05_payment_void(who: &Who, reason: &str) -> DResult<()> {
    if who.role != Role::Admin {
        return Err(DomainError::Forbidden);
    }
    if reason.trim().is_empty() {
        return Err(DomainError::Validation("void_reason", "Escriba la razón de la anulación.".into()));
    }
    Ok(())
}

/// AP-05: cancel a registration (status Canceló): both roles (it is the path 08 offers when delete is refused).
pub fn ap05_registration_cancel(who: &Who) -> DResult<()> {
    ap03_domain(who)
}

/// AP-06: remove a child row without money (registration item, guest, plan item, event item, sponsor, billing file):
/// both roles; the UI confirms first; a foreign key refusal becomes 409 (`InUse`).
pub fn ap06_child_remove(who: &Who) -> DResult<()> {
    ap03_domain(who)
}

/// AP-06 for mail rows (owned here, used by I4): an e-mail or batch is editable only while no e-mail of the batch
/// is sent. Refusal: 409.
pub fn ap06_batch_editable(conn: &Connection, batch_uuid: &str) -> DResult<()> {
    let sent: i64 =
        conn.query_row("SELECT COUNT(*) FROM mail_emails WHERE batch_uuid = ?1 AND sent = 1", [batch_uuid], |r| {
            r.get(0)
        })?;
    if sent > 0 {
        Err(DomainError::InUse(sent))
    } else {
        Ok(())
    }
}

/// AP-09: bulk operations (plan rebuild, send all, grades import): both roles. The scope is fixed by the server
/// (the registration uuid, the batch uuid, the matched serials); no command takes a client list of row ids.
pub fn ap09_bulk(who: &Who) -> DResult<()> {
    ap03_domain(who)
}
