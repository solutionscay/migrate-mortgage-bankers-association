//! `mba-seed` (OP-SYS-03): creates a new database file with synthetic demo data. Server CLI only; no web route
//! reaches it (AP-14).
//!
//!     mba-seed [--db data/mba.db] [--seed 2026] [--force]
//!
//! The same `--seed` gives the same data. `--force` replaces an existing file.

use mba_server::{db, seed};
use std::path::PathBuf;

fn main() {
    if let Err(e) = run() {
        eprintln!("mba-seed: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut path = PathBuf::from(std::env::var("MBA_DB").unwrap_or_else(|_| "data/mba.db".into()));
    let mut seed_no = seed::DEFAULT_SEED;
    let mut force = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--db" => path = PathBuf::from(args.next().ok_or("--db needs a path")?),
            "--seed" => seed_no = args.next().ok_or("--seed needs a number")?.parse()?,
            "--force" => force = true,
            "-h" | "--help" => {
                println!("mba-seed [--db data/mba.db] [--seed {}] [--force]", seed::DEFAULT_SEED);
                return Ok(());
            }
            other => return Err(format!("unknown argument `{other}`").into()),
        }
    }
    if path.exists() {
        if !force {
            return Err(format!("{} exists; pass --force to replace it", path.display()).into());
        }
        for suffix in ["", "-wal", "-shm"] {
            let p = PathBuf::from(format!("{}{suffix}", path.display()));
            if p.exists() {
                std::fs::remove_file(&p)?;
            }
        }
    }
    let mut conn = db::open(&path)?;
    // Seeded files (event PDF, billing PDFs) go where the server looks: MBA_DATA_DIR, else the database's folder.
    let data_dir = std::env::var("MBA_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| path.parent().map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")));
    let summary = seed::run_in(&mut conn, seed_no, Some(&data_dir))?;
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;

    println!("Seeded {} (seed {seed_no}). integrity_check: ok. foreign_key_check: ok.", path.display());
    for (table, n) in &summary.rows {
        println!("  {table:<26} {n:>5}");
    }
    println!();
    println!("Demo accounts (synthetic data; password for all: {}):", seed::DEMO_PASSWORD);
    for (user, name, role) in seed::ACCOUNTS {
        let label = if *role == "admin" { "ROLE-ADMIN" } else { "ROLE-STAFF" };
        println!("  {user:<12} {label:<11} {name}");
    }
    Ok(())
}
