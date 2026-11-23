//! Time. Every timestamp is local civil time of Puerto Rico, `YYYY-MM-DD HH:MM:SS`, no offset (04 header, D-67).
//! America/Puerto_Rico is UTC-4 all year (no DST), so a fixed offset gives the same text on any server clock.

use chrono::{Duration, FixedOffset, NaiveDateTime, Utc};

pub const TIMESTAMP_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

fn puerto_rico() -> FixedOffset {
    FixedOffset::west_opt(4 * 3600).expect("valid offset")
}

/// Now, Puerto Rico civil time.
pub fn now() -> NaiveDateTime {
    Utc::now().with_timezone(&puerto_rico()).naive_local()
}

/// Now as stored text.
pub fn now_text() -> String {
    fmt(now())
}

pub fn fmt(t: NaiveDateTime) -> String {
    t.format(TIMESTAMP_FORMAT).to_string()
}

pub fn parse(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, TIMESTAMP_FORMAT).ok()
}

/// `now + minutes` as stored text (negative for the past).
pub fn in_minutes(minutes: i64) -> String {
    fmt(now() + Duration::minutes(minutes))
}

const MONTHS: [&str; 12] = [
    "enero",
    "febrero",
    "marzo",
    "abril",
    "mayo",
    "junio",
    "julio",
    "agosto",
    "septiembre",
    "octubre",
    "noviembre",
    "diciembre",
];

/// "14 de septiembre de 2026" (letters and print pages). Takes `YYYY-MM-DD` or a stored timestamp.
pub fn spanish_date(stored: &str) -> Option<String> {
    use chrono::Datelike;
    let d = chrono::NaiveDate::parse_from_str(stored.get(..10)?, "%Y-%m-%d").ok()?;
    Some(format!("{} de {} de {}", d.day(), MONTHS[d.month0() as usize], d.year()))
}

/// "lunes 23 de noviembre de 2026" (page bar of UI-02).
pub fn spanish_long_date(t: NaiveDateTime) -> String {
    use chrono::Datelike;
    const DAYS: [&str; 7] = ["lunes", "martes", "miércoles", "jueves", "viernes", "sábado", "domingo"];
    let d = t.date();
    format!(
        "{} {} de {} de {}",
        DAYS[d.weekday().num_days_from_monday() as usize],
        d.day(),
        MONTHS[d.month0() as usize],
        d.year()
    )
}

/// "4:42 p. m." (the time format of the design pages, es-PR).
pub fn spanish_time(t: NaiveDateTime) -> String {
    use chrono::Timelike;
    let (pm, h) = t.hour12();
    format!("{}:{:02} {}", h, t.minute(), if pm { "p. m." } else { "a. m." })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spanish_formats() {
        let t = parse("2026-11-23 16:42:00").unwrap();
        assert_eq!(spanish_long_date(t), "lunes 23 de noviembre de 2026");
        assert_eq!(spanish_date("2026-09-14").as_deref(), Some("14 de septiembre de 2026"));
        assert_eq!(spanish_date("2026-11-23 16:42:00").as_deref(), Some("23 de noviembre de 2026"));
        assert_eq!(spanish_date("x"), None);
        assert_eq!(spanish_time(t), "4:42 p. m.");
        assert_eq!(spanish_time(parse("2026-11-23 09:05:00").unwrap()), "9:05 a. m.");
    }
}
