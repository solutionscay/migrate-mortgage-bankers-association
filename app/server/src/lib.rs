//! MBA demo web server (I1 Foundation).
//!
//! One axum process, one SQLite file (D-01). Each route checks the session and the policy, calls one operation,
//! and returns HTML: a full page for a normal request, the main fragment for an htmx request (05 "Route
//! conventions").
//!
//! Router layout: one module per area, each with its own `routes()`. [`router`] is the only place that merges them.
//! Phase 2 panes add routes inside their own module and never edit this file.

pub mod admin;
pub mod audit;
pub mod auth;
pub mod autosave;
pub mod clock;
pub mod config;
pub mod convention;
pub mod db;
pub mod error;
pub mod home;
pub mod school;
pub mod seed;
pub mod sys;
pub mod web;

use axum::middleware::{from_fn, from_fn_with_state};
use axum::Router;
use rusqlite::Connection;
use std::sync::{Arc, Mutex, MutexGuard};

/// Shared state. One SQLite connection behind a mutex: WAL, one writer, short statements (05 "Errors,
/// concurrency, and observability"). Never hold the guard across an `.await`.
#[derive(Clone)]
pub struct AppState {
    db: Arc<Mutex<Connection>>,
    pub config: Arc<config::Config>,
}

impl AppState {
    pub fn new(conn: Connection, config: config::Config) -> Self {
        Self { db: Arc::new(Mutex::new(conn)), config: Arc::new(config) }
    }

    /// The database connection. A poisoned lock still yields the connection: SQLite rolls back an unfinished
    /// transaction on its own, so the data stays consistent.
    pub fn db(&self) -> MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The whole app. Default deny: every route outside `public` sits behind the session check (AP-01), the CSRF check
/// (AP-02) and the error pages. `/admin/*` adds `require_admin` (AP-08), also for paths phase 2 has not built yet.
pub fn router(state: AppState) -> Router {
    let public = Router::new().merge(auth::public_routes()).merge(web::asset_routes());

    let admin = admin::routes().fallback(error::not_found).layer(from_fn(auth::require_admin));

    let protected = Router::new()
        .merge(home::routes())
        .merge(auth::account_routes())
        .merge(autosave::routes())
        .merge(sys::routes())
        .merge(convention::routes::routes())
        .merge(school::routes::routes())
        .nest("/admin", admin)
        .fallback(error::not_found)
        .layer(from_fn_with_state(state.clone(), auth::csrf_guard))
        .layer(from_fn_with_state(state.clone(), error::error_pages))
        .layer(from_fn_with_state(state.clone(), auth::require_session));

    let inner = public.merge(protected).with_state(state);
    // `_method` must be rewritten before routing, so this layer wraps the router as a service (a `Router::layer`
    // runs after the route is chosen).
    let inner = tower_layer::Layer::layer(&from_fn(web::method_override), inner);

    Router::new()
        .fallback_service(inner)
        .layer(from_fn(web::security_headers))
        .layer(from_fn(audit::request_log))
        .layer(axum::extract::DefaultBodyLimit::max(web::MAX_BODY))
}
