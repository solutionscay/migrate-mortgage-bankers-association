//! `mba-server`: opens (and migrates) the database, then serves the app.
//!
//! Environment: `MBA_BIND` (default `127.0.0.1:8080`), `MBA_DB` (default `data/mba.db`), `MBA_DATA_DIR`,
//! `MBA_SECURE_COOKIE=1` behind HTTPS, `RESEND_API_KEY` (I4; unset = outbox mock). `RUST_LOG` sets the log level.

use mba_server::{config::Config, db, router, sys, AppState};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,mba_server=info".into()),
        )
        .init();
    if let Err(e) = run().await {
        eprintln!("mba-server: {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env()?;
    let conn = db::open(&config.db_path)?;
    let users: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
    if users == 0 {
        tracing::warn!("no users yet: run `mba-seed` to create the demo accounts");
    }
    let bind = config.bind;
    tracing::info!(db = %config.db_path.display(), mail = if config.resend_api_key.is_some() { "resend" } else { "mock" }, "starting");
    let state = AppState::new(conn, config);
    sys::spawn_jobs(state.clone());
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!("listening on http://{}", listener.local_addr()?);
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
