//! UI-02 home: module tiles with live counts (query `home`, BR-SYS-01) and recent activity from `audit_log`.

use crate::auth::{CurrentUser, Shell};
use crate::clock;
use crate::error::AppError;
use crate::web::{render_page, Page};
use crate::AppState;
use askama::Template;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Html;
use axum::routing::get;
use axum::Router;
use rusqlite::Connection;

pub fn routes() -> Router<AppState> {
    Router::new().route("/", get(home))
}

/// Tile figures. Admin figures are read only for ROLE-ADMIN.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Counts {
    pub active_events: i64,
    pub registrations_active_events: i64,
    pub active_students: i64,
    pub open_courses: i64,
    pub draft_batches: i64,
    pub batches_with_errors: i64,
    pub active_users: i64,
    pub locked_users: i64,
}

pub fn counts(conn: &Connection, admin: bool) -> rusqlite::Result<Counts> {
    let one = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0));
    let mut c = Counts {
        active_events: one("SELECT COUNT(*) FROM events WHERE status = 'Activo'")?,
        registrations_active_events: one(
            "SELECT COUNT(*) FROM registration r JOIN events e ON e.uuid = r.event_uuid WHERE e.status = 'Activo'",
        )?,
        active_students: one("SELECT COUNT(*) FROM v_student_academic WHERE is_active = 1")?,
        open_courses: one("SELECT COUNT(*) FROM calendar_course WHERE status IN ('Pendiente', 'Confirmado')")?,
        draft_batches: one("SELECT COUNT(*) FROM v_mail_batch_counts WHERE count_sent = 0")?,
        batches_with_errors: one("SELECT COUNT(*) FROM v_mail_batch_counts WHERE count_errors > 0")?,
        ..Default::default()
    };
    if admin {
        c.active_users = one("SELECT COUNT(*) FROM users WHERE active = 1")?;
        c.locked_users = conn.query_row(
            "SELECT COUNT(*) FROM users WHERE locked_until IS NOT NULL AND locked_until > ?1",
            [clock::now_text()],
            |r| r.get(0),
        )?;
    }
    Ok(c)
}

pub struct Activity {
    pub time: String,
    pub module: &'static str,
    pub text: String,
}

/// The last domain changes (auth events are left out). The text is the `detail.text` the writer stored.
fn recent(conn: &Connection) -> rusqlite::Result<Vec<Activity>> {
    let mut stmt = conn.prepare(
        "SELECT at, entity, detail FROM audit_log WHERE action NOT LIKE 'auth.%' ORDER BY at DESC, id DESC LIMIT 6",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (at, entity, detail) = row?;
        let text = detail
            .and_then(|d| serde_json::from_str::<serde_json::Value>(&d).ok())
            .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(str::to_owned))
            .unwrap_or_else(|| entity.clone().unwrap_or_default());
        let time = clock::parse(&at).map(clock::spanish_time).unwrap_or(at);
        out.push(Activity { time, module: module_of(entity.as_deref()), text });
    }
    Ok(out)
}

fn module_of(entity: Option<&str>) -> &'static str {
    match entity {
        Some(e) if e.starts_with("mail_") => "Correos",
        Some("people" | "registration" | "registration_items" | "payments" | "events" | "packages" | "sponsor") => {
            "Convención"
        }
        Some("users" | "app_settings" | "document_templates" | "value_list_items") => "Admin",
        Some(_) => "Escuela",
        None => "Inicio",
    }
}

#[derive(Template)]
#[template(path = "home.html", blocks = ["content"])]
struct HomeTemplate {
    shell: Shell,
    today: String,
    c: Counts,
    activity: Vec<Activity>,
}

impl Page for HomeTemplate {
    fn render_fragment(&self) -> askama::Result<String> {
        self.as_content().render()
    }
}

async fn home(State(state): State<AppState>, user: CurrentUser, headers: HeaderMap) -> Result<Html<String>, AppError> {
    let shell = Shell::new(&user, "inicio", "inicio");
    let (c, activity) = {
        let conn = state.db();
        (counts(&conn, shell.is_admin)?, recent(&conn)?)
    };
    let page = HomeTemplate { shell, today: clock::spanish_long_date(clock::now()), c, activity };
    render_page(&headers, &page)
}
