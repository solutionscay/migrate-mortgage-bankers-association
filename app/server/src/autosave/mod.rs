//! Per-field autosave (phase 2): the `PATCH` contract of 05 "Per-field autosave (PATCH)".
//!
//! To build here: a single-field form extractor (exactly one field, else 422; T-05-15), the allow-list check per
//! command, the 200 field fragment and the 422 error fragment (`aria-invalid`, `data-unsaved`), one
//! `BEGIN IMMEDIATE` update per field with `audit::stamp_modified` and one `audit_log` `field.change` row
//! (T-05-13). CSRF is already enforced for every PATCH by `auth::csrf_guard` (header `X-CSRF-Token`, sent by htmx
//! from `hx-headers` on `<body>`). Domain modules (I2+) call these helpers from their own `PATCH` handlers.

use crate::AppState;
use axum::Router;

pub fn routes() -> Router<AppState> {
    Router::new()
}
