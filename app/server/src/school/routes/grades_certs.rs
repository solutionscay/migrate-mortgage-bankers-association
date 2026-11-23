//! Phase-2 pane "grades_certs": UI-22 Importar notas (`GET /grades/import`, `POST /grades/import` upload + preview,
//! `POST /grades/import/{token}/apply`; T-05-12: another user's token is 404), UI-32 admin certifications and
//! professors (`/admin/certifications` GET/POST, `/admin/certifications/{id}` GET/PATCH,
//! `POST /admin/certifications/{id}/requirements`, `DELETE /admin/certifications/{id}/requirements/{req_id}`,
//! `/admin/professors` GET/POST, `PATCH /admin/professors/{id}`; inventory screen per D-19, ROLE-ADMIN).
//!
//! The `/admin/*` paths are served here, not in `crate::admin`: write them with the `/admin` prefix; the handlers take
//! `RequireAdmin` (AP-08) because this router is not under the `/admin` nest.
//! Domain: `school::grades::{read_xlsx, preview, apply}`, `school::certifications`,
//! `school::registries::{patch, Resource::{Certification, Requirement, Professor}}`.
//! Templates: `templates/school/grades_certs/`.
//! Stub: no route yet (the protected fallback answers 404 inside the shell).

use crate::AppState;
use axum::Router;

pub fn routes() -> Router<AppState> {
    Router::new()
}
