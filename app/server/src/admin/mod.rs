//! ADMIN area: UI-29 users (`/admin/users`, OP-AUTH-04) and UI-30 settings (`/admin/settings`,
//! `/admin/templates/{code}`, `/admin/value-lists`; OP-SYS-01, OP-SYS-02). 05 "Admin (ROLE-ADMIN)".
//!
//! Mounted at `/admin` by `crate::router` behind `auth::require_admin` (AP-08): paths here are written without the
//! `/admin` prefix. ROLE-STAFF gets 403 on every path, built or not (T-05-05, T-08-12).

pub mod settings;
pub mod users;

use crate::AppState;
use axum::Router;

pub fn routes() -> Router<AppState> {
    Router::new().merge(users::routes()).merge(settings::routes())
}
