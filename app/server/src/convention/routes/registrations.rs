//! Phase-2 pane "registrations": UI-05 Registros (`GET /registrations`), UI-06 Registro (`/registrations/{id}` and
//! every `/registrations/{id}/...` route of 05: event and plan pickers, items, attributes, guests, payments,
//! cancel, delete; `POST /payments/{id}/void`), UI-07 Registro en puerta (`GET /events/{id}/door`,
//! `POST /registrations/{id}/attendance`).
//!
//! Domain: `convention::registrations`, `convention::registries::{patch, Resource::{Registration, Item, Guest}}`,
//! `convention::views::{TotalsBlock, totales_oob}`. Templates: `templates/convention/registrations/`.
//! Stub: no route yet (the protected fallback answers 404 inside the shell).

use crate::AppState;
use axum::Router;

pub fn routes() -> Router<AppState> {
    Router::new()
}
