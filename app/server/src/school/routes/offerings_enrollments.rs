//! Phase-2 pane "offerings_enrollments": UI-18 Ofrecimientos (`/offerings/{kind}` GET/POST, `/offerings/{kind}/{id}`
//! GET/PATCH/DELETE, `POST /offerings/{kind}/{id}/calendar` "Abrir curso", `POST /calendar/{kind}/{id}/mail` is I4),
//! UI-19 Fecha del curso (`GET /calendar/{kind}/{id}` dialog, `PATCH /calendar/{kind}/{id}`,
//! `POST /calendar/{kind}/{id}/dates`, `DELETE /calendar/{kind}/{id}/dates/{date_id}`), UI-20 Matrícula
//! (`GET /students/{id}/enrollments/{kind}/new`, `GET /calendar-options/{kind}`, `POST /students/{id}/enrollments/{kind}`,
//! `/enrollments/{kind}/{id}` GET/POST/PATCH/DELETE, `POST /enrollments/{kind}/{id}/cancel`), UI-21 Matrículas
//! (`GET /enrollments/{kind}`, `GET /calendar/{kind}/{id}/enrollments`, `GET /exports/grades.xlsx`).
//!
//! AP-07 / T-05-07: register `GET` and `PATCH` on `/calendar/{kind}/{id}` and nothing else, so `DELETE` answers 405.
//! Domain: `school::offerings`, `school::enrollments`, `school::grades::export_xlsx`,
//! `school::registries::{patch, Resource::{Offering, Calendar, DateCourse, DateShort, Enrollment}}`.
//! Templates: `templates/school/offerings_enrollments/`.
//! Stub: no route yet (the protected fallback answers 404 inside the shell).

use crate::AppState;
use axum::Router;

pub fn routes() -> Router<AppState> {
    Router::new()
}
