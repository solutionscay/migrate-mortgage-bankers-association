//! Phase-2 pane "events": UI-08 Convenciones (`GET /events`, `/events/new`, `POST /events`), UI-09 Convención
//! (`/events/{id}` with `?tab=items|sponsors|billing`, PATCH, DELETE, PDF, `POST /events/{id}/items`,
//! `POST /events/{id}/sponsors`, `POST /events/{id}/billing`, `DELETE /billing/{id}`), UI-10 Detalle de actividad
//! (`/event-items/{id}`), UI-11 Auspiciador (`/sponsors/{id}`).
//!
//! Domain: `convention::events`, `convention::sponsors`, `convention::registries::{patch, Resource::{Event,
//! EventItem, Sponsor}}`, files through `sys::files::{store, FileField}`. Templates: `templates/convention/events/`.
//! Note: `GET /events/{id}/door` (UI-07) belongs to the registrations pane.
//! Stub: no route yet (the protected fallback answers 404 inside the shell).

use crate::AppState;
use axum::Router;

pub fn routes() -> Router<AppState> {
    Router::new()
}
