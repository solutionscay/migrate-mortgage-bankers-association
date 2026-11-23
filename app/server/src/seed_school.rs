//! The school part of `mba-seed`: the synthetic data of the M1 design pages (estudiante.html, estudiantes.html,
//! afiliados.html, afiliado.html, ofrecimiento.html, matriculas.html, importar-notas.html, admin-certificaciones.html).
//! Deterministic (all picks from the seed generator), e-mails `@ejemplo.com`, phones `(787) 555-01xx` for the
//! synthetic rows, no SSN (D-06: there is no column).

use super::{ascii, Gen, School, CITIES, FIRST, LAST, SEED_ACTOR};
use rusqlite::{params, Transaction};

/// design/afiliados.html: (account, name, business, membership, city, phone, status).
const AFFILIATES: &[(i64, &str, &str, &str, &str, &str, &str)] = &[
    (1001, "Banco Ejemplo", "Banco comercial", "REGULAR", "San Juan", "(787) 555-0300", "ACTIVO"),
    (1002, "Acme Mortgage Demo", "Compañía hipotecaria", "REGULAR", "Guaynabo", "(787) 555-0311", "ACTIVO"),
    (1003, "Cooperativa Muestra", "Cooperativa", "ASOCIADO", "Caguas", "(787) 555-0322", "ACTIVO"),
    (1004, "Capital Ficticio Lending", "Compañía hipotecaria", "REGULAR", "Bayamón", "(787) 555-0333", "INACTIVO"),
    (1005, "Hipotecaria Modelo PR", "Compañía hipotecaria", "REGULAR", "Ponce", "(787) 555-0344", "ACTIVO"),
    (1006, "Avalúos Demo Partners", "Tasación", "ASOCIADO", "Mayagüez", "(787) 555-0355", "ACTIVO"),
    (1007, "Título Seguro Ejemplo", "Seguro de título", "ASOCIADO", "Carolina", "(787) 555-0366", "INACTIVO"),
];

/// design/admin-certificaciones.html Profesores.
const PROFESSORS: &[&str] = &[
    "Prof. Ana Muestra Demo",
    "Prof. Jorge Ejemplo Ficticio",
    "Prof. Lucía Prueba Demo",
    "Prof. Tomás Ficticio Muestra",
    "Prof. Gloria Demo Ejemplo",
    "Prof. Iván Muestra Prueba",
];

/// Specialized courses: (name, member price, individual price). All 3 credits, 45 hours, quorum 12.
const COURSES: &[(&str, i64, i64)] = &[
    ("Suscripción de Préstamos FHA", 35_000, 42_500),    // 0
    ("Análisis de Crédito Residencial", 32_500, 40_000), // 1
    ("Cumplimiento TILA-RESPA", 35_000, 42_500),         // 2
    ("Ética Profesional Hipotecaria", 32_500, 40_000),   // 3
    ("Procesamiento de Préstamos", 32_500, 40_000),      // 4
    ("Tasación Residencial Básica", 30_000, 37_500),     // 5
    ("Suscripción Avanzada", 35_000, 42_500),            // 6
    ("Documentación de Cierre", 32_500, 40_000),         // 7
    ("Seguros", 30_000, 37_500),                         // 8
    ("Ley SAFE", 30_000, 37_500),                        // 9
    ("Auditoría Interna", 32_500, 40_000),               // 10
    ("Originación de Préstamos I", 32_500, 40_000),      // 11
];

/// design/estudiante.html certifications: (code, name, required course indexes, required electives).
const CERTIFICATIONS: &[(&str, &str, &[usize], i64)] = &[
    ("COH", "Originador Hipotecario", &[0, 1, 3, 11], 0),
    ("CSH", "Suscriptor Hipotecario", &[0, 1, 2, 6], 2),
    ("CPP", "Procesador de Préstamos", &[1, 3, 4, 7, 8], 0),
    ("CCR", "Cumplimiento Regulatorio", &[3, 2, 9, 10], 0),
];

/// Course calendar rows: (course, first class date, semester, status, member price, individual price, target count).
const CALENDAR: &[(usize, &str, &str, &str, i64, i64, usize)] = &[
    // design/ofrecimiento.html: the five rows of Suscripción de Préstamos FHA.
    (0, "2027-01-18", "Ene–May 2027", "Pendiente", 35_000, 42_500, 6),
    (0, "2026-09-14", "Ago–Dic 2026", "Confirmado", 35_000, 42_500, 18),
    (0, "2026-03-09", "Ene–May 2026", "Completado", 32_500, 40_000, 21),
    (0, "2025-10-06", "Ago–Dic 2025", "Cancelado", 32_500, 40_000, 4),
    (0, "2025-02-10", "Ene–May 2025", "Completado", 32_500, 40_000, 15),
    // Elena's other courses (design/estudiante.html) and the rows of design/matriculas.html.
    (2, "2027-01-25", "Ene–May 2027", "Pendiente", 35_000, 42_500, 5),
    (1, "2026-03-09", "Ene–May 2026", "Completado", 32_500, 40_000, 9),
    (1, "2026-08-31", "Ago–Dic 2026", "Confirmado", 32_500, 40_000, 10),
    (3, "2025-10-06", "Ago–Dic 2025", "Completado", 32_500, 40_000, 8),
    (4, "2025-02-10", "Ene–May 2025", "Completado", 32_500, 40_000, 7),
    (5, "2024-09-09", "Ago–Dic 2024", "Completado", 30_000, 37_500, 6),
    (11, "2024-03-04", "Ene–May 2024", "Completado", 32_500, 40_000, 6),
    // One row for each remaining course.
    (6, "2027-02-08", "Ene–May 2027", "Pendiente", 35_000, 42_500, 3),
    (7, "2026-10-05", "Ago–Dic 2026", "Confirmado", 32_500, 40_000, 8),
    (8, "2026-04-13", "Ene–May 2026", "Completado", 30_000, 37_500, 7),
    (9, "2026-11-02", "Ago–Dic 2026", "Confirmado", 30_000, 37_500, 9),
    (10, "2027-03-01", "Ene–May 2027", "Pendiente", 32_500, 40_000, 2),
];

/// design/estudiantes.html, afiliado.html: (name, affiliate account or 0, typed company, phone, e-mail).
const NAMED: &[(&str, i64, &str, &str, &str)] = &[
    ("Elena Muestra Ficticia", 1001, "Banco Ejemplo", "(787) 555-0142", "elena.muestra@ejemplo.com"),
    ("Luis Ejemplo Prueba", 1002, "Acme Mortgage Demo", "(787) 555-0187", "luis.ejemplo@ejemplo.com"),
    ("Carmen Demo Ficticio", 1004, "Capital Ficticio Lending", "(939) 555-0110", "carmen.demo@ejemplo.com"),
    // Rafael, Héctor and Marisol typed a company but are not linked to an affiliate: they pay individual prices
    // (design/matriculas.html: INDIVIDUAL $425.00).
    ("Rafael Prueba Muestra", 0, "Cooperativa Muestra", "(787) 555-0163", "rafael.prueba@ejemplo.com"),
    ("Sofía Ficticia Demo", 1005, "Hipotecaria Modelo PR", "(787) 555-0129", "sofia.ficticia@ejemplo.com"),
    ("Héctor Muestra Demo", 0, "Banco Ejemplo", "(939) 555-0175", "hector.muestra@ejemplo.com"),
    ("Marisol Ejemplo Ruiz", 0, "Avalúos Demo Partners", "(787) 555-0198", "marisol.ejemplo@ejemplo.com"),
    ("Pedro Prueba Demo", 1003, "Cooperativa Muestra", "(787) 555-0104", "pedro.prueba@ejemplo.com"),
    ("Gloria Prueba Ficticia", 1001, "Banco Ejemplo", "(787) 555-0151", "gloria.prueba@ejemplo.com"),
    ("Iván Demo Muestra", 1001, "Banco Ejemplo", "(787) 555-0116", "ivan.demo@ejemplo.com"),
];

/// design/matriculas.html + importar-notas.html, Suscripción de Préstamos FHA 9/14/2026:
/// (named index, serial, invoice, paid cents, BAJA, grades: examen, prom. asignaciones, asistencia, prom. final).
type GradeRow = (usize, i64, i64, i64, bool, Option<(f64, f64, f64, f64)>);
const FHA_2026: &[GradeRow] = &[
    (0, 10482, 26114, 35_000, false, Some((94.0, 90.0, 100.0, 92.0))),
    (1, 10483, 26115, 35_000, false, Some((81.0, 85.0, 95.0, 84.0))),
    (5, 10484, 26116, 10_000, true, None),
    (3, 10486, 26118, 20_000, false, Some((66.0, 70.0, 90.0, 70.0))),
    (4, 10487, 26119, 35_000, false, Some((58.0, 62.0, 80.0, 61.0))),
    (6, 10489, 26121, 42_500, false, Some((88.0, 91.0, 100.0, 90.0))),
    (7, 10490, 26122, 0, false, None),
];

/// Elena's other course enrollments (design/estudiante.html): (calendar index, paid, BAJA, score).
/// Originación de Préstamos I (85) is added so her summary reads 90 · 4 courses · 12 credits · 180 hours.
const ELENA_COURSES: &[(usize, i64, bool, Option<i64>)] = &[
    (5, 0, false, None),
    (6, 32_500, false, Some(88)),
    (8, 32_500, false, Some(95)),
    (9, 10_000, true, None),
    (10, 30_000, false, Some(64)),
    (11, 32_500, false, Some(85)),
];

/// Seminars: (name, hours, member price, individual price, calendar rows: (date, status)).
#[allow(clippy::type_complexity)]
const SEMINARS: &[(&str, f64, i64, i64, &[(&str, &str)])] = &[
    ("Seminario de Tendencias del Mercado 2026", 4.0, 9_500, 11_500, &[("2026-05-07", "Completado")]),
    (
        "Prevención de Fraude Hipotecario",
        3.0,
        8_000,
        9_500,
        &[("2025-11-13", "Completado"), ("2027-02-11", "Pendiente")],
    ),
    (
        "Actualización Reglamentaria OCIF",
        3.0,
        8_000,
        9_500,
        &[("2025-04-24", "Completado"), ("2026-12-10", "Confirmado")],
    ),
];

/// Short courses: (name, hours, NMLS fee, member price, individual price, calendar rows: (first date, status)).
#[allow(clippy::type_complexity)]
const SHORT: &[(&str, f64, bool, i64, i64, &[(&str, &str)])] = &[
    (
        "Educación Continua NMLS (8 h)",
        8.0,
        true,
        15_000,
        18_000,
        &[("2026-12-05", "Confirmado"), ("2027-03-06", "Pendiente")],
    ),
    ("Préstamos FHA y VA", 6.0, false, 12_000, 15_000, &[("2027-02-20", "Pendiente")]),
];

fn phone(g: &mut Gen) -> String {
    format!("(787) 555-01{:02}", g.range(0, 99))
}

struct Student {
    uuid: String,
    nombre: String,
    email: String,
    member: bool,
}

#[allow(clippy::too_many_lines)]
pub(super) fn seed_school(tx: &Transaction, g: &mut Gen) -> rusqlite::Result<School> {
    let a = SEED_ACTOR;
    let at = g.at(0);

    // Affiliates.
    let mut affiliates: Vec<(i64, String, String, bool)> = Vec::new(); // (account, uuid, name, active)
    for (acct, name, business, member, city, ph, status) in AFFILIATES {
        let uuid = g.uuid();
        let banco = *acct == 1001;
        tx.execute(
            "INSERT INTO affiliates (uuid, account_num, socio, other_name, address, city, state_zip, zip, telephone, telephone_1,
               fax, miembro, tipo_de_negocio, address1, e_mail, cust_type, created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'PR', ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?16, ?17)",
            params![
                uuid,
                acct,
                name,
                banco.then_some("Banco Ejemplo de PR"),
                if banco { "Avenida Ficticia 400, Suite 12".to_owned() } else { format!("Calle Ejemplo {}", acct - 900) },
                city,
                if banco { "00900" } else { "00000" },
                ph,
                banco.then_some("(787) 555-0301"),
                banco.then_some("(787) 555-0309"),
                member,
                business,
                banco.then_some("Depto. de Adiestramiento\nBanco Ejemplo\nApartado 0000, San Juan PR 00900"),
                if banco { "educacion@ejemplo.com".to_owned() } else { format!("afiliado{acct}@ejemplo.com") },
                status,
                at,
                a
            ],
        )?;
        affiliates.push((*acct, uuid, (*name).to_owned(), *status == "ACTIVO"));
    }
    let affiliate = |acct: i64| affiliates.iter().find(|x| x.0 == acct).cloned();

    // Professors.
    let mut professors = Vec::new();
    for name in PROFESSORS {
        let uuid = g.uuid();
        tx.execute(
            "INSERT INTO professors (uuid, name, created_at, created_by, modified_at, modified_by) VALUES (?1, ?2, ?3, ?4, ?3, ?4)",
            params![uuid, name, at, a],
        )?;
        professors.push(uuid);
    }

    // Courses, seminars, short courses.
    let mut courses = Vec::new();
    for (i, (name, m, ind)) in COURSES.iter().enumerate() {
        let uuid = g.uuid();
        let fha = i == 0;
        tx.execute(
            "INSERT INTO courses (uuid, course, course_description, course_requirements, cost_member, cost_individual, credits,
               hours, quorum, created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 3, 45, 12, ?7, ?8, ?7, ?8)",
            params![
                uuid,
                name,
                if fha {
                    "Guías de suscripción FHA para préstamos residenciales: capacidad de pago, crédito, colateral y documentación. Texto de ejemplo.".to_owned()
                } else {
                    format!("Curso de {}. Texto de ejemplo.", name.to_lowercase())
                },
                fha.then_some("Análisis de Crédito Residencial o experiencia equivalente."),
                m,
                ind,
                at,
                a
            ],
        )?;
        courses.push((uuid, (*name).to_owned()));
    }

    // Certifications and requirements (BR-CRT-01).
    let mut certs = Vec::new();
    for (code, name, req, electives) in CERTIFICATIONS {
        let uuid = g.uuid();
        tx.execute(
            "INSERT INTO certifications (uuid, certification, description, number_of_required_electives, created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?5, ?6)",
            params![uuid, code, name, electives, at, a],
        )?;
        for ci in req.iter() {
            tx.execute(
                "INSERT INTO certification_requirements (uuid, certification_uuid, course_uuid, type, created_at, created_by, modified_at, modified_by)
                 VALUES (?1, ?2, ?3, 'Requisito', ?4, ?5, ?4, ?5)",
                params![g.uuid(), uuid, courses[*ci].0, at, a],
            )?;
        }
        certs.push(uuid);
    }
    // An elective on CSH, so the requirements list shows both types.
    tx.execute(
        "INSERT INTO certification_requirements (uuid, certification_uuid, course_uuid, type, created_at, created_by, modified_at, modified_by)
         VALUES (?1, ?2, ?3, 'Electiva', ?4, ?5, ?4, ?5)",
        params![g.uuid(), certs[1], courses[5].0, at, a],
    )?;

    // Students: the ten of the design, then synthetic ones. One certification row per certification (BR-STU-01).
    let mut students: Vec<Student> = Vec::new();
    for (i, (name, acct, company, ph, mail)) in NAMED.iter().enumerate() {
        let uuid = g.uuid();
        let aff = if *acct > 0 { affiliate(*acct) } else { None };
        let elena = i == 0;
        tx.execute(
            "INSERT INTO students (uuid, member_uuid, compania, nombre, email, email_work, telefono_r, telefono_t, phone_mobile,
               company_position, company_department, nmls, student_id, dir_residencial, residencial_2, home_city, home_state,
               home_zip, emergency_contact, emergency_phone, emergency_contact_relationship, notes,
               created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, 'PR', '00000', ?17, ?18, ?19, ?20, ?21, ?22, ?21, ?22)",
            params![
                uuid,
                aff.as_ref().map(|x| x.1.clone()),
                company,
                name,
                mail,
                elena.then_some("elena.trabajo@ejemplo.com"),
                ph,
                elena.then_some("(787) 555-0300"),
                elena.then_some("(939) 555-0142"),
                elena.then_some("Oficial de préstamos"),
                elena.then_some("Hipotecas"),
                if elena { "900001".to_owned() } else { format!("{}", 900_002 + i as i64) },
                if elena { "4821".to_owned() } else { format!("{}", 4822 + i as i64) },
                if elena { "Calle Ficticia 123".to_owned() } else { format!("Calle Muestra {}", 10 + i) },
                elena.then_some("Urb. Ejemplo"),
                if elena { "Ciudad Ejemplo" } else { CITIES[i % CITIES.len()] },
                elena.then_some("Pedro Prueba Demo"),
                elena.then_some("(787) 555-0104"),
                elena.then_some("Hermano"),
                elena.then_some("Prefiere clases en la tarde. Texto de ejemplo."),
                at,
                a
            ],
        )?;
        students.push(Student {
            uuid,
            nombre: (*name).to_owned(),
            email: (*mail).to_owned(),
            member: aff.is_some_and(|x| x.3),
        });
    }
    for i in 0..70i64 {
        let uuid = g.uuid();
        let first = *g.pick(FIRST);
        let l1 = *g.pick(LAST);
        let mut l2 = *g.pick(LAST);
        if l2 == l1 {
            l2 = "Ejemplar";
        }
        let nombre = format!("{first} {l1} {l2}");
        let aff = if g.chance(0.65) {
            Some(affiliates[g.range(0, affiliates.len() as i64 - 1) as usize].clone())
        } else {
            None
        };
        let mail = format!("{}.{}{}@ejemplo.com", ascii(first), ascii(l1), i + 1);
        let ph = phone(g);
        let stamp = g.stamp();
        tx.execute(
            "INSERT INTO students (uuid, member_uuid, compania, nombre, email, telefono_r, home_city, home_state, nmls, student_id,
               created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'PR', ?8, ?9, ?10, ?11, ?10, ?11)",
            params![
                uuid,
                aff.as_ref().map(|x| x.1.clone()),
                aff.as_ref().map(|x| x.2.clone()),
                nombre,
                mail,
                ph,
                CITIES[(i as usize) % CITIES.len()],
                format!("{}", 910_000 + i),
                format!("{}", 5000 + i),
                stamp,
                a
            ],
        )?;
        students.push(Student { uuid, nombre, email: mail, member: aff.is_some_and(|x| x.3) });
    }
    for s in &students {
        for c in &certs {
            tx.execute(
                "INSERT INTO student_certifications (uuid, student_uuid, certification_uuid, date, graduation_date, created_at, created_by, modified_at, modified_by)
                 VALUES (?1, ?2, ?3, '2026-01-12', ?4, ?5, ?6, ?5, ?6)",
                params![g.uuid(), s.uuid, c, (s.nombre == "Elena Muestra Ficticia" && *c == certs[0]).then_some("2026-06-30"), at, a],
            )?;
        }
    }
    // Elena's certifications of interest (BR-STU-05).
    for (rep, ci) in [(1i64, 1usize), (2, 3)] {
        tx.execute(
            "INSERT INTO student_certification_interests (uuid, student_uuid, repetition, certification_uuid, date) VALUES (?1, ?2, ?3, ?4, '2026-02-03')",
            params![g.uuid(), students[0].uuid, rep, certs[ci]],
        )?;
    }

    // Course calendar rows with four weekly class dates each.
    let mut calendars = Vec::new();
    for (i, (ci, first, semester, status, m, ind, _)) in CALENDAR.iter().enumerate() {
        let uuid = g.uuid();
        tx.execute(
            "INSERT INTO calendar_course (uuid, serial, course_uuid, status, semester, cost_member, cost_individual,
               created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?8, ?9)",
            params![uuid, i as i64 + 1, courses[*ci].0, status, semester, m, ind, at, a],
        )?;
        let start = chrono::NaiveDate::parse_from_str(first, "%Y-%m-%d").expect("date");
        let prof = &professors[i % professors.len()];
        for w in 0..4 {
            let d = (start + chrono::Duration::weeks(w)).format("%Y-%m-%d").to_string();
            tx.execute(
                "INSERT INTO calendar_course_dates (uuid, calendar_course_uuid, date, status, professor_uuid, created_at, created_by, modified_at, modified_by)
                 VALUES (?1, ?2, ?3, 'Día de Clase', ?4, ?5, ?6, ?5, ?6)",
                params![g.uuid(), uuid, d, prof, at, a],
            )?;
        }
        calendars.push(uuid);
    }

    // Course enrollments. Named rows first (fixed serials and invoices), then synthetic fill up to each row's count.
    let mut serial = 11_000i64;
    let mut id_num = 1_000i64;
    let mut invoice = 27_000i64;
    let mut per_calendar = vec![Vec::<usize>::new(); CALENDAR.len()];
    let course_enroll = |tx: &Transaction,
                         g: &mut Gen,
                         cal: usize,
                         st: &Student,
                         serial: i64,
                         id_num: i64,
                         invoice: i64,
                         paid: Option<i64>,
                         baja: bool,
                         grades: Option<(f64, f64, f64, f64)>,
                         score: Option<i64>|
     -> rusqlite::Result<()> {
        let (ci, first, _, status, m, ind, _) = CALENDAR[cal];
        let price = if st.member { m } else { ind };
        let paid = paid.unwrap_or_else(|| {
            if status == "Pendiente" || status == "Cancelado" {
                0
            } else {
                [0, price / 2, price, price][g.range(0, 3) as usize]
            }
        });
        let score = score.or(grades.map(|x| x.3.round() as i64));
        tx.execute(
            "INSERT INTO enrollments_course (uuid, serial, student_uuid, course_uuid, course_calendar_uuid, costeado_por, costo, pago,
               id_num, invoice_num, fecha_de_pago, puntuacion, status, examen, promedio_asignaciones, asistencia_y_puntualidad,
               promedio_final, created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?18, ?19)",
            params![
                g.uuid(),
                serial,
                st.uuid,
                courses[ci].0,
                calendars[cal],
                if st.member { "COMPAÑIA" } else { "INDIVIDUAL" },
                price,
                paid,
                id_num,
                invoice,
                (paid > 0).then_some(first),
                score,
                if baja { "BAJA" } else { "ACTIVO" },
                grades.map(|x| x.0),
                grades.map(|x| x.1),
                grades.map(|x| x.2),
                grades.map(|x| x.3),
                at,
                SEED_ACTOR
            ],
        )?;
        Ok(())
    };
    for (si, ser, inv, paid, baja, grades) in FHA_2026 {
        id_num += 1;
        course_enroll(tx, g, 1, &students[*si], *ser, id_num, *inv, Some(*paid), *baja, *grades, None)?;
        per_calendar[1].push(*si);
    }
    for (cal, paid, baja, score) in ELENA_COURSES {
        serial += 1;
        id_num += 1;
        invoice += 1;
        course_enroll(tx, g, *cal, &students[0], serial, id_num, invoice, Some(*paid), *baja, None, *score)?;
        per_calendar[*cal].push(0);
    }
    for (cal, row) in CALENDAR.iter().enumerate() {
        let (_, _, _, status, _, _, target) = *row;
        while per_calendar[cal].len() < target {
            let si = g.range(10, students.len() as i64 - 1) as usize;
            if per_calendar[cal].contains(&si) {
                continue;
            }
            serial += 1;
            id_num += 1;
            invoice += 1;
            let score = if status == "Completado" { Some(g.range(55, 100)) } else { None };
            let baja = status != "Cancelado" && g.chance(0.06);
            course_enroll(tx, g, cal, &students[si], serial, id_num, invoice, None, baja, None, score)?;
            per_calendar[cal].push(si);
        }
    }

    // Seminars, their calendar rows and enrollments (Elena: two Asistió, one No Canceló paid half).
    let mut sem_invoice = 28_000i64;
    let mut seminar_enrollment = (String::new(), String::new());
    for (k, (name, hours, m, ind, rows)) in SEMINARS.iter().enumerate() {
        let su = g.uuid();
        tx.execute(
            "INSERT INTO seminars (uuid, seminar, description, cost_individual, cost_affiliate, hours, created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?7, ?8)",
            params![su, name, format!("{name}. Texto de ejemplo."), ind, m, hours, at, a],
        )?;
        for (r, (date, status)) in rows.iter().enumerate() {
            let cu = g.uuid();
            tx.execute(
                "INSERT INTO calendar_seminar (uuid, serial, seminar_uuid, status, date, quorum, cost_member, cost_individual, professor_uuid,
                   created_at, created_by, modified_at, modified_by)
                 VALUES (?1, ?2, ?3, ?4, ?5, 15, ?6, ?7, ?8, ?9, ?10, ?9, ?10)",
                params![cu, (k * 2 + r) as i64 + 1, su, status, date, m, ind, professors[(k + r) % professors.len()], at, a],
            )?;
            let mut enrolled: Vec<usize> = Vec::new();
            if r == 0 {
                enrolled.push(0);
            }
            let target = if *status == "Completado" { 8 } else { 4 };
            while enrolled.len() < target {
                let si = g.range(1, students.len() as i64 - 1) as usize;
                if !enrolled.contains(&si) {
                    enrolled.push(si);
                }
            }
            for (n, si) in enrolled.iter().enumerate() {
                let st = &students[*si];
                let price = if st.member { *m } else { *ind };
                let (st_status, paid) = if *si == 0 {
                    match k {
                        2 => (Some("No Canceló"), price / 2),
                        _ => (Some("Asistió"), price),
                    }
                } else if *status == "Completado" {
                    (
                        Some(*g.pick(&["Asistió", "Asistió", "Asistió", "No Canceló"])),
                        if g.chance(0.8) { price } else { 0 },
                    )
                } else {
                    (None, 0)
                };
                sem_invoice += 1;
                tx.execute(
                    "INSERT INTO enrollments_seminars (uuid, serial, student_uuid, seminar_uuid, seminar_calendar_uuid, status, cargo,
                       costeado_por, pago, invoice_number, date_of_payment, created_at, created_by, modified_at, modified_by)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?12, ?13)",
                    params![
                        g.uuid(),
                        n as i64 + 1,
                        st.uuid,
                        su,
                        cu,
                        st_status,
                        price,
                        if st.member { "COMPAÑIA" } else { "INDIVIDUAL" },
                        paid,
                        sem_invoice,
                        (paid > 0).then_some(*date),
                        at,
                        a
                    ],
                )?;
                if k == 1 && r == 1 && n == 0 {
                    seminar_enrollment = (st.nombre.clone(), (*name).to_owned());
                }
            }
        }
    }

    // Short courses with up to 15 dates (three here) and enrollments (Elena has none, as the design shows).
    let mut sc_invoice = 29_000i64;
    for (k, (name, hours, nmls, m, ind, rows)) in SHORT.iter().enumerate() {
        let su = g.uuid();
        tx.execute(
            "INSERT INTO short_courses (uuid, short_course, description, cost_individual, cost_affiliate, hours, nmls_fee,
               created_at, created_by, modified_at, modified_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?8, ?9)",
            params![su, name, format!("{name}. Texto de ejemplo."), ind, m, hours, *nmls as i64, at, a],
        )?;
        for (r, (first, status)) in rows.iter().enumerate() {
            let cu = g.uuid();
            tx.execute(
                "INSERT INTO calendar_short_course (uuid, serial, short_course_uuid, status, quorum, cost_member, cost_individual, hours,
                   created_at, created_by, modified_at, modified_by)
                 VALUES (?1, ?2, ?3, ?4, 12, ?5, ?6, ?7, ?8, ?9, ?8, ?9)",
                params![cu, (k * 2 + r) as i64 + 1, su, status, m, ind, hours, at, a],
            )?;
            let start = chrono::NaiveDate::parse_from_str(first, "%Y-%m-%d").expect("date");
            for rep in 1..=3i64 {
                let d = (start + chrono::Duration::days((rep - 1) * 7)).format("%Y-%m-%d").to_string();
                tx.execute(
                    "INSERT INTO calendar_short_course_dates (uuid, calendar_short_course_uuid, repetition, date, status, professor_uuid)
                     VALUES (?1, ?2, ?3, ?4, 'Día de Clase', ?5)",
                    params![g.uuid(), cu, rep, d, professors[(k + 2) % professors.len()]],
                )?;
            }
            let mut enrolled: Vec<usize> = Vec::new();
            let target = if *status == "Confirmado" { 7 } else { 3 };
            while enrolled.len() < target {
                let si = g.range(1, students.len() as i64 - 1) as usize;
                if !enrolled.contains(&si) {
                    enrolled.push(si);
                }
            }
            for (n, si) in enrolled.iter().enumerate() {
                let st = &students[*si];
                let price = if st.member { *m } else { *ind };
                sc_invoice += 1;
                let paid = if *status == "Confirmado" && g.chance(0.6) { price } else { 0 };
                tx.execute(
                    "INSERT INTO enrollments_short_course (uuid, serial, student_uuid, short_course_uuid, short_course_calendar_uuid,
                       invoice_number, cargo, costeado_por, pago, license_number, created_at, created_by, modified_at, modified_by)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?11, ?12)",
                    params![
                        g.uuid(),
                        n as i64 + 1,
                        st.uuid,
                        su,
                        cu,
                        sc_invoice,
                        price,
                        if st.member { "COMPAÑIA" } else { "INDIVIDUAL" },
                        paid,
                        format!("LIC-{}", 20_000 + sc_invoice),
                        at,
                        a
                    ],
                )?;
            }
        }
    }

    Ok(School {
        students: students.into_iter().map(|s| (s.uuid, s.nombre, s.email)).collect(),
        first_course: (COURSES[0].0.to_owned(), "18/1/2027".to_owned()),
        seminar_enrollment,
    })
}
