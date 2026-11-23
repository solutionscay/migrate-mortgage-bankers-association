//! SYS area (phase 2): OP-SYS-01 settings writes, OP-SYS-02 document templates (sanitized HTML, `ammonia`),
//! OP-SYS-04 files (`POST /files`, `GET /files/{uuid}`; `stored_files` + `data/files/`, `mime_guess`), and JOB-02
//! housekeeping (every 15 minutes: `auth::session::purge_expired`, plus grade-import tokens older than 30 minutes).
//! OP-SYS-03 is the `mba-seed` binary (`crate::seed`), not a route (AP-14).

use crate::AppState;
use axum::Router;

pub fn routes() -> Router<AppState> {
    Router::new()
}

/// Background jobs, started once by `main` (JOB-02 goes here).
pub fn spawn_jobs(_state: AppState) {}
