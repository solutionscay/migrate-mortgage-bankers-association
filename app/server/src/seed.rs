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
        format!("787-555-{:04}", self.range(100, 9999))
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
    let convention = seed_convention(&tx, &mut g)?;
    tx.commit()?;

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

fn seed_convention(tx: &Transaction, g: &mut Gen) -> rusqlite::Result<Convention> {
    let a = SEED_ACTOR;
    // People.
    let mut people = Vec::new();
    for i in 0..60i64 {
        let uuid = g.uuid();
        let at = g.stamp();
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
                at,
                a
            ],
        )?;
        people.push((uuid, format!("{first} {l1} {l2}")));
    }

    // Events with items and plans.
    type Plan = (String, String, Vec<(String, String, i64, i64)>); // (uuid, name, package items)
    struct Ev {
        uuid: String,
        name: &'static str,
        status: &'static str,
        items: Vec<(String, &'static str, i64, i64)>, // (uuid, type, price, qty)
        plans: Vec<Plan>,
    }
    let defs: [(&str, &str, &str, &str, &str); 3] = [
        ("Convención Demo 2027", "2027-03-12", "2027-03-14", "Hotel Ejemplo, San Juan", "Activo"),
        ("Foro de Cumplimiento 2026", "2026-12-04", "2026-12-04", "Centro de Convenciones Muestra", "Activo"),
        ("Convención Demo 2026", "2026-03-13", "2026-03-15", "Hotel Ejemplo, San Juan", "Inactivo"),
    ];
    let item_defs: &[(&str, &str, i64, i64, i64, &str)] = &[
        // (name, type, price cents, qty, day offset, start time)
        ("Habitación de hotel (2 noches)", "Hotel", 37800, 1, 0, "15:00"),
        ("Seminario general", "Seminario", 15000, 1, 0, "09:00"),
        ("Foro de cumplimiento", "Foro", 12500, 1, 1, "10:00"),
        ("Desayuno de bienvenida", "Desayuno", 3500, 1, 1, "07:30"),
        ("Almuerzo de premiación", "Almuerzo", 6500, 1, 1, "12:30"),
        ("Cena de gala", "Cena", 9500, 1, 2, "19:00"),
        ("Cocktail de clausura", "Cocktail", 4500, 1, 2, "17:30"),
    ];
    let mut events = Vec::new();
    for (ei, (name, start, end, place, status)) in defs.iter().enumerate() {
        let uuid = g.uuid();
        let at = g.stamp();
        tx.execute(
            "INSERT INTO events (uuid, name, date_start, date_end, location, status, created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?7, ?8)",
            params![uuid, name, start, end, place, status, at, a],
        )?;
        let start_date = chrono::NaiveDate::parse_from_str(start, "%Y-%m-%d").expect("date");
        let chosen: Vec<usize> = if ei == 1 { vec![2, 3, 4] } else { (0..item_defs.len()).collect() };
        let mut items = Vec::new();
        for (k, &ix) in chosen.iter().enumerate() {
            let (iname, kind, price, qty, day, time) = item_defs[ix];
            let iu = g.uuid();
            let d = (start_date + chrono::Duration::days(if ei == 1 { 0 } else { day })).format("%Y-%m-%d").to_string();
            tx.execute(
                "INSERT INTO event_items (uuid, event_uuid, name, date_start, date_end, time_start, location_room, qty, price, type,
                   sort_order, created_at, created_by, modified_at, modified_by)
                 VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?11, ?12)",
                params![iu, uuid, iname, d, time, format!("Salón {}", k + 1), qty, price, kind, k as i64 + 1, at, a],
            )?;
            items.push((iu, kind, price, qty));
        }
        let plan_defs: Vec<(&str, Vec<&str>, Option<i64>)> = if ei == 1 {
            vec![("Foro completo", vec!["Foro", "Desayuno", "Almuerzo"], Some(19900))]
        } else {
            vec![
                (
                    "P1(A) · completo con hotel",
                    vec!["Hotel", "Seminario", "Foro", "Desayuno", "Almuerzo", "Cena", "Cocktail"],
                    None,
                ),
                ("P2(B) · sin hotel", vec!["Seminario", "Foro", "Almuerzo", "Cena"], None),
            ]
        };
        let mut plans = Vec::new();
        for (pname, kinds, over) in plan_defs {
            let pu = g.uuid();
            tx.execute(
                "INSERT INTO packages (uuid, event_uuid, name, price_override, created_at, created_by, modified_at, modified_by)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?5, ?6)",
                params![pu, uuid, pname, over, at, a],
            )?;
            let mut pitems = Vec::new();
            for (iu, kind, price, qty) in &items {
                let active = kinds.contains(kind);
                let piu = g.uuid();
                tx.execute(
                    "INSERT INTO package_items (uuid, package_uuid, event_item_uuid, active, price, ticket_qty, created_at, created_by, modified_at, modified_by)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?7, ?8)",
                    params![piu, pu, iu, active as i64, price, qty, at, a],
                )?;
                if active {
                    pitems.push((piu, iu.clone(), *price, *qty));
                }
            }
            plans.push((pu, pname.to_owned(), pitems));
        }
        events.push(Ev { uuid, name, status, items, plans });
    }

    // Sponsors.
    for (ei, n) in [(0usize, 4), (2, 2)] {
        for k in 0..n {
            let su = g.uuid();
            let at = g.stamp();
            let kind = ["Platino", "Oro", "Plata", "Bronce"][k % 4];
            let promised = [500000, 250000, 100000, 50000][k % 4];
            let (first, l1, _) = g.person();
            let phone = g.phone();
            tx.execute(
                "INSERT INTO sponsor (uuid, sponsor_name, requested_amount, amount_received, promised, phone, greeting, title,
                   contact_name, type, city, state, event_uuid, created_at, created_by, modified_at, modified_by)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'Gerente', ?8, ?9, ?10, 'PR', ?11, ?12, ?13, ?12, ?13)",
                params![
                    su,
                    COMPANIES[(k * 3 + ei) % COMPANIES.len()],
                    promised,
                    if ei == 2 { promised } else { promised / 2 },
                    promised,
                    phone,
                    format!("Estimado(a) {first}:"),
                    format!("{first} {l1}"),
                    kind,
                    *g.pick(CITIES),
                    events[ei].uuid,
                    at,
                    a
                ],
            )?;
        }
    }

    // Registrations with guests, items and payments.
    let mut number = 1200i64;
    let mut invoice = 30000i64;
    let mut payment = (0, String::new(), 0);
    for _ in 0..72 {
        let ei = *g.pick(&[0usize, 0, 0, 0, 1, 1, 2, 2]);
        let ev = &events[ei];
        let (pu_person, pname) = g.pick(&people).clone();
        number += 1;
        let ru = g.uuid();
        let at = g.stamp();
        let plan = if g.chance(0.6) { Some(g.pick(&ev.plans).clone()) } else { None };
        let status = if ev.status == "Inactivo" {
            *g.pick(&["Asistió", "Asistió", "No Canceló"])
        } else {
            *g.pick(&["Registrado", "Registrado", "Registrado", "Nuevo", "Canceló"])
        };
        tx.execute(
            "INSERT INTO registration (uuid, person_uuid, event_uuid, package_uuid, registration_number, status, costeado_por,
               created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?8, ?9)",
            params![ru, pu_person, ev.uuid, plan.as_ref().map(|p| p.0.clone()), number, status, *g.pick(&["Afiliado", "Individuo"]), at, a],
        )?;
        let guests = *g.pick(&[0i64, 0, 0, 1, 1, 2]);
        for _ in 0..guests {
            let gu = g.uuid();
            let (first, l1, _) = g.person();
            tx.execute(
                "INSERT INTO registration_guests (uuid, registration_uuid, name_first, name_last, name_badge, created_at, created_by, modified_at, modified_by)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?6, ?7)",
                params![gu, ru, first, l1, format!("{first} {l1}"), at, a],
            )?;
        }
        let qty = guests + 1;
        let mut lines: Vec<(Option<String>, String, &str, i64)> = Vec::new();
        if let Some((_, _, pitems)) = &plan {
            for (piu, iu, price, _) in pitems {
                let kind = ev.items.iter().find(|i| &i.0 == iu).map(|i| i.1).unwrap_or("Foro");
                lines.push((Some(piu.clone()), iu.clone(), kind, *price));
            }
        } else {
            let n = g.range(2, ev.items.len().min(4) as i64) as usize;
            let mut idx: Vec<usize> = Vec::new();
            while idx.len() < n {
                let k = g.range(0, ev.items.len() as i64 - 1) as usize;
                if !idx.contains(&k) {
                    idx.push(k);
                }
            }
            for k in idx {
                let (iu, kind, price, _) = &ev.items[k];
                lines.push((None, iu.clone(), kind, *price));
            }
        }
        for (piu, iu, kind, price) in lines {
            invoice += 1;
            let lu = g.uuid();
            let hotel = kind == "Hotel";
            let seating = matches!(kind, "Cena" | "Almuerzo" | "Seminario" | "Desayuno");
            let mesa = if seating { Some(g.range(1, 30).to_string()) } else { None };
            tx.execute(
                "INSERT INTO registration_items (uuid, registration_uuid, package_item_uuid, event_item_uuid, price, ticket_qty,
                   hotel_reservation_num, mesa, invoice_number, created_at, created_by, modified_at, modified_by)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?10, ?11)",
                params![
                    lu,
                    ru,
                    piu,
                    iu,
                    price,
                    qty,
                    if hotel { Some(format!("H-{}", 40000 + invoice)) } else { None },
                    mesa,
                    invoice,
                    at,
                    a
                ],
            )?;
        }
        if status == "Canceló" || status == "Nuevo" {
            continue;
        }
        let total: i64 =
            tx.query_row("SELECT total FROM v_registration_totals WHERE registration_uuid = ?1", [&ru], |r| r.get(0))?;
        let amount = match g.range(0, 4) {
            0 => 0,
            1 => (total / 2 / 100) * 100,
            _ => total,
        };
        if amount > 0 {
            let pu = g.uuid();
            let kind = *g.pick(&["VISA", "MC", "AMEX", "Efectivo", "Cheque"]);
            let num = match kind {
                "Cheque" => Some(format!("{}", g.range(1000, 9999))),
                "Efectivo" => None,
                _ => Some(format!("AUT-{}", g.range(100000, 999999))),
            };
            let pat = g.stamp();
            tx.execute(
                "INSERT INTO payments (uuid, registration_uuid, payment, payment_type, payment_number, created_at, created_by, modified_at, modified_by)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?6, ?7)",
                params![pu, ru, amount, kind, num, pat, a],
            )?;
            if ei == 0 && payment.0 == 0 {
                payment = (number, pname.clone(), amount);
            }
        }
    }
    // One voided payment (D-09, BR-REG-09), so the void state has a row.
    let (vreg, vat): (String, String) = tx.query_row(
        "SELECT registration_uuid, created_at FROM payments ORDER BY created_at, uuid LIMIT 1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let vu = g.uuid();
    tx.execute(
        "INSERT INTO payments (uuid, registration_uuid, payment, payment_type, payment_number, voided_at, voided_by, void_reason,
           created_at, created_by, modified_at, modified_by)
         VALUES (?1, ?2, 5000, 'Cheque', '1001', ?3, 'mvega', 'Cheque devuelto por el banco', ?3, ?4, ?3, 'mvega')",
        params![vu, vreg, vat, a],
    )?;

    let p = &events[0].plans[1];
    Ok(Convention { payment, plan: (p.1.clone(), events[0].name.to_owned()) })
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
