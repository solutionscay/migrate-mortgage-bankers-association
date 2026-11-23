//! SYS area: OP-SYS-04 files (`files`: `POST /files`, `GET /files/{uuid}`, the file-chip fragment) and JOB-02
//! housekeeping (`jobs`). OP-SYS-01/02 domain functions for the admin pages go here too.
//! OP-SYS-03 is the `mba-seed` binary (`crate::seed`), not a route (AP-14).

pub mod files;
pub mod jobs;

use crate::AppState;
use axum::Router;

pub fn routes() -> Router<AppState> {
    Router::new().merge(files::routes())
}

/// Background jobs, started once by `main`: JOB-02 every 15 minutes.
pub fn spawn_jobs(state: AppState) {
    jobs::spawn(state);
}
