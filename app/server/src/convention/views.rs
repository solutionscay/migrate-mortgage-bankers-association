//! Shared convention view pieces for the phase-2 screens.
//!
//! * [`TotalsBlock`]: the inner HTML of `<dl class="totals" id="totales">` of UI-06 (design/registro.html), from
//!   `v_registration_totals`. Use it in the page and, after any line write, out of band with [`totales_oob`].
//! * [`usd`]: "$1,218.00" from cents, for templates.

use super::registrations::{self, Totals};
use super::DResult;
use askama::Template;
use rusqlite::Connection;

/// "$1,218.00" from cents.
pub fn usd(cents: &i64) -> String {
    crate::seed::usd(*cents)
}

#[derive(Template)]
#[template(path = "convention/totales.html")]
pub struct TotalsBlock {
    pub t: Totals,
}

impl TotalsBlock {
    /// "$1,218.00" (template helper).
    pub fn usd(&self, cents: &i64) -> String {
        usd(cents)
    }

    pub fn load(conn: &Connection, registration_uuid: &str) -> DResult<Self> {
        Ok(Self { t: registrations::totals(conn, registration_uuid)? })
    }
}

/// The `#totales` out-of-band fragment for a line-field response (05 per-field PATCH "Success").
pub fn totales_oob(conn: &Connection, registration_uuid: &str) -> DResult<String> {
    let inner =
        TotalsBlock::load(conn, registration_uuid)?.render().map_err(|e| super::DomainError::Db(e.to_string()))?;
    Ok(crate::autosave::totales(&inner))
}
