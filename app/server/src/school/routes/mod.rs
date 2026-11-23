//! School screens (I3 phase 2). One module per screen group, each with its own `routes()`, merged here once and into
//! the app by `crate::router`. Phase-2 panes edit only their own module and `templates/school/<group>/`.
//!
//! Every route sits behind the session and CSRF layers (AP-01, AP-02). Handlers take `RequireStaff` (AP-03) or
//! `RequireAdmin` (AP-08) and call the domain layer (`crate::school::*`) with `Who::of(&user)`. Field PATCH handlers
//! call `school::registries::patch`. Paths with `{kind}` use `Kind::from_slug` (unknown kind -> 404).

pub mod grades_certs;
pub mod offerings_enrollments;
pub mod students_affiliates;

use crate::AppState;
use axum::Router;

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(students_affiliates::routes())
        .merge(offerings_enrollments::routes())
        .merge(grades_certs::routes())
}
