//! Phase-2 pane "people_plans": UI-03 Convencionistas (`GET /people`), UI-04 Convencionista (`/people/new`,
//! `POST /people`, `/people/{id}` GET/PATCH/DELETE, `POST /people/{id}/registrations`), UI-12 Planes
//! (`GET /plans`, `POST /plans`), UI-13 Plan (`/plans/{id}` GET/PATCH/DELETE, `POST /plans/{id}/build`,
//! `PATCH /plan-items/{id}`, `DELETE /plan-items/{id}`).
//!
//! Domain: `convention::people`, `convention::plans`, `convention::registrations::create_registration` (the
//! "Añadir Registro" button), `convention::registries::{patch, Resource::{Person, Plan, PlanItem}}`.
//! Templates: `templates/convention/people_plans/`.
//! Stub: no route yet (the protected fallback answers 404 inside the shell).

use crate::AppState;
use axum::Router;

pub fn routes() -> Router<AppState> {
    Router::new()
}
