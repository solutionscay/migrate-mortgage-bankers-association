//! Per-field autosave: the `PATCH` contract of 05 "Per-field autosave (PATCH)".
//!
//! A record page has no Save button. Each field sends `hx-patch` with `hx-trigger="change"` and only its own value.
//! The pieces here, used by every domain `PATCH` handler:
//!
//! * [`OneField`]: the body extractor. Exactly one field (form fields `_csrf` and `_method` do not count), else 422.
//! * [`Registry`] / [`FieldDef`]: per resource, the allow list of fields with their column, parser and validator.
//! * [`save`]: one `BEGIN IMMEDIATE` transaction per field: read the old value, run the domain check, write one
//!   column (last write wins per field), stamp the row, and write one `audit_log` `field.change` row with the old
//!   and new value (masked for secret fields, BR-SYS-02).
//! * [`FieldView`]: the field fragment. 200 shows the saved, normalised value. 422 shows the value as typed,
//!   `aria-invalid`, the Spanish message and `data-unsaved` on the wrapper. `data-saved` carries the saved value for
//!   the Esc revert in `app.js`.
//! * [`oob`] / [`totales`]: out-of-band fragments for line fields (registration totals and other figures).
//!
//! CSRF is enforced for every PATCH by `auth::csrf_guard` (header `X-CSRF-Token` from `hx-headers` on `<body>`).

pub mod registration;

use crate::audit::{self, Actor};
use crate::error::AppError;
use askama::Template;
use axum::body::Bytes;
use axum::extract::{FromRequest, Request};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::Router;
use rusqlite::types::Value;
use rusqlite::{params_from_iter, Connection, Transaction, TransactionBehavior};
use serde_json::json;

use crate::AppState;

/// No routes of its own: each domain module serves `PATCH` on its own paths and calls the helpers here.
pub fn routes() -> Router<AppState> {
    Router::new()
}

/// What an audit row shows instead of a secret value.
pub const MASK: &str = "••••••";

// ---------------------------------------------------------------------------------------------------------------
// The request: exactly one field
// ---------------------------------------------------------------------------------------------------------------

/// The one field of an autosave `PATCH` body (`campo=valor`, form-encoded).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OneField {
    pub name: String,
    pub value: String,
}

impl OneField {
    /// Parses a url-encoded body. `_csrf` and `_method` are transport fields and do not count. Zero fields, two or
    /// more fields, or one field sent twice: `Err`.
    pub fn parse(body: &[u8]) -> Result<OneField, &'static str> {
        let mut fields = crate::web::form_pairs(body)
            .into_iter()
            .filter(|(k, _)| k != "_csrf" && k != "_method")
            .collect::<Vec<_>>();
        match fields.len() {
            0 => Err("La solicitud no trae ningún campo."),
            1 => {
                let (name, value) = fields.remove(0);
                Ok(OneField { name, value })
            }
            _ => Err("Se guarda un campo a la vez."),
        }
    }
}

impl<S: Send + Sync> FromRequest<S> for OneField {
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let body = Bytes::from_request(req, state).await.map_err(IntoResponse::into_response)?;
        OneField::parse(&body).map_err(contract_error)
    }
}

/// 422 for a request that breaks the contract (no field, several fields, a field outside the allow list). The page
/// never sends one, so nothing on the page is swapped (`HX-Reswap: none`).
pub fn contract_error(message: &'static str) -> Response {
    let mut res = (StatusCode::UNPROCESSABLE_ENTITY, Html(message)).into_response();
    res.headers_mut().insert("hx-reswap", HeaderValue::from_static("none"));
    res
}

// ---------------------------------------------------------------------------------------------------------------
// The registry: allowed fields per resource
// ---------------------------------------------------------------------------------------------------------------

/// Parses the typed text into the stored value, or returns the Spanish message for the field.
pub type Parser = fn(&str) -> Result<Value, String>;
/// Turns a stored value into the text the field shows.
pub type Shower = fn(&Value) -> String;

/// One field a resource lets the page change.
pub struct FieldDef {
    /// Form field name (`name=` on the control).
    pub name: &'static str,
    /// Column written.
    pub column: &'static str,
    /// Spanish label, for the audit text.
    pub label: &'static str,
    pub parse: Parser,
    /// Display of the stored value; `None` = the plain text of the value.
    pub show: Option<Shower>,
    /// Extra columns set by every PATCH of this field (for example `qty_manual = 1`, D-88).
    pub also: &'static [(&'static str, i64)],
    /// Never shown in the audit trail (BR-SYS-02).
    pub secret: bool,
    /// Columns that always take the same new value in the same UPDATE (BR-EVT-01: an event item's `date_end`
    /// follows `date_start`).
    pub mirror: &'static [&'static str],
}

impl FieldDef {
    pub const fn new(name: &'static str, column: &'static str, label: &'static str, parse: Parser) -> Self {
        Self { name, column, label, parse, show: None, also: &[], secret: false, mirror: &[] }
    }
    pub const fn mirror(mut self, cols: &'static [&'static str]) -> Self {
        self.mirror = cols;
        self
    }
    pub const fn show(mut self, show: Shower) -> Self {
        self.show = Some(show);
        self
    }
    pub const fn also(mut self, also: &'static [(&'static str, i64)]) -> Self {
        self.also = also;
        self
    }
    pub const fn secret(mut self) -> Self {
        self.secret = true;
        self
    }
    pub fn display(&self, v: &Value) -> String {
        match self.show {
            Some(f) => f(v),
            None => text_of(v),
        }
    }
}

/// Which stamp columns the table has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stamp {
    /// `modified_at` and `modified_by` (BR-SYS-02).
    Full,
    /// `modified_at` only (`users`).
    AtOnly,
}

/// The allow list of one resource: its table, key column and fields.
pub struct Registry {
    pub table: &'static str,
    pub key_col: &'static str,
    pub stamp: Stamp,
    pub fields: &'static [FieldDef],
}

impl Registry {
    pub fn get(&self, name: &str) -> Option<&FieldDef> {
        self.fields.iter().find(|f| f.name == name)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Parsers (shared by the registries)
// ---------------------------------------------------------------------------------------------------------------

/// Required text, trimmed, at most `max` characters.
pub fn required_text(s: &str, max: usize) -> Result<Value, String> {
    let t = s.trim();
    if t.is_empty() {
        Err("Escriba un valor.".into())
    } else if t.chars().count() > max {
        Err(format!("Use {max} caracteres o menos."))
    } else {
        Ok(Value::Text(t.to_owned()))
    }
}

/// Optional text: empty becomes NULL.
pub fn optional_text(s: &str, max: usize) -> Result<Value, String> {
    let t = s.trim();
    if t.is_empty() {
        Ok(Value::Null)
    } else if t.chars().count() > max {
        Err(format!("Use {max} caracteres o menos."))
    } else {
        Ok(Value::Text(t.to_owned()))
    }
}

/// Whole number in `lo..=hi`.
pub fn int_in(s: &str, lo: i64, hi: i64) -> Result<i64, String> {
    match s.trim().parse::<i64>() {
        Ok(n) if (lo..=hi).contains(&n) => Ok(n),
        Ok(_) => Err(format!("Use un número entre {lo} y {hi}.")),
        Err(_) => Err("Escriba un número entero.".into()),
    }
}

/// Dollar text ("1.50", "$1,250", "3") to integer cents (04 convention). At most two decimals, not negative.
pub fn cents(s: &str) -> Result<i64, String> {
    let t: String = s.trim().trim_start_matches('$').chars().filter(|c| *c != ',').collect();
    let bad = || "Escriba una cantidad en dólares, por ejemplo 1.50.".to_string();
    let (whole, frac) = t.split_once('.').unwrap_or((&t, ""));
    if whole.is_empty() && frac.is_empty()
        || frac.len() > 2
        || !whole.chars().chain(frac.chars()).all(|c| c.is_ascii_digit())
    {
        return Err(bad());
    }
    let w: i64 = if whole.is_empty() { 0 } else { whole.parse().map_err(|_| bad())? };
    let f: i64 = format!("{frac:0<2}").parse().map_err(|_| bad())?;
    w.checked_mul(100).and_then(|c| c.checked_add(f)).ok_or_else(bad)
}

/// "1.50" from 150.
pub fn dollars(cents: i64) -> String {
    format!("{}.{:02}", cents / 100, cents % 100)
}

/// A yes/no control (`1`/`0`, `on`/`off`, `true`/`false`).
pub fn flag(s: &str) -> Result<Value, String> {
    match s.trim() {
        "1" | "on" | "true" => Ok(Value::Integer(1)),
        "0" | "off" | "false" => Ok(Value::Integer(0)),
        _ => Err("Escoja Sí o No.".into()),
    }
}

/// The plain text of a stored value ("" for NULL).
pub fn text_of(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Integer(i) => i.to_string(),
        Value::Real(r) => r.to_string(),
        Value::Text(s) => s.clone(),
        Value::Blob(_) => String::new(),
    }
}

fn json_of(v: &Value) -> serde_json::Value {
    match v {
        Value::Null | Value::Blob(_) => serde_json::Value::Null,
        Value::Integer(i) => json!(i),
        Value::Real(r) => json!(r),
        Value::Text(s) => json!(s),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The write: one field, one transaction, one audit row
// ---------------------------------------------------------------------------------------------------------------

/// Who saves and where the request came from (the audit row carries both).
pub struct Ctx<'a> {
    pub actor: &'a Actor,
    pub request_id: Option<&'a str>,
    /// Short Spanish name of the record for the audit text, for example "Usuario equintana".
    pub record: &'a str,
}

/// A domain check refused the value (422), or the database failed.
pub enum Refusal {
    Invalid(String),
    Db(rusqlite::Error),
}

impl From<rusqlite::Error> for Refusal {
    fn from(e: rusqlite::Error) -> Self {
        Refusal::Db(e)
    }
}

/// The result of [`save`].
#[derive(Debug)]
pub enum Outcome {
    /// Written (or unchanged). `new` is the stored, normalised value.
    Saved { old: Value, new: Value, changed: bool },
    /// Nothing written; the message goes into the field fragment (422).
    Invalid(String),
}

/// Saves one field of one row.
///
/// `check` runs inside the transaction after the old value is read and before the write. It can refuse the value
/// (`Refusal::Invalid`), or make side effects that belong to the same change (for example delete the sessions of a
/// user whose role changed); a refusal rolls them back.
pub fn save<F>(
    conn: &mut Connection,
    reg: &Registry,
    key: &Value,
    def: &FieldDef,
    raw: &str,
    ctx: &Ctx,
    check: F,
) -> Result<Outcome, AppError>
where
    F: FnOnce(&Transaction, &Value, &Value) -> Result<(), Refusal>,
{
    let new = match (def.parse)(raw) {
        Ok(v) => v,
        Err(msg) => return Ok(Outcome::Invalid(msg)),
    };
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let old: Value =
        tx.query_row(&format!("SELECT {} FROM {} WHERE {} = ?1", def.column, reg.table, reg.key_col), [key], |r| {
            r.get(0)
        })?;
    match check(&tx, &old, &new) {
        Ok(()) => {}
        Err(Refusal::Invalid(msg)) => return Ok(Outcome::Invalid(msg)),
        Err(Refusal::Db(e)) => return Err(e.into()),
    }
    let changed = old != new;
    if changed || !def.also.is_empty() {
        let mut sets = vec![format!("{} = ?1", def.column)];
        for col in def.mirror {
            sets.push(format!("{col} = ?1"));
        }
        let mut args = vec![new.clone()];
        for (col, v) in def.also {
            args.push(Value::Integer(*v));
            sets.push(format!("{col} = ?{}", args.len()));
        }
        args.push(key.clone());
        let sql = format!("UPDATE {} SET {} WHERE {} = ?{}", reg.table, sets.join(", "), reg.key_col, args.len());
        tx.execute(&sql, params_from_iter(args.iter()))?;
        match reg.stamp {
            Stamp::Full => {
                audit::stamp_modified(&tx, reg.table, reg.key_col, key, ctx.actor)?;
            }
            Stamp::AtOnly => {
                tx.execute(
                    &format!("UPDATE {} SET modified_at = ?1 WHERE {} = ?2", reg.table, reg.key_col),
                    rusqlite::params![crate::clock::now_text(), key],
                )?;
            }
        }
    }
    if changed {
        let (o, n) = if def.secret { (json!(MASK), json!(MASK)) } else { (json_of(&old), json_of(&new)) };
        let detail = json!({
            "table": reg.table,
            "id": json_of(key),
            "field": def.column,
            "old": o,
            "new": n,
            "actor": ctx.actor.username,
            "request_id": ctx.request_id,
            "text": format!("{} · {}", ctx.record, def.label),
        });
        audit::log(&tx, ctx.actor.user_id, "field.change", Some(reg.table), Some(&text_of(key)), Some(&detail))?;
    }
    tx.commit()?;
    Ok(Outcome::Saved { old, new, changed })
}

// ---------------------------------------------------------------------------------------------------------------
// The response: the field fragment
// ---------------------------------------------------------------------------------------------------------------

/// The control of a field.
#[derive(Debug, Clone)]
pub enum Control {
    Text,
    /// Whole number (`inputmode="numeric"`).
    Number,
    /// Dollars with the `$` adornment.
    Money,
    /// `(value, label)` pairs.
    Select(Vec<(String, String)>),
    Textarea(u16),
}

/// One autosave field: a `.field-wrap` with its control, as the page and the PATCH response render it.
#[derive(Template, Debug, Clone)]
#[template(path = "autosave/field.html")]
pub struct FieldView {
    /// `hx-patch` target.
    pub url: String,
    pub name: String,
    pub id: String,
    /// `aria-label` when no `<label for>` names the control.
    pub aria: Option<String>,
    pub control: Control,
    /// What the control shows: the saved value, or the typed value after a refusal.
    pub value: String,
    /// The saved value (`data-saved`, used by Esc).
    pub saved: String,
    pub error: Option<String>,
    pub hint: Option<String>,
    /// Extra classes on the control or adornment (for example ` ma-short`).
    pub class: &'static str,
}

impl FieldView {
    pub fn new(url: impl Into<String>, name: &str, id: &str, control: Control, saved: impl Into<String>) -> Self {
        let saved = saved.into();
        Self {
            url: url.into(),
            name: name.to_owned(),
            id: id.to_owned(),
            aria: None,
            control,
            value: saved.clone(),
            saved,
            error: None,
            hint: None,
            class: "",
        }
    }
    pub fn aria(mut self, label: &str) -> Self {
        self.aria = Some(label.to_owned());
        self
    }
    pub fn hint(mut self, hint: &str) -> Self {
        self.hint = Some(hint.to_owned());
        self
    }
    pub fn class(mut self, class: &'static str) -> Self {
        self.class = class;
        self
    }
    /// The error state: the value as typed and the message (422).
    pub fn refused(mut self, typed: &str, message: impl Into<String>) -> Self {
        self.value = typed.to_owned();
        self.error = Some(message.into());
        self
    }
    /// HTML for a page template (`{{ f.html()|safe }}`).
    pub fn html(&self) -> String {
        self.render().unwrap_or_else(|e| {
            tracing::error!(error = %e, "field template");
            String::new()
        })
    }
    pub fn is_selected(&self, v: &str) -> bool {
        self.value == v
    }
}

/// 200 with the saved field, or 422 with the refused field. `extra` is appended (out-of-band fragments).
pub fn respond(view: &FieldView, extra: &str) -> Response {
    let status = if view.error.is_some() { StatusCode::UNPROCESSABLE_ENTITY } else { StatusCode::OK };
    let mut html = view.html();
    html.push_str(extra);
    (status, Html(html)).into_response()
}

/// An out-of-band fragment that replaces the inner HTML of `#id` (htmx `hx-swap-oob`).
pub fn oob(id: &str, inner_html: &str) -> String {
    format!("<div hx-swap-oob=\"innerHTML:#{id}\">{inner_html}</div>")
}

/// The registration totals block (`#totales`) out of band, for line fields (I2 item routes call this).
pub fn totales(inner_html: &str) -> String {
    oob("totales", inner_html)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_field_only() {
        assert_eq!(
            OneField::parse(b"_csrf=x&display_name=Ana+P%C3%A9rez").unwrap(),
            OneField { name: "display_name".into(), value: "Ana Pérez".into() }
        );
        assert!(OneField::parse(b"_csrf=x").is_err());
        assert!(OneField::parse(b"").is_err());
        assert!(OneField::parse(b"a=1&b=2").is_err());
        assert!(OneField::parse(b"a=1&a=2").is_err());
        assert_eq!(OneField::parse(b"_method=PATCH&active=0").unwrap().name, "active");
    }

    #[test]
    fn money_and_numbers() {
        assert_eq!(cents("1.50"), Ok(150));
        assert_eq!(cents("$1,250"), Ok(125_000));
        assert_eq!(cents("3.5"), Ok(350));
        assert_eq!(cents(".75"), Ok(75));
        for bad in ["", "1.505", "-2", "abc", "."] {
            assert!(cents(bad).is_err(), "{bad}");
        }
        assert_eq!(dollars(150), "1.50");
        assert_eq!(int_in("10", 1, 480), Ok(10));
        assert!(int_in("0", 1, 480).is_err());
        assert!(int_in("x", 1, 480).is_err());
    }

    static T: Registry = Registry {
        table: "app_settings",
        key_col: "key",
        stamp: Stamp::Full,
        fields: &[
            FieldDef::new("v", "value", "Valor", |s| required_text(s, 20)),
            FieldDef::new("s", "value", "Secreto", |s| required_text(s, 20)).secret(),
        ],
    };

    #[test]
    fn save_writes_one_column_and_one_audit_row() {
        let mut conn = crate::db::open_in_memory().unwrap();
        let actor = Actor { user_id: None, username: "mvega".into() };
        let ctx = Ctx { actor: &actor, request_id: Some("r1"), record: "Configuración" };
        let key = Value::Text("csbs.state_abbr".into());
        let out = save(&mut conn, &T, &key, T.get("v").unwrap(), " PR ", &ctx, |_, _, _| Ok(())).unwrap();
        assert!(matches!(out, Outcome::Saved { changed: true, .. }));
        let (v, by): (String, String) = conn
            .query_row("SELECT value, modified_by FROM app_settings WHERE key = 'csbs.state_abbr'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((v.as_str(), by.as_str()), ("PR", "mvega"));
        let detail: String =
            conn.query_row("SELECT detail FROM audit_log WHERE action = 'field.change'", [], |r| r.get(0)).unwrap();
        let d: serde_json::Value = serde_json::from_str(&detail).unwrap();
        assert_eq!(
            (d["old"].as_str(), d["new"].as_str(), d["request_id"].as_str()),
            (Some("CS"), Some("PR"), Some("r1"))
        );

        // Same value again: no write, no audit row.
        save(&mut conn, &T, &key, T.get("v").unwrap(), "PR", &ctx, |_, _, _| Ok(())).unwrap();
        // Refused by parser and by check: nothing written.
        assert!(matches!(
            save(&mut conn, &T, &key, T.get("v").unwrap(), "", &ctx, |_, _, _| Ok(())).unwrap(),
            Outcome::Invalid(_)
        ));
        assert!(matches!(
            save(&mut conn, &T, &key, T.get("v").unwrap(), "XX", &ctx, |_, _, _| Err(Refusal::Invalid("no".into())))
                .unwrap(),
            Outcome::Invalid(_)
        ));
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM audit_log", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);

        // A secret field is masked in the trail (BR-SYS-02).
        save(&mut conn, &T, &key, T.get("s").unwrap(), "clave-123", &ctx, |_, _, _| Ok(())).unwrap();
        let detail: String =
            conn.query_row("SELECT detail FROM audit_log ORDER BY id DESC LIMIT 1", [], |r| r.get(0)).unwrap();
        assert!(!detail.contains("clave-123") && detail.contains(MASK));
    }

    #[test]
    fn fragment_states() {
        let f = FieldView::new("/admin/settings", "org.name", "cf-org", Control::Text, "MBA");
        let ok = f.html();
        assert!(ok.contains("data-saved=\"MBA\"") && ok.contains("hx-patch=\"/admin/settings\""));
        assert!(!ok.contains("data-unsaved"));
        let bad = f.clone().refused("", "Escriba un valor.").html();
        assert!(
            bad.contains("data-unsaved") && bad.contains("aria-invalid=\"true\"") && bad.contains("Escriba un valor.")
        );
        assert!(bad.contains("value=\"\"") && bad.contains("data-saved=\"MBA\""));
        assert_eq!(respond(&f.clone().refused("x", "m"), "").status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert!(totales("<b>$1.00</b>").contains("hx-swap-oob=\"innerHTML:#totales\""));
    }
}
