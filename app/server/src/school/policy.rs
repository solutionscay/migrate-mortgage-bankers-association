//! School policies. AP-07 is owned here; AP-08 guards configuration (certifications, requirements, professors).
//! AP-03..AP-06 and AP-09 are the shared functions of `convention::policy` (retested by the I3 tests):
//! * AP-03 domain create/view/edit: `ap03_domain`.
//! * AP-04 parent delete (student, affiliate, course, seminar, short course, certification): `ap04_parent_delete`
//!   for the role; the dependents check gives `HasEnrollments` / `HasCalendar` / `InUse` (409).
//! * AP-05 / AP-06 enrollment delete: [`ap05_enrollment_delete`] (a payment gives 409; unpaid: both roles).
//! * AP-06 child rows without money (class dates, interests, requirements): `ap06_child_remove`.
//! * AP-09 bulk (grades import): `ap09_bulk`; the scope is the matched serials of the file.

pub use crate::convention::policy::{ap03_domain, ap06_child_remove, ap09_bulk};

use super::{DResult, DomainError, Who};
use crate::auth::Role;

/// AP-07 / BR-CRS-05: nobody deletes a calendar row (course, seminar or short course); the status goes to
/// Cancelado instead. There is no delete route (T-05-07, T-08-11); this is the domain answer if one is ever asked.
pub fn ap07_calendar_delete(_who: &Who) -> DResult<()> {
    Err(DomainError::NoDelete)
}

/// AP-08: configuration is ROLE-ADMIN only (certifications, requirements, professors; D-19).
pub fn ap08_config(who: &Who) -> DResult<()> {
    if who.role == Role::Admin {
        Ok(())
    } else {
        Err(DomainError::Forbidden)
    }
}

/// AP-04: parent delete is ROLE-ADMIN only (the dependents check follows in each command).
pub fn ap04_admin(who: &Who) -> DResult<()> {
    ap08_config(who)
}

/// AP-05 / AP-06 / BR-ENR-08: an enrollment with a payment is never deleted (409, any role: Dar de baja instead).
/// Without a payment it is a child row with no money, so ROLE-STAFF and ROLE-ADMIN may remove it after the confirm
/// (AP-06, 08; operator decision).
pub fn ap05_enrollment_delete(who: &Who, pago: i64) -> DResult<()> {
    if pago > 0 {
        return Err(DomainError::EnrollmentPaid);
    }
    ap06_child_remove(who)
}
