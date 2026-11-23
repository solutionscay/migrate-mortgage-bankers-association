//! OP-SYS-04 upload and download files (BR-SYS-04, AP-03; 05 `/files`).
//!
//! * One `stored_files` row per upload: original name, MIME type from the extension (never the client header),
//!   size, SHA-256. Bytes live on disk under `<data dir>/files/<2 hex>/<sha256>`, so equal bytes are stored once.
//!   `storage_path` holds the path relative to the data dir. The data dir comes from config and is never in git.
//! * Limits (05 "Rate/size/pagination limits"): 20 MB per file, empty files refused, only the types in [`ALLOWED`].
//! * Download: the stored MIME type; `Content-Disposition: inline` for PDF and images, `attachment` for the rest.
//! * Every upload writes an `audit_log` row `file.upload`.
//!
//! I2 reuses two pieces: [`store`] (the domain operation, for routes such as `POST /events/{id}/pdf`) and
//! [`FileField`] (the file chip of the design, or the "Escoger archivo" button when empty).

use crate::audit::{self, Actor};
use crate::auth::RequireStaff;
use crate::error::AppError;
use crate::{clock, db::sha256_hex, AppState};
use askama::Template;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path as FsPath, PathBuf};

/// 05: uploads max 20 MB.
pub const MAX_FILE_BYTES: usize = 20 * 1024 * 1024;

/// Accepted extensions and their MIME type. SVG and HTML are left out on purpose: the browser runs script in them.
pub const ALLOWED: &[(&str, &str)] = &[
    ("pdf", "application/pdf"),
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("xlsx", "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
    ("xls", "application/vnd.ms-excel"),
    ("docx", "application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
    ("doc", "application/msword"),
    ("csv", "text/csv"),
    ("txt", "text/plain"),
];

/// CSP on downloaded files.
const FILE_CSP: &str =
    "default-src 'none'; img-src 'self'; style-src 'unsafe-inline'; object-src 'self'; frame-ancestors 'none'";

/// The `accept` attribute that matches [`ALLOWED`].
pub const ACCEPT: &str = ".pdf,.png,.jpg,.jpeg,.gif,.webp,.xlsx,.xls,.docx,.doc,.csv,.txt";
/// The `accept` attribute for images only (the logo).
pub const IMAGE_ACCEPT: &str = ".png,.jpg,.jpeg,.gif,.webp";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/files", post(upload))
        .route("/files/field", get(empty_field))
        .route("/files/{uuid}", get(download))
}

/// A `stored_files` row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredFile {
    pub uuid: String,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub storage_path: String,
}

impl StoredFile {
    /// "38 KB", "1.2 MB" (the chip text of the design).
    pub fn size_label(&self) -> String {
        size_label(self.size_bytes)
    }
}

pub fn size_label(n: i64) -> String {
    const KB: f64 = 1024.0;
    let n = n as f64;
    if n < KB {
        format!("{n} B")
    } else if n < KB * KB {
        format!("{:.0} KB", (n / KB).ceil())
    } else {
        format!("{:.1} MB", n / KB / KB)
    }
}

/// Why an upload is refused. The text is the Spanish field error.
#[derive(Debug, PartialEq, Eq)]
pub enum FileError {
    Empty,
    TooLarge,
    BadType,
    NoFile,
}

impl FileError {
    pub fn message(&self) -> &'static str {
        match self {
            FileError::Empty => "El archivo está vacío.",
            FileError::TooLarge => "El archivo pasa de 20 MB.",
            FileError::BadType => "Tipo de archivo no permitido. Use PDF, imagen, Excel, Word, CSV o texto.",
            FileError::NoFile => "Escoja un archivo.",
        }
    }
}

/// The MIME type for a file name, or `None` when the extension is not in [`ALLOWED`].
pub fn mime_for(filename: &str) -> Option<&'static str> {
    let ext = FsPath::new(filename).extension()?.to_str()?.to_ascii_lowercase();
    ALLOWED.iter().find(|(e, _)| *e == ext).map(|(_, m)| *m)
}

/// The base name only, no path parts or control characters, at most 200 characters.
pub fn clean_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let s: String = base.chars().filter(|c| !c.is_control()).take(200).collect();
    let s = s.trim().to_owned();
    if s.is_empty() || s == "." || s == ".." {
        "archivo".into()
    } else {
        s
    }
}

/// OP-SYS-04 store: checks the limits, writes the bytes under `data_dir`, inserts `stored_files` and the audit row.
/// The caller runs it once per file; it takes the DB lock only for the insert.
pub fn store(state: &AppState, actor: &Actor, filename: &str, bytes: &[u8]) -> Result<StoredFile, StoreError> {
    let filename = clean_filename(filename);
    let mime = mime_for(&filename).ok_or(StoreError::Rejected(FileError::BadType))?;
    if bytes.is_empty() {
        return Err(StoreError::Rejected(FileError::Empty));
    }
    if bytes.len() > MAX_FILE_BYTES {
        return Err(StoreError::Rejected(FileError::TooLarge));
    }
    let sha = sha256_hex(bytes);
    let rel = format!("files/{}/{}", &sha[..2], sha);
    let abs = state.config.data_dir.join(&rel);
    if !abs.exists() {
        write_atomic(&abs, bytes).map_err(|e| StoreError::Internal(format!("write file: {e}")))?;
    }
    let file = StoredFile {
        uuid: uuid::Uuid::new_v4().to_string(),
        filename,
        mime_type: mime.to_owned(),
        size_bytes: bytes.len() as i64,
        sha256: sha,
        storage_path: rel,
    };
    let conn = state.db();
    insert(&conn, &file, actor).map_err(|e| StoreError::Internal(format!("insert stored_files: {e}")))?;
    Ok(file)
}

#[derive(Debug)]
pub enum StoreError {
    Rejected(FileError),
    Internal(String),
}

fn insert(conn: &Connection, f: &StoredFile, actor: &Actor) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO stored_files (uuid, filename, mime_type, size_bytes, sha256, storage_path, created_at, created_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            f.uuid,
            f.filename,
            f.mime_type,
            f.size_bytes,
            f.sha256,
            f.storage_path,
            clock::now_text(),
            actor.username
        ],
    )?;
    audit::log(
        &tx,
        actor.user_id,
        "file.upload",
        Some("stored_files"),
        Some(&f.uuid),
        Some(&serde_json::json!({ "filename": f.filename, "mime_type": f.mime_type, "size_bytes": f.size_bytes })),
    )?;
    tx.commit()
}

fn write_atomic(path: &FsPath, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().expect("files/<xx>/ has a parent");
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".tmp-{}", uuid::Uuid::new_v4().simple()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// One `stored_files` row by uuid.
pub fn get_file(conn: &Connection, uuid: &str) -> rusqlite::Result<Option<StoredFile>> {
    conn.query_row(
        "SELECT uuid, filename, mime_type, size_bytes, sha256, storage_path FROM stored_files WHERE uuid = ?1",
        [uuid],
        |r| {
            Ok(StoredFile {
                uuid: r.get(0)?,
                filename: r.get(1)?,
                mime_type: r.get(2)?,
                size_bytes: r.get(3)?,
                sha256: r.get(4)?,
                storage_path: r.get(5)?,
            })
        },
    )
    .optional()
}

// --- File field fragment ----------------------------------------------------------------------------------------

/// The reusable file widget: the design file chip (`.chip`, `#i-file`, the `x` button) with a hidden input that
/// carries the `stored_files.uuid`, or, when empty, the "Escoger archivo" button. Wrap target: `.file-field`.
///
/// * `name`: the hidden input name (for example `pdf_file_uuid`, `logo_file_uuid`).
/// * `label`: what the file is, for the `aria-label` texts ("PDF", "logo").
/// * `remove_url`: where the `x` sends `DELETE` (I2: `/events/{id}/pdf`). Without it the `x` only resets the widget.
#[derive(Template, Clone, Debug)]
#[template(path = "sys/file_field.html")]
pub struct FileField {
    pub name: String,
    pub label: String,
    pub file: Option<StoredFile>,
    pub remove_url: Option<String>,
    pub error: Option<&'static str>,
    pub accept: &'static str,
    /// Autosave target (05 per-field PATCH): a fresh upload sends `name=<uuid>` there on load, and the `x` sends
    /// `name=` (empty). The PATCH answers with this widget. Without it, a form around the widget submits the uuid.
    pub save_url: Option<String>,
    /// Just uploaded and not saved yet: the hidden input fires the autosave PATCH when htmx swaps it in.
    pub fresh: bool,
}

impl FileField {
    pub fn new(name: impl Into<String>, label: impl Into<String>, file: Option<StoredFile>) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            file,
            remove_url: None,
            error: None,
            accept: ACCEPT,
            save_url: None,
            fresh: false,
        }
    }

    /// Autosave the uuid to `url` (for example `/admin/settings` for `logo_file_uuid`).
    pub fn with_save_url(mut self, url: impl Into<String>) -> Self {
        self.save_url = Some(url.into());
        self
    }

    /// Narrows the `accept` attribute (the server still checks the type).
    pub fn with_accept(mut self, accept: &'static str) -> Self {
        self.accept = accept;
        self
    }

    /// The hint next to the button, by the accepted types.
    pub fn hint(&self) -> &'static str {
        if self.accept == IMAGE_ACCEPT {
            "PNG, JPEG, GIF o WebP; hasta 20 MB"
        } else {
            "PDF, imagen, Excel, Word, CSV o texto; hasta 20 MB"
        }
    }

    /// `hx-vals` of the `x` when the widget autosaves: the field set to empty.
    pub fn clear_vals(&self) -> String {
        serde_json::json!({ self.name.clone(): "" }).to_string()
    }

    pub fn with_remove_url(mut self, url: impl Into<String>) -> Self {
        self.remove_url = Some(url.into());
        self
    }

    /// `hx-vals` for the upload input: carries name and label to `POST /files`, so the chip comes back the same.
    pub fn hx_vals(&self) -> String {
        let mut v = serde_json::json!({ "name": self.name, "label": self.label });
        if let Some(url) = &self.save_url {
            v["save"] = serde_json::Value::from(url.clone());
        }
        v.to_string()
    }

    /// The query string of `GET /files/field` for this widget.
    pub fn reset_query(&self) -> String {
        format!("name={}&label={}", crate::web::urlencode(&self.name), crate::web::urlencode(&self.label))
    }
}

/// Field name and label from the client: only safe characters, so they can go into attributes and JSON.
fn clean_ident(s: &str, fallback: &str, allow_space: bool) -> String {
    let s: String = s
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || (allow_space && *c == ' '))
        .take(60)
        .collect();
    if s.trim().is_empty() {
        fallback.into()
    } else {
        s
    }
}

/// A save URL from the client: a local path of safe characters only, or nothing. It goes into an attribute.
fn clean_local_path(s: &str) -> Option<String> {
    let ok = s.starts_with('/')
        && !s.starts_with("//")
        && s.len() <= 100
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '.'));
    ok.then(|| s.to_owned())
}

#[derive(serde::Deserialize)]
pub struct FieldQuery {
    #[serde(default)]
    name: String,
    #[serde(default)]
    label: String,
}

/// `GET /files/field`: the empty widget (what the `x` of an unsaved chip swaps back in).
async fn empty_field(_: RequireStaff, Query(q): Query<FieldQuery>) -> Result<Html<String>, AppError> {
    let f = FileField::new(clean_ident(&q.name, "file_uuid", false), clean_ident(&q.label, "archivo", true), None);
    Ok(Html(f.render()?))
}

/// `POST /files` (AP-03): multipart with `file`, plus optional `name` and `label` for the returned widget. 200 with
/// the chip, or 422 with the empty widget and the field error (the htmx config swaps 422).
async fn upload(staff: RequireStaff, State(state): State<AppState>, mut mp: Multipart) -> Response {
    let mut name = String::new();
    let mut label = String::new();
    let mut save = String::new();
    let mut picked: Option<(String, Result<Vec<u8>, FileError>)> = None;
    loop {
        let mut field = match mp.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => return (StatusCode::BAD_REQUEST, format!("multipart: {}", e.body_text())).into_response(),
        };
        match field.name().unwrap_or_default() {
            "file" if picked.is_none() => {
                let filename = field.file_name().unwrap_or_default().to_owned();
                let mut buf = Vec::new();
                let mut too_large = false;
                loop {
                    match field.chunk().await {
                        Ok(Some(c)) => {
                            if buf.len() + c.len() > MAX_FILE_BYTES {
                                too_large = true;
                                break;
                            }
                            buf.extend_from_slice(&c);
                        }
                        Ok(None) => break,
                        Err(e) => {
                            return (StatusCode::BAD_REQUEST, format!("multipart: {}", e.body_text())).into_response()
                        }
                    }
                }
                let bytes = if too_large { Err(FileError::TooLarge) } else { Ok(buf) };
                picked = Some((filename, bytes));
            }
            "name" => name = field.text().await.unwrap_or_default(),
            "label" => label = field.text().await.unwrap_or_default(),
            "save" => save = field.text().await.unwrap_or_default(),
            _ => {}
        }
    }
    let mut widget = FileField::new(clean_ident(&name, "file_uuid", false), clean_ident(&label, "archivo", true), None);
    widget.save_url = clean_local_path(&save);
    let result = match picked {
        Some((filename, _)) if filename.is_empty() => Err(FileError::NoFile),
        Some((filename, Ok(bytes))) => {
            let actor = staff.0.actor();
            let st = state.clone();
            match tokio::task::spawn_blocking(move || store(&st, &actor, &filename, &bytes)).await {
                Ok(Ok(f)) => Ok(f),
                Ok(Err(StoreError::Rejected(e))) => Err(e),
                Ok(Err(StoreError::Internal(m))) => return AppError::Internal(m).into_response(),
                Err(e) => return AppError::Internal(format!("upload task: {e}")).into_response(),
            }
        }
        Some((_, Err(e))) => Err(e),
        None => Err(FileError::NoFile),
    };
    let status = match result {
        Ok(f) => {
            widget.file = Some(f);
            widget.fresh = true;
            StatusCode::OK
        }
        Err(e) => {
            widget.error = Some(e.message());
            StatusCode::UNPROCESSABLE_ENTITY
        }
    };
    match widget.render() {
        Ok(html) => (status, Html(html)).into_response(),
        Err(e) => AppError::from(e).into_response(),
    }
}

/// `GET /files/{uuid}` (AP-03): the bytes with the stored MIME type.
async fn download(
    _: RequireStaff,
    State(state): State<AppState>,
    Path(uuid): Path<String>,
) -> Result<Response, AppError> {
    let file = get_file(&state.db(), &uuid)?.ok_or(AppError::NotFound)?;
    let path = resolve(&state.config.data_dir, &file.storage_path).ok_or(AppError::NotFound)?;
    let read = tokio::task::spawn_blocking(move || std::fs::read(path))
        .await
        .map_err(|e| AppError::Internal(format!("read task: {e}")))?;
    let bytes = read.map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => AppError::NotFound,
        _ => AppError::Internal(format!("read file: {e}")),
    })?;
    let mut res = bytes.into_response();
    let h = res.headers_mut();
    let ct = HeaderValue::from_str(&file.mime_type).unwrap_or(HeaderValue::from_static("application/octet-stream"));
    h.insert(header::CONTENT_TYPE, ct);
    if let Ok(v) = HeaderValue::from_str(&content_disposition(&file.filename, &file.mime_type)) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    // Own CSP for file bytes: no script, but `object-src 'self'` so the browser PDF viewer may render an inline PDF
    // (the app CSP has `object-src 'none'`; `security_headers` keeps a CSP the handler already set).
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(FILE_CSP));
    Ok(res)
}

/// The stored relative path under `data_dir`, refusing anything that climbs out of `files/`.
fn resolve(data_dir: &FsPath, rel: &str) -> Option<PathBuf> {
    let p = FsPath::new(rel);
    let ok = p.starts_with("files") && p.components().all(|c| matches!(c, std::path::Component::Normal(_)));
    ok.then(|| data_dir.join(p))
}

/// `inline` for PDF and images (05), `attachment` for the rest. ASCII fallback name plus the UTF-8 name (RFC 6266).
pub fn content_disposition(filename: &str, mime: &str) -> String {
    let kind = if mime == "application/pdf" || mime.starts_with("image/") { "inline" } else { "attachment" };
    let ascii: String = filename
        .chars()
        .map(|c| if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' { c } else { '_' })
        .collect();
    let utf8: String = filename
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' | b'_' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect();
    format!("{kind}; filename=\"{ascii}\"; filename*=UTF-8''{utf8}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_names_and_headers() {
        assert_eq!(mime_for("Programa.PDF"), Some("application/pdf"));
        assert_eq!(mime_for("logo.svg"), None);
        assert_eq!(mime_for("sin-extension"), None);
        assert_eq!(clean_filename("C:\\Users\\x\\logo.png"), "logo.png");
        assert_eq!(clean_filename("../../etc/passwd"), "passwd");
        assert_eq!(clean_filename(".."), "archivo");
        assert_eq!(
            content_disposition("año 2027.pdf", "application/pdf"),
            "inline; filename=\"a_o 2027.pdf\"; filename*=UTF-8''a%C3%B1o%202027.pdf"
        );
        assert!(content_disposition("notas.xlsx", ALLOWED[6].1).starts_with("attachment;"));
        assert_eq!(size_label(38 * 1024), "38 KB");
        assert_eq!(size_label(214 * 1024 + 5), "215 KB");
        assert!(resolve(FsPath::new("/d"), "files/ab/x").is_some());
        assert!(resolve(FsPath::new("/d"), "files/../x").is_none());
        assert!(resolve(FsPath::new("/d"), "/etc/passwd").is_none());
    }
}
