//! Autosave registries of the I2 record resources (05 per-field PATCH rows) and [`patch`], the one entry point the
//! phase-2 handlers call: it runs `autosave::save` with the resource's domain check inside the transaction, then the
//! after-rule of the field (BR-EVT-01 end date, BR-EVT-06 build on a plan's event change, BR-REG-03 guest qty).
//!
//! Registration items use `autosave::registration::ITEM` (D-88: a qty PATCH sets `qty_manual` = 1).

use super::{events, plans, registrations, DomainError, Who};
use crate::autosave::{self, registration::ITEM, Ctx, FieldDef, Outcome, Refusal, Registry, Stamp};
use crate::error::AppError;
use rusqlite::types::Value;
use rusqlite::Connection;

// ---------------------------------------------------------------------------------------------------------------
// Parsers and display
// ---------------------------------------------------------------------------------------------------------------

/// Optional ISO date `YYYY-MM-DD`; also takes the design's `M/D/YYYY`.
pub fn parse_date(s: &str) -> Result<Value, String> {
    let t = s.trim();
    if t.is_empty() {
        return Ok(Value::Null);
    }
    chrono::NaiveDate::parse_from_str(t, "%Y-%m-%d")
        .or_else(|_| chrono::NaiveDate::parse_from_str(t, "%m/%d/%Y"))
        .map(|d| Value::Text(d.format("%Y-%m-%d").to_string()))
        .map_err(|_| "Use una fecha como 5/6/2027.".to_string())
}

/// "5/6/2027" from `2027-05-06` (design date format).
pub fn show_date(v: &Value) -> String {
    match v {
        Value::Text(t) => chrono::NaiveDate::parse_from_str(t, "%Y-%m-%d")
            .map(|d| d.format("%-m/%-d/%Y").to_string())
            .unwrap_or_else(|_| t.clone()),
        _ => String::new(),
    }
}

/// Optional time: "8:30 AM", "8:30 PM" or "20:30"; stored `HH:MM` (04).
pub fn parse_time(s: &str) -> Result<Value, String> {
    let t = s.trim().to_uppercase();
    if t.is_empty() {
        return Ok(Value::Null);
    }
    let bad = || "Use una hora como 8:30 AM.".to_string();
    let (hm, ampm) = match t.split_once(' ') {
        Some((hm, ap)) if ap == "AM" || ap == "PM" => (hm.to_owned(), Some(ap == "PM")),
        Some(_) => return Err(bad()),
        None => (t.clone(), None),
    };
    let (hs, ms) = hm.split_once(':').ok_or_else(bad)?;
    if ms.len() != 2 || hs.is_empty() || hs.len() > 2 {
        return Err(bad());
    }
    let (h, m): (u32, u32) = (hs.parse().map_err(|_| bad())?, ms.parse().map_err(|_| bad())?);
    if m > 59 {
        return Err(bad());
    }
    let h24 = match ampm {
        Some(pm) if (1..=12).contains(&h) => (h % 12) + if pm { 12 } else { 0 },
        Some(_) => return Err(bad()),
        None if h < 24 => h,
        None => return Err(bad()),
    };
    Ok(Value::Text(format!("{h24:02}:{m:02}")))
}

/// "8:30 AM" from `08:30`.
pub fn show_time(v: &Value) -> String {
    match v {
        Value::Text(t) => match t.split_once(':').and_then(|(h, m)| Some((h.parse::<u32>().ok()?, m))) {
            Some((h, m)) => format!(
                "{}:{} {}",
                if h % 12 == 0 { 12 } else { h % 12 },
                &m[..2.min(m.len())],
                if h < 12 { "AM" } else { "PM" }
            ),
            None => t.clone(),
        },
        _ => String::new(),
    }
}

/// Money in cents, required, 0 or more.
pub fn parse_money(s: &str) -> Result<Value, String> {
    autosave::cents(s).map(Value::Integer)
}

/// Money in cents, empty = NULL (price override: empty means "computed", 0 means "free"; T-07-06).
pub fn parse_money_opt(s: &str) -> Result<Value, String> {
    if s.trim().is_empty() {
        Ok(Value::Null)
    } else {
        parse_money(s)
    }
}

/// "1,218.00" from cents (NULL -> "").
pub fn show_money(v: &Value) -> String {
    match v {
        Value::Integer(c) => crate::seed::usd(*c).trim_start_matches('$').to_owned(),
        _ => String::new(),
    }
}

fn one_of(s: &str, allowed: &[&str], msg: &str) -> Result<Value, String> {
    let t = s.trim();
    allowed.iter().find(|a| **a == t).map(|a| Value::Text((*a).to_owned())).ok_or_else(|| msg.to_owned())
}

fn one_of_opt(s: &str, allowed: &[&str], msg: &str) -> Result<Value, String> {
    if s.trim().is_empty() {
        Ok(Value::Null)
    } else {
        one_of(s, allowed, msg)
    }
}

fn text200(s: &str) -> Result<Value, String> {
    autosave::optional_text(s, 200)
}
fn text100(s: &str) -> Result<Value, String> {
    autosave::optional_text(s, 100)
}
fn text30(s: &str) -> Result<Value, String> {
    autosave::optional_text(s, 30)
}
fn note(s: &str) -> Result<Value, String> {
    autosave::optional_text(s, 4000)
}
fn email(s: &str) -> Result<Value, String> {
    let v = autosave::optional_text(s, 200)?;
    if let Value::Text(t) = &v {
        let ok = t
            .split_once('@')
            .is_some_and(|(a, d)| !a.is_empty() && d.contains('.') && !d.starts_with('.') && !d.ends_with('.'));
        if !ok || t.contains(' ') {
            return Err("Escriba un e-mail como nombre@ejemplo.com.".into());
        }
    }
    Ok(v)
}
fn qty(s: &str) -> Result<Value, String> {
    autosave::int_in(s, 1, 999).map(Value::Integer)
}

// ---------------------------------------------------------------------------------------------------------------
// Registries (05 per-field PATCH rows of I2)
// ---------------------------------------------------------------------------------------------------------------

/// `PATCH /people/{id}` (UI-04, OP-PPL-01).
pub static PERSON: Registry = Registry {
    table: "people",
    key_col: "uuid",
    stamp: Stamp::Full,
    fields: &[
        FieldDef::new("company", "company", "Compañía", text200),
        FieldDef::new("name_first", "name_first", "Nombre", text100),
        FieldDef::new("name_last", "name_last", "Apellido", text100),
        FieldDef::new("name_badge", "name_badge", "Nombre badge", text100),
        FieldDef::new("phone1", "phone1", "Teléfono", text30),
        FieldDef::new("phone2", "phone2", "Teléfono 2", text30),
        FieldDef::new("fax", "fax", "Fax", text30),
        FieldDef::new("address", "address", "Dirección", text200),
        FieldDef::new("city", "city", "Ciudad", text100),
        FieldDef::new("state", "state", "Estado", text30),
        FieldDef::new("zip", "zip", "Zip", text30),
        FieldDef::new("email", "email", "E-mail", email),
    ],
};

fn reg_status(s: &str) -> Result<Value, String> {
    one_of(s, registrations::STATUSES, "Escoja un estatus de la lista.")
}
fn costeado(s: &str) -> Result<Value, String> {
    one_of_opt(s, &["Afiliado", "Individuo"], "Escoja Afiliado o Individuo.")
}

/// `PATCH /registrations/{id}` (UI-06 header): status, costeado por, comments, guest badge text.
pub static REGISTRATION: Registry = Registry {
    table: "registration",
    key_col: "uuid",
    stamp: Stamp::Full,
    fields: &[
        FieldDef::new("status", "status", "Estatus", reg_status),
        FieldDef::new("costeado_por", "costeado_por", "Costeado por", costeado),
        FieldDef::new("comments", "comments", "Comentarios", note),
        FieldDef::new("guest_name_badges", "guest_name_badges", "Nombres badge", text200),
    ],
};

/// `PATCH /registrations/{id}/guests/{guest_id}` (OP-REG-06).
pub static GUEST: Registry = Registry {
    table: "registration_guests",
    key_col: "uuid",
    stamp: Stamp::Full,
    fields: &[
        FieldDef::new("name_first", "name_first", "Nombre", text100),
        FieldDef::new("name_last", "name_last", "Apellido", text100),
        FieldDef::new("name_badge", "name_badge", "Nombre badge", text100),
        FieldDef::new("company", "company", "Compañía", text200),
    ],
};

fn event_status(s: &str) -> Result<Value, String> {
    one_of(s, &["Activo", "Inactivo"], "Escoja Activo o Inactivo.")
}
fn event_name(s: &str) -> Result<Value, String> {
    autosave::required_text(s, 200)
}

/// `PATCH /events/{id}` (UI-09 header, OP-EVT-02). An empty end date takes the start date (BR-EVT-01, after-rule).
pub static EVENT: Registry = Registry {
    table: "events",
    key_col: "uuid",
    stamp: Stamp::Full,
    fields: &[
        FieldDef::new("name", "name", "Nombre", event_name),
        FieldDef::new("location", "location", "Lugar", text200),
        FieldDef::new("date_start", "date_start", "Comienza", parse_date).show(show_date),
        FieldDef::new("date_end", "date_end", "Termina", parse_date).show(show_date),
        FieldDef::new("status", "status", "Estatus", event_status),
    ],
};

fn item_type(s: &str) -> Result<Value, String> {
    one_of(s, events::ITEM_TYPES, "Escoja un tipo de la lista.")
}

/// `PATCH /event-items/{id}` (UI-09 Actividades, UI-10). `date_end` is not editable: it mirrors `date_start`
/// (BR-EVT-01).
pub static EVENT_ITEM: Registry = Registry {
    table: "event_items",
    key_col: "uuid",
    stamp: Stamp::Full,
    fields: &[
        FieldDef::new("name", "name", "Nombre", event_name),
        FieldDef::new("type", "type", "Tipo", item_type),
        FieldDef::new("date_start", "date_start", "Fecha", parse_date).show(show_date).mirror(&["date_end"]),
        FieldDef::new("time_start", "time_start", "Hora comienza", parse_time).show(show_time),
        FieldDef::new("time_end", "time_end", "Hora termina", parse_time).show(show_time),
        FieldDef::new("location_room", "location_room", "Lugar/Salón", text100),
        FieldDef::new("qty", "qty", "Qty", qty),
        FieldDef::new("price", "price", "Precio", parse_money).show(show_money),
    ],
};

fn sponsor_name(s: &str) -> Result<Value, String> {
    autosave::required_text(s, 200)
}
fn sponsor_type(s: &str) -> Result<Value, String> {
    one_of_opt(s, super::sponsors::CATEGORIES, "Escoja una categoría de la lista.")
}

/// `PATCH /sponsors/{id}` (UI-11, UI-09 Auspiciadores; OP-SPN-01).
pub static SPONSOR: Registry = Registry {
    table: "sponsor",
    key_col: "uuid",
    stamp: Stamp::Full,
    fields: &[
        FieldDef::new("sponsor_name", "sponsor_name", "Auspiciador", sponsor_name),
        FieldDef::new("type", "type", "Tipo", sponsor_type),
        FieldDef::new("requested_amount", "requested_amount", "Pedido", parse_money_opt).show(show_money),
        FieldDef::new("promised", "promised", "Prometido", parse_money_opt).show(show_money),
        FieldDef::new("amount_received", "amount_received", "Recibido", parse_money_opt).show(show_money),
        FieldDef::new("contact_name", "contact_name", "Contacto", text200),
        FieldDef::new("title", "title", "Título", text100),
        FieldDef::new("greeting", "greeting", "Saludo", text200),
        FieldDef::new("address", "address", "Dirección", text200),
        FieldDef::new("city", "city", "Ciudad", text100),
        FieldDef::new("state", "state", "Estado", text30),
        FieldDef::new("zip", "zip", "Zip", text30),
        FieldDef::new("phone", "phone", "Teléfono", text30),
    ],
};

fn plan_name(s: &str) -> Result<Value, String> {
    autosave::required_text(s, 200)
}
fn plan_event(s: &str) -> Result<Value, String> {
    autosave::required_text(s, 64)
}

/// `PATCH /plans/{id}` (UI-13): name, price override (empty = computed, BR-EVT-05), event (active only; the
/// after-rule builds the plan items, OP-EVT-04).
pub static PLAN: Registry = Registry {
    table: "packages",
    key_col: "uuid",
    stamp: Stamp::Full,
    fields: &[
        FieldDef::new("name", "name", "Plan", plan_name),
        FieldDef::new("price_override", "price_override", "Precio fijo", parse_money_opt).show(show_money),
        FieldDef::new("event_uuid", "event_uuid", "Evento", plan_event),
    ],
};

/// `PATCH /plan-items/{id}` (UI-13): included flag and qty (BR-EVT-05 price follows).
pub static PLAN_ITEM: Registry = Registry {
    table: "package_items",
    key_col: "uuid",
    stamp: Stamp::Full,
    fields: &[
        FieldDef::new("active", "active", "Incluir", autosave::flag),
        FieldDef::new("ticket_qty", "ticket_qty", "Qty.", qty),
    ],
};

// ---------------------------------------------------------------------------------------------------------------
// The entry point
// ---------------------------------------------------------------------------------------------------------------

/// The I2 autosave resources.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resource {
    Person,
    Registration,
    Item,
    Guest,
    Event,
    EventItem,
    Sponsor,
    Plan,
    PlanItem,
}

impl Resource {
    pub fn registry(self) -> &'static Registry {
        match self {
            Resource::Person => &PERSON,
            Resource::Registration => &REGISTRATION,
            Resource::Item => &ITEM,
            Resource::Guest => &GUEST,
            Resource::Event => &EVENT,
            Resource::EventItem => &EVENT_ITEM,
            Resource::Sponsor => &SPONSOR,
            Resource::Plan => &PLAN,
            Resource::PlanItem => &PLAN_ITEM,
        }
    }
}

/// The record a PATCH writes: the resource, its uuid, and for items and guests the registration it must belong to.
#[derive(Clone, Copy, Debug)]
pub struct Target<'a> {
    pub resource: Resource,
    pub key: &'a str,
    pub parent: Option<&'a str>,
}

impl<'a> Target<'a> {
    pub fn new(resource: Resource, key: &'a str) -> Self {
        Self { resource, key, parent: None }
    }
    /// A registration item or guest of `registration_uuid`.
    pub fn child(resource: Resource, key: &'a str, registration_uuid: &'a str) -> Self {
        Self { resource, key, parent: Some(registration_uuid) }
    }
}

/// What a PATCH did, for the handler's response.
#[derive(Debug)]
pub enum Patched {
    /// Field outside the allow list: answer `autosave::contract_error` (422, nothing swapped).
    Unknown,
    /// Saved (or unchanged) or refused; same as `autosave::Outcome`.
    Done(Outcome),
}

/// Saves one field of one I2 record, with its domain rules. Policy: AP-03 (both roles); the route also has `RequireStaff`.
pub fn patch(
    conn: &mut Connection,
    who: &Who,
    request_id: Option<&str>,
    target: Target,
    name: &str,
    raw: &str,
) -> Result<Patched, AppError> {
    super::policy::ap03_domain(who).map_err(|_| AppError::Forbidden)?;
    let Target { resource, key, parent } = target;
    let reg = resource.registry();
    let Some(def) = reg.get(name) else { return Ok(Patched::Unknown) };
    if let (Some(p), Resource::Item | Resource::Guest) = (parent, resource) {
        let n: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM {} WHERE uuid = ?1 AND registration_uuid = ?2", reg.table),
            [key, p],
            |r| r.get(0),
        )?;
        if n == 0 {
            return Err(AppError::NotFound);
        }
    }
    let exists: i64 =
        conn.query_row(&format!("SELECT COUNT(*) FROM {} WHERE uuid = ?1", reg.table), [key], |r| r.get(0))?;
    if exists == 0 {
        return Err(AppError::NotFound);
    }
    let record = record_label(resource);
    let ctx = Ctx { actor: &who.actor, request_id, record };
    let keyv = Value::Text(key.to_owned());
    let outcome = autosave::save(conn, reg, &keyv, def, raw, &ctx, |tx, _old, new| {
        match (resource, def.name) {
            (Resource::Item, f @ ("hotel_reservation_num" | "adicional" | "mesa")) => {
                registrations::attribute_allowed(tx, key, f).map_err(refusal)?
            }
            (Resource::Plan, "event_uuid") => {
                if let Value::Text(ev) = new {
                    plans::check_active_event(tx, ev).map_err(refusal)?;
                }
            }
            _ => {}
        }
        Ok(())
    })?;
    if let Outcome::Saved { changed: true, .. } = &outcome {
        let tx = super::begin(conn)?;
        match (resource, def.name) {
            (Resource::Event, "date_start" | "date_end") => {
                events::default_end_date(&tx, who, key)?;
            }
            (Resource::Plan, "event_uuid") => {
                plans::after_event_change(&tx, who, key).map_err(domain_to_app)?;
            }
            (Resource::Guest, _) => {
                let r: String =
                    tx.query_row("SELECT registration_uuid FROM registration_guests WHERE uuid = ?1", [key], |r| {
                        r.get(0)
                    })?;
                registrations::after_guest_edit(&tx, who, &r).map_err(domain_to_app)?;
            }
            _ => {}
        }
        tx.commit()?;
    }
    Ok(Patched::Done(outcome))
}

fn record_label(r: Resource) -> &'static str {
    match r {
        Resource::Person => "Convencionista",
        Resource::Registration => "Registro",
        Resource::Item => "Línea de registro",
        Resource::Guest => "Acompañante",
        Resource::Event => "Convención",
        Resource::EventItem => "Actividad",
        Resource::Sponsor => "Auspiciador",
        Resource::Plan => "Plan",
        Resource::PlanItem => "Línea de plan",
    }
}

fn refusal(e: DomainError) -> Refusal {
    match e {
        DomainError::Db(m) => Refusal::Db(rusqlite::Error::InvalidParameterName(m)),
        e => Refusal::Invalid(e.message()),
    }
}

fn domain_to_app(e: DomainError) -> AppError {
    match e {
        DomainError::Forbidden => AppError::Forbidden,
        DomainError::NotFound => AppError::NotFound,
        e => AppError::Internal(format!("convention after-rule: {e:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsers() {
        assert_eq!(parse_time("8:30 AM").unwrap(), Value::Text("08:30".into()));
        assert_eq!(parse_time("12:00 PM").unwrap(), Value::Text("12:00".into()));
        assert_eq!(parse_time("12:15 am").unwrap(), Value::Text("00:15".into()));
        assert_eq!(parse_time("19:00").unwrap(), Value::Text("19:00".into()));
        assert!(parse_time("13:00 PM").is_err() && parse_time("8.30").is_err() && parse_time("8:75").is_err());
        assert_eq!(show_time(&Value::Text("19:00".into())), "7:00 PM");
        assert_eq!(show_time(&Value::Text("00:15".into())), "12:15 AM");
        assert_eq!(parse_date("5/6/2027").unwrap(), Value::Text("2027-05-06".into()));
        assert_eq!(show_date(&Value::Text("2027-05-06".into())), "5/6/2027");
        assert_eq!(parse_money("$1,218.00").unwrap(), Value::Integer(121800));
        assert_eq!(parse_money_opt("").unwrap(), Value::Null);
        assert_eq!(parse_money_opt("0").unwrap(), Value::Integer(0));
        assert_eq!(show_money(&Value::Integer(121800)), "1,218.00");
        assert!(email("maria@ejemplo").is_err() && email("maria@ejemplo.com").is_ok());
    }
}
