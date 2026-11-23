//! OP-SYS-03 `mba-seed`: deterministic synthetic data (D-02). The same seed number gives the same database, row for
//! row (T-03-02): every uuid, salt, pick and timestamp comes from one ChaCha generator seeded with that number, and no
//! row takes the wall clock. All names, companies and figures are fake; every e-mail address is `@ejemplo.com`.
//! No SMTP host, SMTP password or licence key is written (D-05).
//!
//! Users: the dummy accounts of 08 "Identity provisioning" (mvega, inieves, equintana) plus the `admin.demo` and
//! `staff.demo` accounts the M1 design pages show. All five get [`DEMO_PASSWORD`], printed by the tool.

use crate::auth::password;
use crate::db::{self, DbError};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use rusqlite::{params, Connection, Transaction};

/// The demo password of every seeded account. Demo only: synthetic data, no real account (D-02, D-07).
pub const DEMO_PASSWORD: &str = "demo-demo-demo";
pub const DEFAULT_SEED: u64 = 2026;
/// `created_by` / `modified_by` of seeded rows.
pub const SEED_ACTOR: &str = "mba-seed";

/// (username, display name, role). 08 "Demo seed users", plus the design's two role accounts.
pub const ACCOUNTS: &[(&str, &str, &str)] = &[
    ("mvega", "Marisol Vega Ortiz", "admin"),
    ("inieves", "Ivelisse Nieves Arroyo", "admin"),
    ("equintana", "Edwin Quintana Roldán", "staff"),
    ("admin.demo", "Administración Demo", "admin"),
    ("staff.demo", "Personal Demo", "staff"),
];

/// Row counts of the main tables, for the tool's report.
#[derive(Debug, Default)]
pub struct Summary {
    pub rows: Vec<(&'static str, i64)>,
}

const FIRST: &[&str] = &[
    "Ana", "Luis", "Carmen", "José", "María", "Carlos", "Elena", "Jorge", "Sofía", "Miguel", "Isabel", "Rafael",
    "Lucía", "Pedro", "Gabriela", "Andrés", "Valeria", "Héctor", "Natalia", "Ramón", "Paola", "Samuel", "Diana",
    "Felipe", "Mónica", "Javier", "Teresa", "Ricardo", "Beatriz", "Tomás",
];
const LAST: &[&str] = &[
    "Ejemplo",
    "Muestra",
    "Prueba",
    "Ficticio",
    "Modelo",
    "Demo",
    "Simulado",
    "Ensayo",
    "Patrón",
    "Borrador",
    "Plantilla",
    "Supuesto",
    "Inventado",
    "Genérico",
    "Figurado",
    "Imaginario",
];
const COMPANIES: &[&str] = &[
    "Banco Ejemplo de Puerto Rico",
    "Hipotecaria Muestra",
    "Cooperativa Prueba",
    "Financiera Ficticia",
    "Préstamos Modelo",
    "Crédito Demo",
    "Hogar Simulado Mortgage",
    "Fondo Ensayo",
    "Hipotecas Patrón",
    "Banca Borrador",
    "Servicios Plantilla",
    "Capital Supuesto",
    "Inversiones Inventadas",
    "Mortgage Genérico",
];
const CITIES: &[&str] = &["San Juan", "Bayamón", "Carolina", "Ponce", "Caguas", "Guaynabo", "Mayagüez", "Arecibo"];
const COURSES: &[&str] = &[
    "Fundamentos de Préstamos Hipotecarios",
    "Leyes y Reglamentos Federales",
    "Análisis de Crédito",
    "Tasación y Valoración",
    "Cierre de Préstamos",
    "Servicio de Préstamos",
    "Ética Profesional",
    "Taller de Cumplimiento",
];
const SEMINARS: &[&str] =
    &["Seminario de Ética Profesional", "Seminario de Prevención de Fraude", "Seminario de Ventas"];
const SHORT_COURSES: &[&str] = &["Educación Continua NMLS (8 h)", "Actualización Reglamentaria", "Préstamos FHA y VA"];
const PROFESSORS: &[&str] = &[
    "Dra. Laura Ejemplo Ruiz",
    "Lcdo. Mario Muestra Vélez",
    "Prof. Irma Prueba Soto",
    "Lcda. Nilda Modelo Cruz",
    "Prof. Iván Demo Ortiz",
];

struct Gen {
    rng: ChaCha8Rng,
    /// Seed clock: every timestamp is `BASE` plus an offset, never the wall clock.
    base: chrono::NaiveDateTime,
}

impl Gen {
    fn new(seed: u64) -> Self {
        Self {
            rng: ChaCha8Rng::seed_from_u64(seed),
            base: crate::clock::parse("2026-11-02 08:00:00").expect("base date"),
        }
    }

    /// UUID v4 text from the seeded generator.
    fn uuid(&mut self) -> String {
        let mut b: [u8; 16] = self.rng.random();
        b[6] = (b[6] & 0x0f) | 0x40;
        b[8] = (b[8] & 0x3f) | 0x80;
        uuid::Uuid::from_bytes(b).to_string()
    }

    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        self.rng.random_range(lo..=hi)
    }

    fn chance(&mut self, p: f64) -> bool {
        self.rng.random_bool(p)
    }

    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.rng.random_range(0..xs.len())]
    }

    /// A timestamp `minutes` after the seed base.
    fn at(&self, minutes: i64) -> String {
        crate::clock::fmt(self.base + chrono::Duration::minutes(minutes))
    }

    /// A random timestamp in the 19 days after the base, office hours.
    fn stamp(&mut self) -> String {
        let day = self.range(0, 18);
        let minute = self.range(0, 9 * 60);
        self.at(day * 24 * 60 + minute)
    }

    fn person(&mut self) -> (String, String, String) {
        let first = (*self.pick(FIRST)).to_owned();
        let l1 = (*self.pick(LAST)).to_owned();
        let mut l2 = (*self.pick(LAST)).to_owned();
        if l2 == l1 {
            l2 = "Ejemplar".into();
        }
        (first, l1, l2)
    }

    fn phone(&mut self) -> String {
        format!("787-555-01{:02}", self.range(0, 99))
    }
}

/// `nombre.apellido@ejemplo.com`, ASCII only.
fn email(first: &str, last: &str, n: i64) -> String {
    format!("{}.{}{}@ejemplo.com", ascii(first), ascii(last), n)
}

fn ascii(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' => 'a',
            'é' => 'e',
            'í' => 'i',
            'ó' => 'o',
            'ú' | 'ü' => 'u',
            'ñ' => 'n',
            c => c,
        })
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

/// "$1,234.50" from integer cents.
pub fn usd(cents: i64) -> String {
    let neg = cents < 0;
    let c = cents.abs();
    let dollars = (c / 100).to_string();
    let mut grouped = String::new();
    for (i, ch) in dollars.chars().enumerate() {
        if i > 0 && (dollars.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    format!("{}${}.{:02}", if neg { "-" } else { "" }, grouped, c % 100)
}

/// Seeds an empty, migrated database. Refuses a database that already has users.
pub fn run(conn: &mut Connection, seed: u64) -> Result<Summary, DbError> {
    run_in(conn, seed, None)
}

/// [`run`], and with a data folder also writes the seeded files (event PDF, billing PDFs) under `files/`, where
/// `sys::files` serves them (OP-SYS-04).
pub fn run_in(conn: &mut Connection, seed: u64, data_dir: Option<&std::path::Path>) -> Result<Summary, DbError> {
    let users: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
    if users > 0 {
        return Err(DbError::Check("the database already has users; seed a new file".into()));
    }
    let mut g = Gen::new(seed);

    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    seed_users(&tx, &mut g)?;
    tx.commit()?;

    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let school = seed_school(&tx, &mut g)?;
    tx.commit()?;

    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let mut files = Vec::new();
    let convention = seed_convention(&tx, &mut g, &mut files)?;
    tx.commit()?;
    if let Some(dir) = data_dir {
        for f in &files {
            let path = dir.join(&f.storage_path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| DbError::Check(format!("create {}: {e}", parent.display())))?;
            }
            std::fs::write(&path, &f.bytes).map_err(|e| DbError::Check(format!("write {}: {e}", path.display())))?;
        }
    }

    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let mail = seed_mail(&tx, &mut g, &school.students)?;
    seed_settings_and_activity(&tx, &g, &school, &convention, &mail)?;
    tx.commit()?;

    db::integrity_check(conn)?;
    db::foreign_key_check(conn)?;

    let mut summary = Summary::default();
    for t in [
        "users",
        "affiliates",
        "students",
        "courses",
        "calendar_course",
        "enrollments_course",
        "enrollments_seminars",
        "enrollments_short_course",
        "people",
        "events",
        "event_items",
        "packages",
        "registration",
        "registration_items",
        "payments",
        "sponsor",
        "mail_batches",
        "mail_emails",
        "audit_log",
    ] {
        let n: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0))?;
        summary.rows.push((t, n));
    }
    Ok(summary)
}

fn seed_users(tx: &Transaction, g: &mut Gen) -> rusqlite::Result<()> {
    for (i, (username, name, role)) in ACCOUNTS.iter().enumerate() {
        let salt: [u8; 16] = g.rng.random();
        let at = g.at(i as i64);
        tx.execute(
            "INSERT INTO users (username, display_name, email, password_hash, role, active, created_at, modified_at)
             VALUES (?1, ?2, NULL, ?3, ?4, 1, ?5, ?5)",
            params![username, name, password::hash_with_salt(DEMO_PASSWORD, &salt), role, at],
        )?;
    }
    Ok(())
}

struct School {
    students: Vec<(String, String, String)>, // (uuid, nombre, email)
    first_course: (String, String),          // (course name, first date) of a Pendiente calendar row
    seminar_enrollment: (String, String),    // (student, seminar)
}

fn seed_school(tx: &Transaction, g: &mut Gen) -> rusqlite::Result<School> {
    let a = SEED_ACTOR;
    // Affiliates.
    let mut affiliates = Vec::new();
    for (i, socio) in COMPANIES.iter().enumerate() {
        let uuid = g.uuid();
        let at = g.stamp();
        let city = *g.pick(CITIES);
        let miembro = *g.pick(&["REGULAR", "ASOCIADO", "CORRESPONSAL"]);
        let cust = if i % 5 == 4 { "INACTIVO" } else { "ACTIVO" };
        let phone = g.phone();
        tx.execute(
            "INSERT INTO affiliates (uuid, account_num, socio, address, city, state_zip, telephone, miembro,
               tipo_de_negocio, e_mail, zip, cust_type, created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, 'PR', ?6, ?7, 'Banca hipotecaria', ?8, ?9, ?10, ?11, ?12, ?11, ?12)",
            params![
                uuid,
                1001 + i as i64,
                socio,
                format!("Calle Ejemplo {}", 100 + i),
                city,
                phone,
                miembro,
                format!("contacto{}@ejemplo.com", i + 1),
                format!("00{}", 900 + i),
                cust,
                at,
                a
            ],
        )?;
        affiliates.push((uuid, (*socio).to_owned()));
    }

    // Professors.
    let mut professors = Vec::new();
    for name in PROFESSORS {
        let uuid = g.uuid();
        let at = g.stamp();
        tx.execute(
            "INSERT INTO professors (uuid, name, created_at, created_by, modified_at, modified_by) VALUES (?1, ?2, ?3, ?4, ?3, ?4)",
            params![uuid, name, at, a],
        )?;
        professors.push(uuid);
    }

    // Courses, seminars, short courses.
    let mut courses = Vec::new();
    for name in COURSES {
        let uuid = g.uuid();
        let at = g.stamp();
        let member = g.range(15, 30) * 1000;
        tx.execute(
            "INSERT INTO courses (uuid, course, course_description, cost_member, cost_individual, credits, hours, quorum,
               created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, 3, 30, 8, ?6, ?7, ?6, ?7)",
            params![uuid, name, format!("Curso de {}", name.to_lowercase()), member, member + 5000, at, a],
        )?;
        courses.push((uuid, (*name).to_owned(), member));
    }
    let mut seminars = Vec::new();
    for name in SEMINARS {
        let uuid = g.uuid();
        let at = g.stamp();
        let price = g.range(8, 15) * 1000;
        tx.execute(
            "INSERT INTO seminars (uuid, seminar, cost_individual, cost_affiliate, hours, created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, 4, ?5, ?6, ?5, ?6)",
            params![uuid, name, price + 2500, price, at, a],
        )?;
        seminars.push((uuid, (*name).to_owned(), price));
    }
    let mut short_courses = Vec::new();
    for (i, name) in SHORT_COURSES.iter().enumerate() {
        let uuid = g.uuid();
        let at = g.stamp();
        let price = g.range(10, 20) * 1000;
        tx.execute(
            "INSERT INTO short_courses (uuid, short_course, cost_individual, cost_affiliate, hours, nmls_fee,
               created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, 8, ?5, ?6, ?7, ?6, ?7)",
            params![uuid, name, price + 3000, price, (i == 0) as i64, at, a],
        )?;
        short_courses.push((uuid, price));
    }

    // Certifications and requirements.
    let certs = [
        ("GMB", "Gerente Hipotecario Certificado", 1, vec![0, 1, 2, 3, 4], vec![5, 6]),
        ("OCH", "Oficial de Cumplimiento Hipotecario", 0, vec![1, 6, 7], vec![]),
    ];
    let mut cert_uuids = Vec::new();
    for (code, desc, electives, req, elec) in certs.iter() {
        let uuid = g.uuid();
        let at = g.stamp();
        tx.execute(
            "INSERT INTO certifications (uuid, certification, description, number_of_required_electives, created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?5, ?6)",
            params![uuid, code, desc, electives, at, a],
        )?;
        for (list, kind) in [(req, "Requisito"), (elec, "Electiva")] {
            for &ci in list.iter() {
                let ru = g.uuid();
                tx.execute(
                    "INSERT INTO certification_requirements (uuid, certification_uuid, course_uuid, type, created_at, created_by, modified_at, modified_by)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?5, ?6)",
                    params![ru, uuid, courses[ci].0, kind, at, a],
                )?;
            }
        }
        cert_uuids.push(uuid);
    }

    // Students: one student_certifications row per certification (BR-STU-01).
    let mut students = Vec::new();
    for i in 0..80i64 {
        let uuid = g.uuid();
        let at = g.stamp();
        let (first, l1, l2) = g.person();
        let nombre = format!("{first} {l1} {l2}");
        let member = if g.chance(0.7) { Some(g.pick(&affiliates).clone()) } else { None };
        let mail = email(&first, &l1, i + 1);
        let phone = g.phone();
        let city = *g.pick(CITIES);
        tx.execute(
            "INSERT INTO students (uuid, member_uuid, compania, nombre, email, phone_mobile, home_city, home_state,
               nmls, student_id, created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'PR', ?8, ?9, ?10, ?11, ?10, ?11)",
            params![
                uuid,
                member.as_ref().map(|m| m.0.clone()),
                member.as_ref().map(|m| m.1.clone()),
                nombre,
                mail,
                phone,
                city,
                format!("{}", 1_500_000 + i * 37),
                format!("E-2026-{:04}", i + 1),
                at,
                a
            ],
        )?;
        for cu in &cert_uuids {
            let su = g.uuid();
            tx.execute(
                "INSERT INTO student_certifications (uuid, student_uuid, certification_uuid, date, created_at, created_by, modified_at, modified_by)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?5, ?6)",
                params![su, uuid, cu, &at[..10], at, a],
            )?;
        }
        students.push((uuid, nombre, mail, member.is_some()));
    }

    // Course calendar rows with class dates, and enrollments.
    let plan: [(&str, &str, &str); 10] = [
        ("Completado", "Septiembre 2026", "2026-09-08"),
        ("Completado", "Septiembre 2026", "2026-09-15"),
        ("Completado", "Septiembre 2026", "2026-10-06"),
        ("Confirmado", "Septiembre 2026", "2026-11-03"),
        ("Confirmado", "Septiembre 2026", "2026-11-10"),
        ("Confirmado", "Febrero 2027", "2026-12-01"),
        ("Confirmado", "Febrero 2027", "2027-02-02"),
        ("Pendiente", "Febrero 2027", "2027-02-09"),
        ("Pendiente", "Febrero 2027", "2027-05-06"),
        ("Cancelado", "Febrero 2027", "2027-02-16"),
    ];
    let mut serial = 0i64;
    let mut first_course = (String::new(), String::new());
    for (i, (status, semester, start)) in plan.iter().enumerate() {
        let course = if i == 8 { courses[7].clone() } else { g.pick(&courses).clone() };
        let cal = g.uuid();
        let at = g.stamp();
        tx.execute(
            "INSERT INTO calendar_course (uuid, serial, course_uuid, status, semester, cost_member, cost_individual,
               created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?8, ?9)",
            params![cal, i as i64 + 1, course.0, status, semester, course.2, course.2 + 5000, at, a],
        )?;
        let start_date = chrono::NaiveDate::parse_from_str(start, "%Y-%m-%d").expect("date");
        let prof = g.pick(&professors).clone();
        for w in 0..4 {
            let du = g.uuid();
            let d = (start_date + chrono::Duration::weeks(w)).format("%Y-%m-%d").to_string();
            tx.execute(
                "INSERT INTO calendar_course_dates (uuid, calendar_course_uuid, date, status, professor_uuid, created_at, created_by, modified_at, modified_by)
                 VALUES (?1, ?2, ?3, 'Día de Clase', ?4, ?5, ?6, ?5, ?6)",
                params![du, cal, d, prof, at, a],
            )?;
        }
        if i == 8 {
            let d = start_date.format("%-d/%-m/%Y").to_string();
            first_course = (course.1.clone(), d);
        }
        if *status == "Cancelado" {
            continue;
        }
        let n = g.range(6, 14) as usize;
        let mut picked: Vec<usize> = Vec::new();
        while picked.len() < n {
            let s = g.range(0, students.len() as i64 - 1) as usize;
            if !picked.contains(&s) {
                picked.push(s);
            }
        }
        for s in picked {
            serial += 1;
            let (su, _, _, is_member) = &students[s];
            let (paid_by, cost) = if *is_member { ("COMPAÑIA", course.2) } else { ("INDIVIDUAL", course.2 + 5000) };
            let pago = match g.range(0, 3) {
                0 => 0,
                1 => cost / 2,
                _ => cost,
            };
            let score = if *status == "Completado" { Some(g.range(62, 100)) } else { None };
            let baja = g.chance(0.08);
            let eu = g.uuid();
            let at = g.stamp();
            tx.execute(
                "INSERT INTO enrollments_course (uuid, serial, student_uuid, course_uuid, course_calendar_uuid, costeado_por,
                   costo, pago, id_num, invoice_num, fecha_de_pago, puntuacion, promedio_final, status,
                   created_at, created_by, modified_at, modified_by)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12, ?13, ?14, ?15, ?14, ?15)",
                params![
                    eu,
                    serial,
                    su,
                    course.0,
                    cal,
                    paid_by,
                    cost,
                    pago,
                    1000 + serial,
                    5000 + serial,
                    if pago > 0 { Some(at[..10].to_owned()) } else { None },
                    score,
                    if baja { "BAJA" } else { "ACTIVO" },
                    at,
                    a
                ],
            )?;
        }
    }

    // Seminars: one calendar row each, with enrollments.
    let mut seminar_enrollment = (String::new(), String::new());
    let mut invoice = 7000i64;
    for (i, (su, name, price)) in seminars.iter().enumerate() {
        let cal = g.uuid();
        let at = g.stamp();
        let date = ["2026-10-20", "2026-12-08", "2027-03-02"][i];
        let status = ["Completado", "Confirmado", "Pendiente"][i];
        let prof = g.pick(&professors).clone();
        tx.execute(
            "INSERT INTO calendar_seminar (uuid, serial, seminar_uuid, status, date, quorum, cost_member, cost_individual,
               professor_uuid, created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, 10, ?6, ?7, ?8, ?9, ?10, ?9, ?10)",
            params![cal, i as i64 + 1, su, status, date, price, price + 2500, prof, at, a],
        )?;
        for k in 0..g.range(6, 10) {
            let s = &students[g.range(0, students.len() as i64 - 1) as usize];
            invoice += 1;
            let eu = g.uuid();
            let at = g.stamp();
            let st = if status == "Completado" {
                Some(*g.pick(&["Asistió", "Asistió", "Asistió", "No Canceló"]))
            } else {
                None
            };
            let charge = if s.3 { *price } else { price + 2500 };
            tx.execute(
                "INSERT INTO enrollments_seminars (uuid, serial, student_uuid, seminar_uuid, seminar_calendar_uuid, status,
                   cargo, costeado_por, pago, invoice_number, created_at, created_by, modified_at, modified_by)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?11, ?12)",
                params![
                    eu,
                    k + 1,
                    s.0,
                    su,
                    cal,
                    st,
                    charge,
                    if s.3 { "COMPAÑIA" } else { "INDIVIDUAL" },
                    if g.chance(0.7) { charge } else { 0 },
                    invoice,
                    at,
                    a
                ],
            )?;
            if i == 1 && k == 0 {
                seminar_enrollment = (s.1.clone(), name.clone());
            }
        }
    }

    // Short courses: two calendar rows with three dates each, and enrollments.
    for (i, (scu, price)) in short_courses.iter().take(2).enumerate() {
        let cal = g.uuid();
        let at = g.stamp();
        tx.execute(
            "INSERT INTO calendar_short_course (uuid, serial, short_course_uuid, status, quorum, cost_member, cost_individual, hours,
               created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, 12, ?5, ?6, 8, ?7, ?8, ?7, ?8)",
            params![cal, i as i64 + 1, scu, ["Confirmado", "Pendiente"][i], price, price + 3000, at, a],
        )?;
        let prof = g.pick(&professors).clone();
        for rep in 1..=3i64 {
            let du = g.uuid();
            let d = format!("2027-0{}-{:02}", i + 1, 10 + rep);
            tx.execute(
                "INSERT INTO calendar_short_course_dates (uuid, calendar_short_course_uuid, repetition, date, status, professor_uuid)
                 VALUES (?1, ?2, ?3, ?4, 'Día de Clase', ?5)",
                params![du, cal, rep, d, prof],
            )?;
        }
        for k in 0..g.range(5, 9) {
            let s = &students[g.range(0, students.len() as i64 - 1) as usize];
            invoice += 1;
            let eu = g.uuid();
            let at = g.stamp();
            let charge = if s.3 { *price } else { price + 3000 };
            tx.execute(
                "INSERT INTO enrollments_short_course (uuid, serial, student_uuid, short_course_uuid, short_course_calendar_uuid,
                   invoice_number, cargo, costeado_por, pago, license_number, created_at, created_by, modified_at, modified_by)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?11, ?12)",
                params![
                    eu,
                    k + 1,
                    s.0,
                    scu,
                    cal,
                    invoice,
                    charge,
                    if s.3 { "COMPAÑIA" } else { "INDIVIDUAL" },
                    if g.chance(0.6) { charge } else { 0 },
                    format!("LIC-{}", 20000 + invoice),
                    at,
                    a
                ],
            )?;
        }
    }

    Ok(School {
        students: students.into_iter().map(|(u, n, e, _)| (u, n, e)).collect(),
        first_course,
        seminar_enrollment,
    })
}

struct Convention {
    payment: (i64, String, i64), // (registration number, person name, amount)
    plan: (String, String),      // (plan name, event name)
}

/// A seeded file: its `stored_files` row now, its bytes later (written only when the tool has a data folder).
pub struct SeedFile {
    pub storage_path: String,
    pub bytes: Vec<u8>,
}

/// One-page PDF with a title line: placeholder bytes for the seeded event PDF and billing files.
pub fn placeholder_pdf(title: &str) -> Vec<u8> {
    let text: String = title.chars().filter(|c| c.is_ascii() && *c != '(' && *c != ')' && *c != '\\').collect();
    let stream =
        format!("BT /F1 18 Tf 72 720 Td ({text}) Tj ET\nBT /F1 11 Tf 72 696 Td (MBA demo - datos ficticios) Tj ET");
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_string(),
        format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes(),
    );
    out
}

fn seed_file(
    tx: &Transaction,
    g: &mut Gen,
    files: &mut Vec<SeedFile>,
    name: &str,
    at: &str,
) -> rusqlite::Result<String> {
    let bytes = placeholder_pdf(name.trim_end_matches(".pdf"));
    let sha = crate::db::sha256_hex(&bytes);
    let storage_path = format!("files/{}/{}", &sha[..2], sha);
    let uuid = g.uuid();
    tx.execute(
        "INSERT INTO stored_files (uuid, filename, mime_type, size_bytes, sha256, storage_path, created_at, created_by)
         VALUES (?1, ?2, 'application/pdf', ?3, ?4, ?5, ?6, ?7)",
        params![uuid, name, bytes.len() as i64, sha, storage_path, at, SEED_ACTOR],
    )?;
    files.push(SeedFile { storage_path, bytes });
    Ok(uuid)
}

/// An event item of the seed: (name, type, date offset from the event start, time start, time end, room, price cents).
type ItemDef = (&'static str, &'static str, i64, &'static str, &'static str, &'static str, i64);

/// Convención Demo 2027 as design/convencion.html lists it (12 activities, sort order as shown). Hotel nights keep
/// their check-in date as start and end (BR-EVT-01: an item's end date equals its start date).
const CONV_2027_ITEMS: &[ItemDef] = &[
    ("Hotel Demo Plaza", "Hotel", -1, "16:00", "12:00", "", 28900),
    ("Hotel Demo Plaza", "Hotel", 0, "16:00", "12:00", "", 28900),
    ("Taller de Cumplimiento Afiliado", "Seminario", 0, "08:30", "17:00", "Salón A", 13500),
    ("Taller de Cumplimiento No Afiliado", "Seminario", 0, "08:30", "17:00", "Salón A", 17500),
    ("Coctel de Bienvenida", "Cocktail", 0, "18:00", "20:00", "Terraza Demo", 5500),
    ("Desayuno de Networking", "Desayuno", 1, "07:00", "09:00", "Salón B", 0),
    ("Panel de Tendencias del Mercado", "Foro", 1, "09:00", "12:00", "Gran Salón", 0),
    ("Almuerzo de Reconocimientos", "Almuerzo", 1, "12:30", "15:00", "Gran Salón A, B", 14000),
    ("Hotel Demo Plaza", "Hotel", 1, "16:00", "12:00", "", 28900),
    ("Cena de Gala Aniversario", "Cena", 1, "19:00", "23:00", "Gran Salón", 11000),
    ("Desayuno de Clausura", "Desayuno", 2, "08:00", "10:00", "Terraza Demo", 4500),
    ("Hotel Demo Plaza (2 noches)", "Hotel", 0, "16:00", "12:00", "", 57800),
];

const FORO_ITEMS: &[ItemDef] = &[
    ("Foro de Cumplimiento", "Foro", 0, "09:00", "12:00", "Salón Principal", 0),
    ("Desayuno del Foro", "Desayuno", 0, "07:30", "09:00", "Salón Principal", 3500),
    ("Almuerzo del Foro", "Almuerzo", 0, "12:30", "14:00", "Salón Principal", 6000),
];

const CONV_OLD_ITEMS: &[ItemDef] = &[
    ("Hotel Demo Plaza", "Hotel", 0, "16:00", "12:00", "", 27500),
    ("Hotel Demo Plaza", "Hotel", 1, "16:00", "12:00", "", 27500),
    ("Taller de Cumplimiento", "Seminario", 0, "08:30", "17:00", "Salón A", 13000),
    ("Coctel de Bienvenida", "Cocktail", 0, "18:00", "20:00", "Terraza Demo", 5000),
    ("Almuerzo de Reconocimientos", "Almuerzo", 1, "12:30", "15:00", "Gran Salón", 13500),
    ("Cena de Gala", "Cena", 1, "19:00", "23:00", "Gran Salón", 10500),
    ("Desayuno de Clausura", "Desayuno", 2, "08:00", "10:00", "Terraza Demo", 4500),
];

/// The six events of design/convenciones.html: (name, location, start, end, status, items).
#[allow(clippy::type_complexity)]
const EVENTS: &[(&str, &str, &str, &str, &str, &[ItemDef])] = &[
    ("Convención Demo 2027", "Hotel Demo Plaza", "2027-05-06", "2027-05-08", "Activo", CONV_2027_ITEMS),
    ("Foro de Otoño Demo 2027", "Centro de Convenciones Ejemplo", "2027-10-14", "2027-10-14", "Activo", FORO_ITEMS),
    ("Foro Demo 2026", "Centro de Convenciones Ejemplo", "2026-10-08", "2026-10-08", "Inactivo", FORO_ITEMS),
    ("Convención Demo 2026", "Hotel Demo Plaza", "2026-05-14", "2026-05-16", "Inactivo", CONV_OLD_ITEMS),
    ("Foro Demo 2025", "Centro de Convenciones Ejemplo", "2025-10-09", "2025-10-09", "Inactivo", FORO_ITEMS),
    ("Convención Demo 2025", "Hotel Ejemplo Resort", "2025-05-15", "2025-05-17", "Inactivo", CONV_OLD_ITEMS),
];

/// Plans of design/planes-lista.html: (event index, name, price override cents, active item indexes, qty overrides).
#[allow(clippy::type_complexity)]
const PLANS: &[(usize, &str, Option<i64>, &[usize], &[(usize, i64)])] = &[
    (0, "P1 · Completo", Some(164000), &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10], &[]),
    // 8 of 12 included; Cena and Desayuno de Clausura x2: $1,218.00 (design/planes.html).
    (0, "P2(B) · 15 marzo", None, &[1, 2, 4, 5, 7, 8, 9, 10], &[(9, 2), (10, 2)]),
    (0, "P3 · Solo seminarios", None, &[2, 3], &[]),
    (0, "P4 · Hotel y cena", Some(68800), &[1, 8, 9], &[]),
    (0, "P5 · Afiliado sin hotel", None, &[2, 4, 5, 6, 7, 10], &[]),
    (1, "F1 · Foro completo", None, &[0, 1, 2], &[]),
    (2, "F1 · Foro completo", None, &[0, 1, 2], &[]),
    (3, "P1 · Completo", None, &[0, 1, 2, 3, 4, 5, 6], &[]),
    (3, "P2 · Sin hotel", None, &[2, 3, 4, 5, 6], &[]),
    (4, "F1 · Foro completo", None, &[0, 1, 2], &[]),
    (5, "P1 · Completo", None, &[0, 1, 2, 3, 4, 5, 6], &[]),
];

/// design/convencion.html Auspiciadores: (name, type, title, contact, phone, requested, received, promised).
#[allow(clippy::type_complexity)]
const SPONSORS: &[(&str, &str, &str, &str, &str, i64, i64, i64)] = &[
    ("Acme Mortgage Demo", "Platino", "Sra.", "Ana Ejemplo", "787-555-0112", 1_000_000, 1_000_000, 0),
    ("Banco Ejemplo", "Oro", "Sr.", "Juan Pérez Demo", "787-555-0127", 500_000, 500_000, 0),
    ("Capital Ficticio Lending", "Plata", "Lcda.", "Mengana Prueba", "787-555-0133", 250_000, 250_000, 0),
    ("Bufete Fulano & Mengano", "Bronce", "Lcdo.", "Fulano de Tal", "787-555-0141", 50_000, 50_000, 0),
    ("Cooperativa Muestra", "Plata", "Sra.", "María Ejemplo", "787-555-0148", 250_000, 0, 250_000),
    ("Título Demo Services", "Oro", "Sr.", "Zutano Muestra", "787-555-0156", 500_000, 500_000, 0),
    ("Tasaciones Ejemplo", "Bronce", "Ing.", "Carla Demo", "787-555-0163", 50_000, 0, 0),
    ("Crédito Prueba FCU", "Plata", "Sr.", "Tomás Ejemplo", "787-555-0170", 250_000, 0, 250_000),
    ("Avalúos Demo Partners", "Bronce", "Sra.", "Laura Muestra", "787-555-0178", 50_000, 50_000, 0),
    ("Hipotecas Ficticias, LLC", "Oro", "Sr.", "Pedro Prueba", "787-555-0185", 500_000, 0, 500_000),
    ("Seguros Ejemplo", "Plata", "Sra.", "Beatriz Demo", "787-555-0191", 250_000, 0, 0),
    ("Abogados Demo & Asoc.", "Bronce", "Lcdo.", "Mario Ejemplo", "787-555-0104", 50_000, 50_000, 0),
    ("Financiera Ficticia", "Plata", "Sr.", "Ricardo Muestra", "787-555-0109", 250_000, 0, 250_000),
    ("Préstamos Prueba Inc.", "Bronce", "Sra.", "Paola Demo", "787-555-0119", 50_000, 0, 50_000),
];

/// design/convencionistas.html: (first, last, company, phone, e-mail, city).
const NAMED_PEOPLE: &[(&str, &str, &str, &str, &str, &str)] = &[
    ("María", "Ejemplo Demo", "Cooperativa Muestra", "787-555-0148", "maria@ejemplo.com", "Ciudad Ejemplo"),
    ("Fulano", "de Tal", "Acme Mortgage Demo", "787-555-0141", "fulano@ejemplo.com", "Pueblo Demo"),
    ("Pedro", "Prueba Demo", "Cooperativa Muestra", "787-555-0185", "pedro@ejemplo.com", "Ciudad Ejemplo"),
    ("Elena", "Muestra Ficticia", "Banco Ejemplo", "787-555-0101", "elena@ejemplo.com", "Villa Muestra"),
    ("Luis", "Ejemplo Prueba", "Capital Ficticio Lending", "787-555-0102", "luis@ejemplo.com", "Pueblo Demo"),
    ("Carmen", "Demo Ficticio", "Hipotecas Ficticias, LLC", "787-555-0103", "carmen@ejemplo.com", "Ciudad Ejemplo"),
    ("Rafael", "Prueba Muestra", "Avalúos Demo Partners", "787-555-0105", "rafael@ejemplo.com", "Villa Muestra"),
    ("Sofía", "Ficticia Demo", "Abogados Demo & Asoc.", "787-555-0106", "sofia@ejemplo.com", "Pueblo Demo"),
];

struct SeedReg<'a> {
    number: i64,
    person: usize,
    event: usize,
    plan: Option<&'a str>,
    status: &'a str,
    costeado: &'a str,
    /// Guest names (first, last, badge).
    guests: &'a [(&'a str, &'a str, &'a str)],
    /// Items without a plan: (event item index, hand-set qty or 0 = guests + 1).
    items: &'a [(usize, i64)],
    /// Payments: (date, number, type, cents, void reason).
    payments: &'a [(&'a str, &'a str, &'a str, i64, Option<&'a str>)],
    /// "pay the total" instead of listed payments.
    pay_total: bool,
    comments: Option<&'a str>,
}

#[allow(clippy::type_complexity)]
fn seed_convention(tx: &Transaction, g: &mut Gen, files: &mut Vec<SeedFile>) -> rusqlite::Result<Convention> {
    let a = SEED_ACTOR;
    let at = g.at(0);

    // People: the eight of the design, then synthetic ones (787-555-01xx, @ejemplo.com).
    let mut people: Vec<(String, String)> = Vec::new();
    for (i, (first, last, company, phone, mail, city)) in NAMED_PEOPLE.iter().enumerate() {
        let uuid = g.uuid();
        let maria = i == 0;
        tx.execute(
            "INSERT INTO people (uuid, company, name_first, name_last, phone1, phone2, fax, address, city, state, zip, email,
               name_badge, created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'PR', '00000', ?10, ?11, ?12, ?13, ?12, ?13)",
            params![
                uuid,
                company,
                first,
                last,
                phone,
                maria.then_some("787-555-0149"),
                maria.then_some("787-555-0177"),
                format!("Calle Ficticia {}", 123 + i * 10),
                city,
                mail,
                format!("{first} {}", last.split(' ').next().unwrap_or_default()),
                at,
                a
            ],
        )?;
        people.push((uuid, format!("{first} {last}")));
    }
    for i in 0..52i64 {
        let uuid = g.uuid();
        let stamp = g.stamp();
        let (first, l1, l2) = g.person();
        let company = *g.pick(COMPANIES);
        let phone = g.phone();
        let city = *g.pick(CITIES);
        tx.execute(
            "INSERT INTO people (uuid, company, name_first, name_last, phone1, address, city, state, zip, email, name_badge,
               created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'PR', ?8, ?9, ?10, ?11, ?12, ?11, ?12)",
            params![
                uuid,
                company,
                first,
                format!("{l1} {l2}"),
                phone,
                format!("Calle Muestra {}", 10 + i),
                city,
                format!("00{}", 900 + (i % 80)),
                email(&first, &l1, 100 + i),
                format!("{first} {l1}"),
                stamp,
                a
            ],
        )?;
        people.push((uuid, format!("{first} {l1} {l2}")));
    }

    // Events with their items (BR-EVT-01: item end date = start date; BR-EVT-03: sort order 1..n).
    let mut events: Vec<(String, Vec<(String, &'static str, i64)>)> = Vec::new();
    for (ei, (name, place, start, end, status, items)) in EVENTS.iter().enumerate() {
        let uuid = g.uuid();
        let pdf = if ei == 0 { Some(seed_file(tx, g, files, "programa-convencion-2027.pdf", &at)?) } else { None };
        tx.execute(
            "INSERT INTO events (uuid, name, date_start, date_end, location, status, pdf_file_uuid, created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?8, ?9)",
            params![uuid, name, start, end, place, status, pdf, at, a],
        )?;
        let start_date = chrono::NaiveDate::parse_from_str(start, "%Y-%m-%d").expect("date");
        let mut its = Vec::new();
        for (k, (iname, kind, day, t1, t2, room, price)) in items.iter().enumerate() {
            let iu = g.uuid();
            let d = (start_date + chrono::Duration::days(*day)).format("%Y-%m-%d").to_string();
            tx.execute(
                "INSERT INTO event_items (uuid, event_uuid, name, date_start, date_end, time_start, time_end, location_room, qty,
                   price, type, sort_order, created_at, created_by, modified_at, modified_by)
                 VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?7, 1, ?8, ?9, ?10, ?11, ?12, ?11, ?12)",
                params![iu, uuid, iname, d, t1, t2, (!room.is_empty()).then_some(*room), price, kind, k as i64 + 1, at, a],
            )?;
            its.push((iu, *kind, *price));
        }
        events.push((uuid, its));
    }

    // Plans: every plan item built (OP-EVT-04), the listed ones included.
    let mut plans: Vec<(usize, &str, String, Vec<(String, String, i64)>)> = Vec::new(); // (event, name, uuid, active items)
    for (ei, name, over, active, qtys) in PLANS {
        let pu = g.uuid();
        tx.execute(
            "INSERT INTO packages (uuid, event_uuid, name, price_override, created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?5, ?6)",
            params![pu, events[*ei].0, name, over, at, a],
        )?;
        let mut act = Vec::new();
        for (k, (iu, _, price)) in events[*ei].1.iter().enumerate() {
            let on = active.contains(&k);
            let q = qtys.iter().find(|(i, _)| *i == k).map(|(_, q)| *q).unwrap_or(1);
            let piu = g.uuid();
            tx.execute(
                "INSERT INTO package_items (uuid, package_uuid, event_item_uuid, active, price, ticket_qty, created_at, created_by, modified_at, modified_by)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?7, ?8)",
                params![piu, pu, iu, on as i64, price, q, at, a],
            )?;
            if on {
                act.push((piu, iu.clone(), *price));
            }
        }
        plans.push((*ei, name, pu, act));
    }

    // Sponsors of Convención Demo 2027 (design), and a few for 2026.
    for (k, (name, kind, title, contact, phone, req, rec, prom)) in SPONSORS.iter().enumerate() {
        let su = g.uuid();
        let last = contact.split(' ').next_back().unwrap_or_default();
        let greet = match *title {
            "Sra." | "Lcda." => format!("Estimada señora {last}:"),
            _ => format!("Estimado señor {last}:"),
        };
        let (address, city) =
            if k == 0 { ("Avenida Demo 400, Suite 2", "Pueblo Demo") } else { ("Calle Ejemplo 1", "Ciudad Ejemplo") };
        tx.execute(
            "INSERT INTO sponsor (uuid, sponsor_name, requested_amount, amount_received, promised, address, phone, greeting, title,
               contact_name, type, city, zip, state, event_uuid, created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, '00000', 'PR', ?13, ?14, ?15, ?14, ?15)",
            params![
                su,
                name,
                req,
                (*rec > 0).then_some(*rec),
                (*prom > 0).then_some(*prom),
                address,
                phone,
                greet,
                title,
                contact,
                kind,
                city,
                events[0].0,
                at,
                a
            ],
        )?;
    }
    for (name, kind, amount) in [("Banco Ejemplo", "Oro", 500_000i64), ("Acme Mortgage Demo", "Platino", 1_000_000)] {
        let su = g.uuid();
        tx.execute(
            "INSERT INTO sponsor (uuid, sponsor_name, requested_amount, amount_received, type, event_uuid, created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?3, ?4, ?5, ?6, ?7, ?6, ?7)",
            params![su, name, amount, kind, events[3].0, at, a],
        )?;
    }

    // Billing files of Convención Demo 2027 (design Facturación tab, D-16).
    for (date, time) in [
        ("2027-05-04", "16:42:18"),
        ("2027-05-03", "09:15:02"),
        ("2027-04-26", "11:07:45"),
        ("2027-04-19", "15:28:51"),
        ("2027-04-12", "10:02:33"),
    ] {
        let ts = format!("{date} {time}");
        let fu = seed_file(tx, g, files, &format!("Lista_Facturacion_{date}.pdf"), &ts)?;
        let bu = g.uuid();
        tx.execute(
            "INSERT INTO billing (uuid, event_uuid, timestamp, file_uuid, created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?3, ?5, ?3, ?5)",
            params![bu, events[0].0, ts, fu, a],
        )?;
    }

    // Registrations #1283..#1290 of the design (registros.html, registro.html, registro-puerta.html).
    let named: &[SeedReg] = &[
        SeedReg {
            number: 1283,
            person: 0,
            event: 0,
            plan: Some("P2(B) · 15 marzo"),
            status: "Nuevo",
            costeado: "Afiliado",
            guests: &[("Pedro", "Prueba Demo", "Pedro Prueba")],
            items: &[],
            payments: &[
                ("2027-04-02 10:00:00", "DEMO-01", "Cheque", 50_000, None),
                ("2027-04-10 15:30:00", "DEMO-02", "VISA", 25_000, Some("Cargo duplicado en la tarjeta")),
            ],
            pay_total: false,
            comments: Some("Texto de ejemplo para la demostración. Datos ficticios."),
        },
        SeedReg {
            number: 1284,
            person: 2,
            event: 0,
            plan: None,
            status: "Registrado",
            costeado: "Individuo",
            guests: &[],
            items: &[(9, 2)], // Cena de Gala x2, qty set by hand (D-88): $220.00
            payments: &[],
            pay_total: true,
            comments: None,
        },
        SeedReg {
            number: 1285,
            person: 3,
            event: 0,
            plan: None,
            status: "Registrado",
            costeado: "Afiliado",
            guests: &[("Ana", "Muestra", "Ana Muestra"), ("Jorge", "Ficticio", "Jorge Ficticio")],
            items: &[(1, 1), (9, 0)],
            payments: &[],
            pay_total: true,
            comments: None,
        },
        SeedReg {
            number: 1286,
            person: 7,
            event: 0,
            plan: None,
            status: "No Canceló",
            costeado: "Individuo",
            guests: &[],
            items: &[(11, 0), (3, 0), (4, 0), (5, 0)],
            payments: &[],
            pay_total: false,
            comments: None,
        },
        SeedReg {
            number: 1287,
            person: 6,
            event: 0,
            plan: None,
            status: "Canceló",
            costeado: "Individuo",
            guests: &[],
            items: &[(2, 0), (7, 0)],
            payments: &[],
            pay_total: false,
            comments: Some("Canceló por teléfono."),
        },
        SeedReg {
            number: 1288,
            person: 5,
            event: 0,
            plan: Some("P1 · Completo"),
            status: "Asistió",
            costeado: "Afiliado",
            guests: &[("Ramón", "Demo", "Ramón Demo")],
            items: &[],
            payments: &[],
            pay_total: true,
            comments: None,
        },
        SeedReg {
            number: 1289,
            person: 4,
            event: 0,
            plan: Some("P3 · Solo seminarios"),
            status: "Registrado",
            costeado: "Afiliado",
            guests: &[],
            items: &[],
            payments: &[],
            pay_total: true,
            comments: None,
        },
        SeedReg {
            number: 1290,
            person: 1,
            event: 0,
            plan: Some("P5 · Afiliado sin hotel"),
            status: "Nuevo",
            costeado: "Afiliado",
            guests: &[],
            items: &[],
            payments: &[],
            pay_total: false,
            comments: None,
        },
    ];
    let mut invoice = 30_000i64;
    let mut payment = (0, String::new(), 0);
    for r in named {
        let paid = insert_registration(tx, g, &people, &events, &plans, r, &mut invoice, &at)?;
        if r.number == 1283 {
            payment = (r.number, people[r.person].1.clone(), paid);
        }
    }

    // Synthetic registrations: #1201..#1282 for 2027 and the Foro de Otoño, older numbers for past events.
    let mut number = 1200i64;
    let mut old_number = 1000i64;
    for _ in 0..110 {
        let ei = *g.pick(&[0usize, 0, 0, 0, 0, 1, 2, 3, 3, 4, 5]);
        let n = if ei <= 1 && number < 1282 {
            number += 1;
            number
        } else {
            old_number += 1;
            old_number
        };
        let person = g.range(8, people.len() as i64 - 1) as usize;
        let event_plans: Vec<&str> = plans.iter().filter(|p| p.0 == ei).map(|p| p.1).collect();
        let plan = if g.chance(0.6) && !event_plans.is_empty() { Some(*g.pick(&event_plans)) } else { None };
        let status = if EVENTS[ei].4 == "Inactivo" {
            *g.pick(&["Asistió", "Asistió", "Asistió", "No Canceló", "Canceló"])
        } else {
            *g.pick(&["Registrado", "Registrado", "Registrado", "Nuevo", "Canceló"])
        };
        let nguests = *g.pick(&[0usize, 0, 0, 1, 1, 2]);
        let guest_names: Vec<(String, String, String)> = (0..nguests)
            .map(|_| {
                let (f, l, _) = g.person();
                let badge = format!("{f} {l}");
                (f, l, badge)
            })
            .collect();
        let guests: Vec<(&str, &str, &str)> =
            guest_names.iter().map(|(f, l, b)| (f.as_str(), l.as_str(), b.as_str())).collect();
        let nitems = events[ei].1.len();
        let mut idx: Vec<(usize, i64)> = Vec::new();
        if plan.is_none() {
            let want = g.range(1, nitems.min(4) as i64) as usize;
            while idx.len() < want {
                let k = g.range(0, nitems as i64 - 1) as usize;
                if !idx.iter().any(|(i, _)| *i == k) {
                    idx.push((k, 0));
                }
            }
        }
        let pay_total = status != "Canceló" && status != "Nuevo" && g.chance(0.75);
        let r = SeedReg {
            number: n,
            person,
            event: ei,
            plan,
            status,
            costeado: g.pick(&["Afiliado", "Individuo"]),
            guests: &guests,
            items: &idx,
            payments: &[],
            pay_total,
            comments: None,
        };
        insert_registration(tx, g, &people, &events, &plans, &r, &mut invoice, &at)?;
    }

    Ok(Convention { payment, plan: ("P2(B) · 15 marzo".into(), "Convención Demo 2027".into()) })
}

/// One registration with its guests, items (plan items or hand-picked ones) and payments. Returns the cents paid
/// (voided payments excluded).
#[allow(clippy::too_many_arguments)]
#[allow(clippy::type_complexity)]
fn insert_registration(
    tx: &Transaction,
    g: &mut Gen,
    people: &[(String, String)],
    events: &[(String, Vec<(String, &'static str, i64)>)],
    plans: &[(usize, &str, String, Vec<(String, String, i64)>)],
    r: &SeedReg,
    invoice: &mut i64,
    at: &str,
) -> rusqlite::Result<i64> {
    let a = SEED_ACTOR;
    let ru = g.uuid();
    let stamp = if r.number >= 1283 { at.to_owned() } else { g.stamp() };
    let plan = r.plan.and_then(|name| plans.iter().find(|p| p.0 == r.event && p.1 == name));
    tx.execute(
        "INSERT INTO registration (uuid, person_uuid, event_uuid, package_uuid, registration_number, status, costeado_por,
           comments, created_at, created_by, modified_at, modified_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?9, ?10)",
        params![
            ru,
            people[r.person].0,
            events[r.event].0,
            plan.map(|p| p.2.clone()),
            r.number,
            r.status,
            r.costeado,
            r.comments,
            stamp,
            a
        ],
    )?;
    for (f, l, b) in r.guests {
        let gu = g.uuid();
        tx.execute(
            "INSERT INTO registration_guests (uuid, registration_uuid, name_first, name_last, name_badge, created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?6, ?7)",
            params![gu, ru, f, l, b, stamp, a],
        )?;
    }
    let qty = r.guests.len() as i64 + 1;
    // (package item, event item, price, qty, qty_manual)
    let mut lines: Vec<(Option<String>, String, i64, i64, i64)> = Vec::new();
    if let Some(p) = plan {
        for (piu, iu, price) in &p.3 {
            lines.push((Some(piu.clone()), iu.clone(), *price, qty, 0));
        }
    } else {
        for (k, hand) in r.items {
            let (iu, _, price) = &events[r.event].1[*k];
            lines.push((None, iu.clone(), *price, if *hand > 0 { *hand } else { qty }, (*hand > 0) as i64));
        }
    }
    let mut table = 3;
    for (piu, iu, price, q, manual) in lines {
        *invoice += 1;
        let kind = events[r.event].1.iter().find(|(u, _, _)| *u == iu).map(|(_, k, _)| *k).unwrap_or("Foro");
        let (reserva, mesa) = match kind {
            "Hotel" => (Some(format!("DEMO-{:02}", r.number % 100)), None),
            k if crate::convention::events::is_seating(k) && price > 0 => {
                table += 3;
                (None, Some(table.to_string()))
            }
            _ => (None, None),
        };
        let lu = g.uuid();
        tx.execute(
            "INSERT INTO registration_items (uuid, registration_uuid, package_item_uuid, event_item_uuid, price, ticket_qty,
               qty_manual, hotel_reservation_num, mesa, invoice_number, created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?11, ?12)",
            params![lu, ru, piu, iu, price, q, manual, reserva, mesa, *invoice, stamp, a],
        )?;
    }
    let mut paid = 0;
    for (date, num, kind, cents, void) in r.payments {
        let pu = g.uuid();
        tx.execute(
            "INSERT INTO payments (uuid, registration_uuid, payment, payment_type, payment_number, voided_at, voided_by, void_reason,
               created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?9, ?10)",
            params![
                pu,
                ru,
                cents,
                kind,
                num,
                void.map(|_| *date),
                void.map(|_| "mvega"),
                void,
                date,
                a
            ],
        )?;
        if void.is_none() {
            paid += cents;
        }
    }
    if r.pay_total {
        let total: i64 =
            tx.query_row("SELECT total FROM v_registration_totals WHERE registration_uuid = ?1", [&ru], |row| {
                row.get(0)
            })?;
        if total > 0 {
            let pu = g.uuid();
            let kind = *g.pick(&["VISA", "MC", "AMEX", "Efectivo", "Cheque"]);
            let num = match kind {
                "Efectivo" => None,
                "Cheque" => Some(format!("{}", g.range(1000, 9999))),
                _ => Some(format!("AUT-{}", g.range(100000, 999999))),
            };
            tx.execute(
                "INSERT INTO payments (uuid, registration_uuid, payment, payment_type, payment_number, created_at, created_by, modified_at, modified_by)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?6, ?7)",
                params![pu, ru, total, kind, num, stamp, a],
            )?;
            paid += total;
        }
    }
    Ok(paid)
}

struct Mail {
    sent_batch: (i64, String, i64),       // (serial, title, sent)
    error_batch: (i64, String, i64, i64), // (serial, title, sent, errors)
}

fn seed_mail(tx: &Transaction, g: &mut Gen, students: &[(String, String, String)]) -> rusqlite::Result<Mail> {
    let a = SEED_ACTOR;
    let at = g.at(0);
    let profiles = [
        ("escuela@ejemplo.com", "Oficina de la Escuela", "Coordinación académica", 1),
        ("convencion@ejemplo.com", "Comité de la Convención", "Coordinación de eventos", 0),
    ];
    for (mail, name, title, def) in profiles {
        tx.execute(
            "INSERT INTO mail_profiles (email, name, title, is_default, created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?5, ?6)",
            params![mail, name, title, def, at, a],
        )?;
    }
    let mut templates = Vec::new();
    for (subject, html) in [
        ("Aviso de la Escuela", "<p>Saludos <SCI_NAME>:</p><SCI_BODY><p><SCI_USER><br><SCI_TITLE></p>"),
        (
            "Recordatorio de curso",
            "<p>Hola <SCI_NAME>:</p><p>Le recordamos su próximo curso.</p><SCI_BODY><p><SCI_USER></p>",
        ),
    ] {
        let tu = g.uuid();
        tx.execute(
            "INSERT INTO mail_templates (uuid, subject, html_template, created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?4, ?5)",
            params![tu, subject, html, at, a],
        )?;
        templates.push((tu, subject, html));
    }
    tx.execute(
        "UPDATE app_settings SET value = ?1, modified_at = ?2, modified_by = ?3 WHERE key = 'mail.default_template_uuid'",
        params![templates[0].0, at, a],
    )?;

    // (serial, title, state): draft, all sent, sent with one error.
    let batches = [
        (44, "Bienvenida · semestre de febrero", "draft"),
        (45, "Cursos abiertos · febrero 2027", "draft"),
        (46, "Recordatorio de pago", "draft"),
        (47, "Seminario de Ética · abril", "sent"),
        (48, "Taller de Cumplimiento · mayo", "error"),
    ];
    let mut out = Mail { sent_batch: (0, String::new(), 0), error_batch: (0, String::new(), 0, 0) };
    for (serial, title, state) in batches {
        let bu = g.uuid();
        let t = &templates[(serial % 2) as usize];
        let bat = g.stamp();
        tx.execute(
            "INSERT INTO mail_batches (uuid, template_uuid, subject, body, body_entry, serial, title, from_email,
               created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'escuela@ejemplo.com', ?8, ?9, ?8, ?9)",
            params![bu, t.0, t.1, t.2, format!("<p>{title}.</p>"), serial, title, bat, a],
        )?;
        let n = g.range(8, 16) as usize;
        let mut picked: Vec<usize> = Vec::new();
        while picked.len() < n {
            let s = g.range(0, students.len() as i64 - 1) as usize;
            if !picked.contains(&s) {
                picked.push(s);
            }
        }
        let (mut sent, mut errors) = (0, 0);
        for (k, s) in picked.into_iter().enumerate() {
            let (su, name, mail) = &students[s];
            let eu = g.uuid();
            let (is_sent, is_err) = match state {
                "sent" => (true, false),
                "error" => (k != 0, k == 0),
                _ => (false, false),
            };
            let sent_on = if is_sent { Some(bat.clone()) } else { None };
            tx.execute(
                "INSERT INTO mail_emails (uuid, batch_uuid, student_uuid, name, to_addrs, sent, sent_on, error, error_log,
                   created_at, created_by, modified_at, modified_by)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?10, ?11)",
                params![
                    eu,
                    bu,
                    su,
                    name,
                    mail,
                    is_sent as i64,
                    sent_on,
                    is_err as i64,
                    if is_err { Some("550 buzón no existe (simulado)") } else { None },
                    bat,
                    a
                ],
            )?;
            if is_sent || is_err {
                tx.execute(
                    "INSERT INTO mail_outbox (email_uuid, provider, from_addr, to_addrs, subject, html, status, error, created_at)
                     VALUES (?1, 'mock', 'escuela@ejemplo.com', ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        eu,
                        mail,
                        title,
                        format!("<p>Saludos {name}:</p><p>{title}.</p>"),
                        if is_err { "failed" } else { "sent" },
                        if is_err { Some("550 buzón no existe (simulado)") } else { None },
                        bat
                    ],
                )?;
            }
            sent += is_sent as i64;
            errors += is_err as i64;
        }
        match state {
            "sent" => out.sent_batch = (serial, title.to_owned(), sent),
            "error" => out.error_batch = (serial, title.to_owned(), sent, errors),
            _ => {}
        }
    }
    Ok(out)
}

/// Placeholder signers (D-18) and the recent-activity rows of UI-02, at fixed times on the seed's last day.
fn seed_settings_and_activity(
    tx: &Transaction,
    g: &Gen,
    school: &School,
    convention: &Convention,
    mail: &Mail,
) -> rusqlite::Result<()> {
    let a = SEED_ACTOR;
    let at = g.at(0);
    for (key, value) in [
        ("school.signer_name", "Dra. Laura Ejemplo Ruiz"),
        ("school.signer_title", "Directora de la Escuela"),
        ("convention.signer_name", "Lcdo. Carlos Muestra Díaz"),
        ("convention.signer_title", "Presidente del Comité de la Convención"),
    ] {
        tx.execute(
            "UPDATE app_settings SET value = ?1, modified_at = ?2, modified_by = ?3 WHERE key = ?4",
            params![value, at, a, key],
        )?;
    }
    // Settings rows from migration 0001 carry the time the migration ran; pin them so the dump does not depend on it.
    tx.execute("UPDATE app_settings SET modified_at = ?1 WHERE modified_by IS NULL", [&at])?;
    // D-18 document templates for UI-30 (texts are synthetic).
    crate::admin::settings::seed_document_templates(tx, &at, a)?;
    let day = 19 * 24 * 60; // 2026-11-21
    let rows = [
        (
            day + 101,
            "mail_batches",
            "mail.send_batch",
            format!("Envío #{} «{}»: {} enviados", mail.sent_batch.0, mail.sent_batch.1, mail.sent_batch.2),
        ),
        (
            day + 185,
            "packages",
            "field.change",
            format!("Plan {} actualizado en {}", convention.plan.0, convention.plan.1),
        ),
        (
            day + 380,
            "calendar_course",
            "calendar.open",
            format!("Curso abierto: {}, {}", school.first_course.0, school.first_course.1),
        ),
        (
            day + 475,
            "enrollments_seminars",
            "enrollment.create",
            format!("Matrícula: {} en {}", school.seminar_enrollment.0, school.seminar_enrollment.1),
        ),
        (
            day + 492,
            "payments",
            "payment.create",
            format!(
                "Registro #{} · {} · pago de {}",
                convention.payment.0,
                convention.payment.1,
                usd(convention.payment.2)
            ),
        ),
        (
            day + 518,
            "mail_batches",
            "mail.send_batch",
            format!(
                "Envío #{} «{}»: {} enviados {} error",
                mail.error_batch.0, mail.error_batch.1, mail.error_batch.2, mail.error_batch.3
            ),
        ),
    ];
    for (minute, entity, action, text) in rows {
        tx.execute(
            "INSERT INTO audit_log (at, user_id, action, entity, entity_uuid, detail) VALUES (?1, 1, ?2, ?3, NULL, ?4)",
            params![g.at(minute), action, entity, serde_json::json!({ "text": text }).to_string()],
        )?;
    }
    Ok(())
}

/// Every row of every table, as text, in key order: two databases with the same dump hold the same data (T-03-02).
pub fn dump(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut tables: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name <> 'schema_migrations' ORDER BY name",
        )?;
        let rows = stmt.query_map([], |r| r.get(0))?.collect::<Result<Vec<String>, _>>()?;
        rows
    };
    tables.sort();
    let mut out = Vec::new();
    for t in tables {
        let mut stmt = conn.prepare(&format!("SELECT * FROM \"{t}\" ORDER BY 1"))?;
        let n = stmt.column_count();
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            let mut line = format!("{t}:");
            for i in 0..n {
                let v: rusqlite::types::Value = r.get(i)?;
                line.push_str(&format!("{v:?}|"));
            }
            out.push(line);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded(seed: u64) -> Connection {
        let mut conn = db::open_in_memory().unwrap();
        run(&mut conn, seed).unwrap();
        conn
    }

    /// T-03-02 (same seed, same data) and T-03-03 (integrity_check, foreign_key_check).
    #[test]
    fn seed_is_deterministic_and_consistent() {
        let a = seeded(DEFAULT_SEED);
        let b = seeded(DEFAULT_SEED);
        assert_eq!(dump(&a).unwrap(), dump(&b).unwrap());
        assert_ne!(dump(&a).unwrap(), dump(&seeded(7)).unwrap(), "another seed gives other data");
        db::integrity_check(&a).unwrap();
        db::foreign_key_check(&a).unwrap();
    }

    #[test]
    fn seed_users_and_no_secrets() {
        let conn = seeded(DEFAULT_SEED);
        let roles: Vec<(String, String)> = conn
            .prepare("SELECT username, role FROM users ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(roles[0], ("mvega".into(), "admin".into()));
        assert_eq!(roles[1], ("inieves".into(), "admin".into()));
        assert_eq!(roles[2], ("equintana".into(), "staff".into()));
        let hash: String =
            conn.query_row("SELECT password_hash FROM users WHERE username = 'mvega'", [], |r| r.get(0)).unwrap();
        assert!(password::verify(DEMO_PASSWORD, &hash));
        // Every e-mail address in the data is @ejemplo.com.
        for line in dump(&conn).unwrap() {
            for word in line.split(['"', '|', ' ', ',']) {
                if word.contains('@') && !word.starts_with('<') {
                    assert!(word.ends_with("@ejemplo.com"), "{word} in {line}");
                }
            }
            let low = line.to_lowercase();
            assert!(!low.contains("smtp") && !low.contains("licen"), "{line}");
        }
    }

    #[test]
    fn seed_refuses_a_seeded_database() {
        let mut conn = seeded(1);
        assert!(run(&mut conn, 1).is_err());
    }

    #[test]
    fn usd_format() {
        assert_eq!(usd(50000), "$500.00");
        assert_eq!(usd(123456789), "$1,234,567.89");
        assert_eq!(usd(5), "$0.05");
    }
}
