//! Convention screens (I2 phase 2). One module per screen group, each with its own `routes()`, merged here once and
//! into the app by `crate::router`. Phase-2 panes edit only their own module and `templates/convention/<group>/`.
//!
//! Every route here sits behind the session and CSRF layers (AP-01, AP-02). Handlers take `RequireStaff` (AP-03)
//! and call the domain layer (`crate::convention::*`) with `Who::of(&user)`; the domain checks AP-04..AP-06/AP-09
//! itself. Field PATCH handlers call `convention::registries::patch`.

pub mod events;
pub mod people_plans;
pub mod registrations;

use crate::AppState;
use axum::Router;

pub fn routes() -> Router<AppState> {
    Router::new().merge(registrations::routes()).merge(events::routes()).merge(people_plans::routes())
}
