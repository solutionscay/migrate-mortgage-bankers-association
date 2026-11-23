//! Phase-2 pane "students_affiliates": UI-14 Estudiantes (`GET /students`: Quick Find OP-STU-05, 0 / 1 / many),
//! UI-15 Estudiante (`/students/new`, `POST /students`, `/students/{id}` GET/PATCH/DELETE with
//! `?tab=certifications|courses|seminars|short-courses|details`, `POST /students/{id}/duplicate`,
//! `POST /students/{id}/interests`, `PATCH /student-certifications/{id}`, `GET /affiliates/options`), UI-16
//! Afiliados (`GET /affiliates`), UI-17 Afiliado (`/affiliates/new`, `POST /affiliates`, `/affiliates/{id}`
//! GET/PATCH/DELETE), EXT-02 "Ver en el mapa" (`students::maps_url`, `affiliates::maps_url`).
//!
//! Domain: `school::students`, `school::affiliates`, `school::enrollments::course_lines`,
//! `school::registries::{patch, Resource::{Student, StudentCert, Affiliate}}`. The enrollment dialog itself (UI-20)
//! belongs to the offerings_enrollments pane. Templates: `templates/school/students_affiliates/`.
//! Stub: no route yet (the protected fallback answers 404 inside the shell).

use crate::AppState;
use axum::Router;

pub fn routes() -> Router<AppState> {
    Router::new()
}
