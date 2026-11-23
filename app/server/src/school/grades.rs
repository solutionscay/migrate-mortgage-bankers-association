//! Grades XLSX round trip (OP-ENR-06 export, OP-ENR-07 import; BR-ENR-06, BR-ENR-07; D-23).
//!
//! * Export: the course enrollments of a calendar row (or all), one row each: Serial, Estudiante, the grade columns.
//!   XLSX written with `rust_xlsxwriter`.
//! * Import, step 1 [`preview`]: read the XLSX (`calamine`), match each row on the enrollment `serial` (not
//!   `id_num`), compute the changes. **Nothing is written.** The preview is kept in memory under a random token,
//!   for this user, 30 minutes (05: no table in 04 for it; JOB-02 calls [`purge_previews`]).
//! * Import, step 2 [`apply`]: only the token's owner, within 30 minutes; every matched row in one transaction;
//!   `puntuacion` takes the imported final average, so `nota` and `is_passed` follow in the view (BR-ENR-03..05).
//!   Unmatched rows are listed and never create enrollments. AP-09: the scope is the matched serials of the file.
//!
//! An empty cell leaves the stored value as it is (it does not clear it).

use super::{policy, DResult, DomainError, Who};
use chrono::NaiveDateTime;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// The grade columns of the file, in order: (header, `enrollments_course` column).
pub const COLUMNS: &[(&str, &str)] = &[
    ("Examen", "examen"),
    ("1ra", "primera"),
    ("2da", "segunda"),
    ("3ra", "tercera"),
    ("4ta", "cuarta"),
    ("5ta", "quinta"),
    ("6ta", "sexta"),
    ("7ma", "septima"),
    ("8va", "octava"),
    ("Prom. asignaciones", "promedio_asignaciones"),
    ("Asistencia y puntualidad", "asistencia_y_puntualidad"),
    ("Promedio final", "promedio_final"),
];

/// Preview tokens live this long (05).
pub const PREVIEW_MINUTES: i64 = crate::sys::jobs::PREVIEW_TOKEN_MINUTES;
/// Largest file the import reads (05: 5,000 rows).
pub const MAX_ROWS: usize = 5_000;

// ---------------------------------------------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------------------------------------------

/// OP-ENR-06 / BR-ENR-07: the XLSX of the course enrollments of `calendar` (all calendars when `None`), by name.
pub fn export_xlsx(conn: &Connection, calendar: Option<&str>) -> DResult<Vec<u8>> {
    use rust_xlsxwriter::{Format, Workbook};
    let cols = COLUMNS.iter().map(|(_, c)| format!("e.{c}")).collect::<Vec<_>>().join(", ");
    let mut stmt = conn.prepare(&format!(
        "SELECT e.serial, s.nombre, {cols} FROM enrollments_course e JOIN students s ON s.uuid = e.student_uuid
          WHERE (?1 IS NULL OR e.course_calendar_uuid = ?1) ORDER BY s.nombre COLLATE NOCASE, e.serial"
    ))?;
    let rows: Vec<(Option<i64>, String, Vec<Option<f64>>)> = stmt
        .query_map([calendar], |r| {
            let grades = (0..COLUMNS.len()).map(|i| r.get::<_, Option<f64>>(2 + i)).collect::<Result<Vec<_>, _>>()?;
            Ok((r.get(0)?, r.get(1)?, grades))
        })?
        .collect::<Result<_, _>>()?;
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    let bold = Format::new().set_bold();
    let xl = |e: rust_xlsxwriter::XlsxError| DomainError::Db(format!("xlsx: {e}"));
    ws.write_string_with_format(0, 0, "Serial", &bold).map_err(xl)?;
    ws.write_string_with_format(0, 1, "Estudiante", &bold).map_err(xl)?;
    for (i, (h, _)) in COLUMNS.iter().enumerate() {
        ws.write_string_with_format(0, 2 + i as u16, *h, &bold).map_err(xl)?;
    }
    for (r, (serial, name, grades)) in rows.iter().enumerate() {
        let row = r as u32 + 1;
        if let Some(s) = serial {
            ws.write_number(row, 0, *s as f64).map_err(xl)?;
        }
        ws.write_string(row, 1, name).map_err(xl)?;
        for (i, g) in grades.iter().enumerate() {
            if let Some(v) = g {
                ws.write_number(row, 2 + i as u16, *v).map_err(xl)?;
            }
        }
    }
    wb.save_to_buffer().map_err(xl)
}

// ---------------------------------------------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------------------------------------------

/// One data row of the file: its row number (1 = header), the serial cell as text, the grade values.
#[derive(Debug, Clone, PartialEq)]
pub struct FileRow {
    pub row: usize,
    pub serial: String,
    /// (`enrollments_course` column, value) for the non-empty grade cells.
    pub values: Vec<(&'static str, f64)>,
}

/// Reads the XLSX: the first sheet, a header row with "Serial" and any of the [`COLUMNS`] headers
/// (case-insensitive). Errors are Spanish messages for the upload field.
pub fn read_xlsx(bytes: &[u8]) -> Result<Vec<FileRow>, String> {
    use calamine::{Data, Reader, Xlsx};
    let mut wb: Xlsx<_> =
        Xlsx::new(std::io::Cursor::new(bytes.to_vec())).map_err(|_| "El archivo no es un XLSX válido.".to_string())?;
    let range = wb
        .worksheet_range_at(0)
        .ok_or_else(|| "El archivo no tiene hojas.".to_string())?
        .map_err(|_| "No se pudo leer la hoja.".to_string())?;
    let mut rows = range.rows();
    let header: Vec<String> = rows
        .next()
        .ok_or_else(|| "El archivo está vacío.".to_string())?
        .iter()
        .map(|c| c.to_string().trim().to_lowercase())
        .collect();
    let serial_ix =
        header.iter().position(|h| h == "serial").ok_or_else(|| "Falta la columna «Serial».".to_string())?;
    let mut map: Vec<(usize, &'static str)> = Vec::new();
    for (h, col) in COLUMNS {
        if let Some(ix) = header.iter().position(|x| x == &h.to_lowercase()) {
            map.push((ix, col));
        }
    }
    if map.is_empty() {
        return Err("El archivo no tiene columnas de notas.".into());
    }
    let mut out = Vec::new();
    for (i, r) in rows.enumerate() {
        if out.len() >= MAX_ROWS {
            return Err(format!("El archivo tiene más de {MAX_ROWS} filas."));
        }
        let cell = |ix: usize| r.get(ix).cloned().unwrap_or(Data::Empty);
        let serial = match cell(serial_ix) {
            Data::Float(f) => format!("{}", f as i64),
            Data::Int(n) => n.to_string(),
            Data::Empty => String::new(),
            other => other.to_string().trim().to_owned(),
        };
        let mut values = Vec::new();
        for (ix, col) in &map {
            match cell(*ix) {
                Data::Float(f) => values.push((*col, f)),
                Data::Int(n) => values.push((*col, n as f64)),
                Data::String(s) if !s.trim().is_empty() => match s.trim().replace(',', ".").parse::<f64>() {
                    Ok(f) => values.push((*col, f)),
                    Err(_) => values.push((*col, f64::NAN)),
                },
                _ => {}
            }
        }
        if serial.is_empty() && values.is_empty() && r.iter().all(|c| matches!(c, Data::Empty)) {
            continue; // blank line
        }
        out.push(FileRow { row: i + 2, serial, values });
    }
    Ok(out)
}

/// One matched enrollment in the preview.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub serial: i64,
    pub enrollment_uuid: String,
    pub student: String,
    /// (column, old, new) for the values that change.
    pub fields: Vec<(&'static str, Option<f64>, f64)>,
    /// The score after the import (`puntuacion` = rounded final average when it is in the file), and its letter.
    pub new_score: Option<i64>,
}

/// A row the import does not apply: (file row, serial text, reason).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub row: usize,
    pub serial: String,
    pub reason: &'static str,
}

/// The preview of step 2 (UI-22).
#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    pub token: String,
    pub changes: Vec<Change>,
    /// Matched rows whose values are the same as stored.
    pub unchanged: Vec<(i64, String)>,
    pub skipped: Vec<Skipped>,
}

struct Stored {
    user_id: Option<i64>,
    created: NaiveDateTime,
    changes: Vec<Change>,
    unmatched: usize,
}

static PREVIEWS: LazyLock<Mutex<HashMap<String, Stored>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// OP-ENR-07 step 1: match the rows on `serial` and keep the preview under a token. Writes nothing to the database.
pub fn preview(conn: &Connection, who: &Who, rows: &[FileRow]) -> DResult<Preview> {
    policy::ap09_bulk(who)?;
    let mut changes = Vec::new();
    let mut unchanged = Vec::new();
    let mut skipped = Vec::new();
    for r in rows {
        let Ok(serial) = r.serial.parse::<i64>() else {
            skipped.push(Skipped {
                row: r.row,
                serial: r.serial.clone(),
                reason: if r.serial.is_empty() { "vacío" } else { "serial no válido" },
            });
            continue;
        };
        if r.values.iter().any(|(_, v)| !v.is_finite() || *v < 0.0 || *v > 100.0) {
            skipped.push(Skipped { row: r.row, serial: r.serial.clone(), reason: "nota fuera de 0 a 100" });
            continue;
        }
        let cols = COLUMNS.iter().map(|(_, c)| format!("e.{c}")).collect::<Vec<_>>().join(", ");
        let found = conn
            .query_row(
                &format!("SELECT e.uuid, s.nombre, {cols} FROM enrollments_course e JOIN students s ON s.uuid = e.student_uuid WHERE e.serial = ?1"),
                [serial],
                |x| {
                    let olds = (0..COLUMNS.len()).map(|i| x.get::<_, Option<f64>>(2 + i)).collect::<Result<Vec<_>, _>>()?;
                    Ok((x.get::<_, String>(0)?, x.get::<_, String>(1)?, olds))
                },
            )
            .optional()?;
        let Some((uuid, student, olds)) = found else {
            skipped.push(Skipped { row: r.row, serial: r.serial.clone(), reason: "sin pareja" });
            continue;
        };
        let mut fields = Vec::new();
        for (col, v) in &r.values {
            let ix = COLUMNS.iter().position(|(_, c)| c == col).unwrap_or(0);
            if olds[ix] != Some(*v) {
                fields.push((*col, olds[ix], *v));
            }
        }
        if fields.is_empty() {
            unchanged.push((serial, student));
            continue;
        }
        let final_ix = COLUMNS.len() - 1;
        let new_final = r.values.iter().find(|(c, _)| *c == "promedio_final").map(|(_, v)| *v).or(olds[final_ix]);
        changes.push(Change {
            serial,
            enrollment_uuid: uuid,
            student,
            fields,
            new_score: new_final.map(|f| f.round() as i64),
        });
    }
    let token = crate::auth::random_token();
    let unmatched = skipped.len();
    purge_previews(crate::clock::now());
    PREVIEWS.lock().unwrap_or_else(|e| e.into_inner()).insert(
        token.clone(),
        Stored { user_id: who.actor.user_id, created: crate::clock::now(), changes: changes.clone(), unmatched },
    );
    Ok(Preview { token, changes, unchanged, skipped })
}

/// What [`apply`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub updated: usize,
    pub unmatched: usize,
}

/// OP-ENR-07 step 2: apply the token's preview in this transaction. Another user's token, an expired or unknown
/// token: `NotFound` (T-05-12). The token is spent on success. Writes `grades.import` to the audit trail.
pub fn apply(tx: &Transaction, who: &Who, token: &str) -> DResult<Applied> {
    policy::ap09_bulk(who)?;
    let stored = {
        let mut map = PREVIEWS.lock().unwrap_or_else(|e| e.into_inner());
        let ok = map.get(token).is_some_and(|s| {
            s.user_id == who.actor.user_id
                && crate::clock::now() - s.created <= chrono::Duration::minutes(PREVIEW_MINUTES)
        });
        if !ok {
            return Err(DomainError::NotFound);
        }
        map.remove(token).ok_or(DomainError::NotFound)?
    };
    let now = crate::clock::now_text();
    let mut updated = 0;
    for c in &stored.changes {
        let mut sets: Vec<String> =
            c.fields.iter().enumerate().map(|(i, (col, _, _))| format!("{col} = ?{}", i + 1)).collect();
        let mut args: Vec<rusqlite::types::Value> =
            c.fields.iter().map(|(_, _, v)| rusqlite::types::Value::Real(*v)).collect();
        if c.fields.iter().any(|(col, _, _)| *col == "promedio_final") {
            args.push(rusqlite::types::Value::from(c.new_score));
            sets.push(format!("puntuacion = ?{}", args.len()));
        }
        args.push(now.clone().into());
        sets.push(format!("modified_at = ?{}", args.len()));
        args.push(who.actor.username.clone().into());
        sets.push(format!("modified_by = ?{}", args.len()));
        args.push(c.serial.into());
        let sql = format!("UPDATE enrollments_course SET {} WHERE serial = ?{}", sets.join(", "), args.len());
        updated += tx.execute(&sql, rusqlite::params_from_iter(args.iter()))?;
    }
    crate::audit::log(
        tx,
        who.actor.user_id,
        "grades.import",
        Some("enrollments_course"),
        None,
        Some(&serde_json::json!({ "updated": updated, "unmatched": stored.unmatched })),
    )?;
    Ok(Applied { updated, unmatched: stored.unmatched })
}

/// Deletes previews older than 30 minutes; returns how many (JOB-02).
pub fn purge_previews(now: NaiveDateTime) -> usize {
    let mut map = PREVIEWS.lock().unwrap_or_else(|e| e.into_inner());
    let before = map.len();
    map.retain(|_, s| now - s.created <= chrono::Duration::minutes(PREVIEW_MINUTES));
    before - map.len()
}

/// Puts a preview's age back by `minutes` (tests of the 30-minute limit).
#[doc(hidden)]
pub fn age_preview_for_tests(token: &str, minutes: i64) {
    if let Some(s) = PREVIEWS.lock().unwrap_or_else(|e| e.into_inner()).get_mut(token) {
        s.created -= chrono::Duration::minutes(minutes);
    }
}

/// The OP-ENR-08 by-hand rule (BR-ENR-03): when the final average is set, the score takes its rounded value.
/// `registries::patch` runs it after a `promedio_final` PATCH.
pub fn score_follows_final(conn: &Connection, who: &Who, enrollment: &str) -> rusqlite::Result<usize> {
    let n = conn.execute(
        "UPDATE enrollments_course SET puntuacion = CAST(ROUND(promedio_final) AS INTEGER)
          WHERE uuid = ?1 AND promedio_final IS NOT NULL AND (puntuacion IS NULL OR puntuacion <> CAST(ROUND(promedio_final) AS INTEGER))",
        params![enrollment],
    )?;
    if n > 0 {
        crate::audit::stamp_modified(conn, "enrollments_course", "uuid", &enrollment, &who.actor)?;
    }
    Ok(n)
}
