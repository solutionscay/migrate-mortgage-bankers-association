//! I3 gate: server-side invariants and negative authorization of the school domain, before any screen is wired.
//! T-07-07..13, T-07-18, T-08-11; the domain parts of T-05-07, T-05-11, T-05-12; the #5 gates BR-ENR-06 (import),
//! BR-STU-02 (affiliate by key), D-06 (no SSN); AP-03..06, AP-08, AP-09 retested on school resources.

#[path = "autosave_support.rs"]
mod support;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use mba_server::auth::Role;
use mba_server::autosave::Outcome;
use mba_server::school::registries::{patch, Patched, Resource};
use mba_server::school::{
    affiliates, begin, certifications, enrollments, grades, offerings, students, DomainError, Kind, Who,
};
use mba_server::{db, seed};
use rusqlite::Connection;

fn seeded() -> Connection {
    let mut conn = db::open_in_memory().unwrap();
    seed::run(&mut conn, seed::DEFAULT_SEED).unwrap();
    conn
}
fn admin() -> Who {
    Who::new(1, "mvega", Role::Admin)
}
fn staff() -> Who {
    Who::new(3, "equintana", Role::Staff)
}
fn one<T: rusqlite::types::FromSql>(conn: &Connection, sql: &str, arg: &str) -> T {
    conn.query_row(sql, [arg], |r| r.get(0)).unwrap()
}
fn uuid_of(conn: &Connection, table: &str, col: &str, val: &str) -> String {
    one(conn, &format!("SELECT uuid FROM {table} WHERE {col} = ?1"), val)
}
fn patched(conn: &mut Connection, who: &Who, res: Resource, key: &str, name: &str, raw: &str) -> Outcome {
    match patch(conn, who, None, res, key, name, raw).unwrap() {
        Patched::Done(o) => o,
        Patched::Unknown => panic!("unknown field {name}"),
    }
}
fn is_saved(o: &Outcome) -> bool {
    matches!(o, Outcome::Saved { .. })
}

/// A fresh offering of `kind` with one calendar row of `status`.
fn calendar(conn: &mut Connection, kind: Kind, status: &str) -> (String, String) {
    let tx = begin(conn).unwrap();
    let o = offerings::create_offering(
        &tx,
        &staff(),
        kind,
        &offerings::NewOffering {
            name: format!("Prueba {kind:?} {status}"),
            member_price: "100".into(),
            individual_price: "150".into(),
        },
    )
    .unwrap();
    let c = offerings::open_calendar(
        &tx,
        &staff(),
        kind,
        &o,
        &offerings::NewCalendar { semester: "Ene–May 2027".into(), date: "2027-02-01".into() },
    )
    .unwrap();
    tx.execute(&format!("UPDATE {} SET status = ?1 WHERE uuid = ?2", kind.calendar_table()), [status, &c]).unwrap();
    tx.commit().unwrap();
    (o, c)
}

fn new_student(conn: &mut Connection, name: &str, member: Option<&str>) -> String {
    let tx = begin(conn).unwrap();
    let s = students::create_student(
        &tx,
        &staff(),
        &students::NewStudent { nombre: name.into(), member_uuid: member.map(str::to_owned), ..Default::default() },
    )
    .unwrap();
    tx.commit().unwrap();
    s
}

// --- T-07-07 (BR-STU-01) --------------------------------------------------------------------------------------------

#[test]
fn t07_07_one_certification_row_each() {
    let mut conn = seeded();
    let certs: i64 = conn.query_row("SELECT COUNT(*) FROM certifications", [], |r| r.get(0)).unwrap();
    let s = new_student(&mut conn, "Ana Nueva Prueba", None);
    let rows = |c: &Connection, s: &str| {
        one::<i64>(c, "SELECT COUNT(*) FROM student_certifications WHERE student_uuid = ?1", s)
    };
    assert_eq!(rows(&conn, &s), certs);
    let tx = begin(&mut conn).unwrap();
    let d = students::duplicate_student(&tx, &staff(), &s).unwrap();
    assert_eq!(students::ensure_certification_rows(&tx, &staff(), &s).unwrap(), 0, "a rerun adds no duplicate pair");
    tx.commit().unwrap();
    assert_eq!(rows(&conn, &d), certs);
    assert_eq!(one::<String>(&conn, "SELECT nombre FROM students WHERE uuid = ?1", &d), "Ana Nueva Prueba");
    let dup: i64 = conn
        .query_row("SELECT COUNT(*) FROM (SELECT student_uuid, certification_uuid FROM student_certifications GROUP BY 1, 2 HAVING COUNT(*) > 1)", [], |r| r.get(0))
        .unwrap();
    assert_eq!(dup, 0);
    // A new certification gives every student its row too.
    let tx = begin(&mut conn).unwrap();
    let c = certifications::create_certification(&tx, &admin(), "NEW", "Certificación nueva").unwrap();
    tx.commit().unwrap();
    let students_n: i64 = conn.query_row("SELECT COUNT(*) FROM students", [], |r| r.get(0)).unwrap();
    assert_eq!(
        one::<i64>(&conn, "SELECT COUNT(*) FROM student_certifications WHERE certification_uuid = ?1", &c),
        students_n
    );
}

// --- T-07-08 (BR-STU-03) --------------------------------------------------------------------------------------------

#[test]
fn t07_08_student_status_is_live() {
    let mut conn = seeded();
    let s = new_student(&mut conn, "Estatus Prueba", None);
    assert_eq!(students::academic(&conn, &s).unwrap().status, "INACTIVO", "no enrollments");
    let (_, cal) = calendar(&mut conn, Kind::Course, "Pendiente");
    let tx = begin(&mut conn).unwrap();
    enrollments::enroll(&tx, &staff(), Kind::Course, &s, &cal).unwrap();
    tx.commit().unwrap();
    assert_eq!(students::academic(&conn, &s).unwrap().status, "INACTIVO", "only Pendiente");
    assert!(is_saved(&patched(&mut conn, &staff(), Resource::Calendar(Kind::Course), &cal, "status", "Confirmado")));
    assert_eq!(students::academic(&conn, &s).unwrap().status, "ACTIVO", "Confirmado");
}

// --- T-07-09 (BR-CRT-02..04) ----------------------------------------------------------------------------------------

#[test]
fn t07_09_certification_progress() {
    let mut conn = seeded();
    let w = admin();
    let s = new_student(&mut conn, "Progreso Prueba", None);
    let (course, cal) = calendar(&mut conn, Kind::Course, "Confirmado");
    let tx = begin(&mut conn).unwrap();
    let cert = certifications::create_certification(&tx, &w, "TST", "Certificación de prueba").unwrap();
    certifications::add_requirement(&tx, &w, &cert, &course, "Requisito").unwrap();
    // Taken twice, passed twice.
    for _ in 0..2 {
        let e = enrollments::enroll(&tx, &staff(), Kind::Course, &s, &cal).unwrap();
        tx.execute("UPDATE enrollments_course SET puntuacion = 90 WHERE uuid = ?1", [&e.uuid]).unwrap();
    }
    tx.commit().unwrap();
    let p =
        students::certification_progress(&conn, &s).unwrap().into_iter().find(|p| p.certification == "TST").unwrap();
    assert_eq!((p.total, p.completed, p.left_count), (1, 1, 0), "a retaken course counts once; left never negative");
    assert!(p.courses_left.is_empty());
    assert!(!p.is_graduated);
    assert!(
        !certifications::certificate_allowed(&conn, &p.uuid).unwrap(),
        "BR-CRT-04: no certificate before graduation"
    );
    assert!(is_saved(&patched(&mut conn, &staff(), Resource::StudentCert, &p.uuid, "graduation_date", "6/30/2026")));
    assert!(certifications::certificate_allowed(&conn, &p.uuid).unwrap());
    // The seeded Elena: the design's progress rows.
    let elena = uuid_of(&conn, "students", "nombre", "Elena Muestra Ficticia");
    let rows = students::certification_progress(&conn, &elena).unwrap();
    let cpp = rows.iter().find(|r| r.certification == "CPP").unwrap();
    assert_eq!((cpp.total, cpp.completed, cpp.left_count, cpp.average_points), (5, 2, 3, Some(92)));
    assert_eq!(cpp.courses_left, ["Documentación de Cierre", "Procesamiento de Préstamos", "Seguros"]);
    assert!(rows.iter().find(|r| r.certification == "COH").unwrap().is_graduated);
}

// --- T-07-10, T-05-11 (BR-ENR-01, BR-CRS-01) ------------------------------------------------------------------------

#[test]
fn t07_10_price_by_affiliate_status_and_active_rows() {
    let mut conn = seeded();
    let active_aff = uuid_of(&conn, "affiliates", "socio", "Banco Ejemplo");
    let inactive_aff = uuid_of(&conn, "affiliates", "socio", "Capital Ficticio Lending");
    let member = new_student(&mut conn, "Socio Activo", Some(&active_aff));
    let inactive = new_student(&mut conn, "Socio Inactivo", Some(&inactive_aff));
    let none = new_student(&mut conn, "Sin Afiliado", None);
    let (offering, cal) = calendar(&mut conn, Kind::Course, "Confirmado");
    // Two more rows of the same offering: one Completado, one Cancelado (not offered).
    let tx = begin(&mut conn).unwrap();
    for st in ["Completado", "Cancelado"] {
        let c = offerings::open_calendar(&tx, &staff(), Kind::Course, &offering, &Default::default()).unwrap();
        tx.execute("UPDATE calendar_course SET status = ?1 WHERE uuid = ?2", [st, &c]).unwrap();
    }
    tx.commit().unwrap();
    // T-05-11: only active rows, with the member price for the affiliate student.
    let opts = offerings::calendar_options(&conn, Kind::Course, &offering, &member).unwrap();
    assert_eq!(opts.len(), 1);
    assert_eq!(
        (opts[0].uuid.as_str(), opts[0].price, opts[0].member_price_applies),
        (cal.as_str(), Some(10_000), true)
    );
    assert_eq!(offerings::calendar_options(&conn, Kind::Course, &offering, &none).unwrap()[0].price, Some(15_000));

    let tx = begin(&mut conn).unwrap();
    let e1 = enrollments::enroll(&tx, &staff(), Kind::Course, &member, &cal).unwrap();
    let e2 = enrollments::enroll(&tx, &staff(), Kind::Course, &inactive, &cal).unwrap();
    let e3 = enrollments::enroll(&tx, &staff(), Kind::Course, &none, &cal).unwrap();
    assert_eq!((e1.price, e2.price, e3.price), (Some(10_000), Some(15_000), Some(15_000)));
    assert_eq!(one::<String>(&tx, "SELECT costeado_por FROM enrollments_course WHERE uuid = ?1", &e1.uuid), "COMPAÑIA");
    assert_eq!(
        one::<String>(&tx, "SELECT status FROM enrollments_course WHERE uuid = ?1", &e1.uuid),
        "ACTIVO",
        "BR-ENR-09"
    );
    let done: String =
        one(&tx, "SELECT uuid FROM calendar_course WHERE course_uuid = ?1 AND status = 'Completado'", &offering);
    assert_eq!(enrollments::enroll(&tx, &staff(), Kind::Course, &member, &done), Err(DomainError::CalendarNotActive));
    // The price was stored: a later price change of the row does not move it (D-14).
    tx.execute("UPDATE calendar_course SET cost_member = 99999 WHERE uuid = ?1", [&cal]).unwrap();
    assert_eq!(one::<i64>(&tx, "SELECT costo FROM enrollments_course WHERE uuid = ?1", &e1.uuid), 10_000);
    // Seminars and short courses price the same way.
    tx.commit().unwrap();
    for kind in [Kind::Seminar, Kind::ShortCourse] {
        let (_, c) = calendar(&mut conn, kind, "Pendiente");
        let tx = begin(&mut conn).unwrap();
        assert_eq!(enrollments::enroll(&tx, &staff(), kind, &member, &c).unwrap().price, Some(10_000), "{kind:?}");
        assert_eq!(enrollments::enroll(&tx, &staff(), kind, &none, &c).unwrap().price, Some(15_000), "{kind:?}");
        tx.commit().unwrap();
    }
}

// --- T-07-11 (BR-ENR-03..05) ----------------------------------------------------------------------------------------

#[test]
fn t07_11_score_letter_passed() {
    let mut conn = seeded();
    let s = new_student(&mut conn, "Notas Prueba", None);
    let (_, cal) = calendar(&mut conn, Kind::Course, "Confirmado");
    let tx = begin(&mut conn).unwrap();
    let e = enrollments::enroll(&tx, &staff(), Kind::Course, &s, &cal).unwrap().uuid;
    tx.commit().unwrap();
    let res = Resource::Enrollment(Kind::Course);
    let view = |c: &Connection| -> (Option<String>, i64) {
        c.query_row("SELECT nota, is_passed FROM v_enrollments_course_lines WHERE uuid = ?1", [&e], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap()
    };
    assert_eq!(view(&conn), (None, 0), "empty -> no letter");
    assert!(matches!(patched(&mut conn, &staff(), res, &e, "puntuacion", "101"), Outcome::Invalid(_)));
    assert!(is_saved(&patched(&mut conn, &staff(), res, &e, "puntuacion", "69")));
    assert_eq!(view(&conn), (Some("D".into()), 0), "69 not passed");
    assert!(is_saved(&patched(&mut conn, &staff(), res, &e, "puntuacion", "70")));
    assert_eq!(view(&conn), (Some("C".into()), 1), "70 C passed");
    assert_eq!((mba_server::school::letter(Some(70)), mba_server::school::passed(Some(70))), (Some("C"), true));
    // The final average sets the score (rounded).
    assert!(is_saved(&patched(&mut conn, &staff(), res, &e, "promedio_final", "91.6")));
    assert_eq!(one::<i64>(&conn, "SELECT puntuacion FROM enrollments_course WHERE uuid = ?1", &e), 92);
    assert!(matches!(patched(&mut conn, &staff(), res, &e, "examen", "100.5"), Outcome::Invalid(_)));
}

// --- T-07-12, T-05-12 (BR-ENR-06, gate) -----------------------------------------------------------------------------

fn xlsx(rows: &[(&str, &[(&str, f64)])]) -> Vec<u8> {
    let mut wb = rust_xlsxwriter::Workbook::new();
    let ws = wb.add_worksheet();
    let headers =
        ["Serial", "Estudiante", "Examen", "Prom. asignaciones", "Asistencia y puntualidad", "Promedio final"];
    for (i, h) in headers.iter().enumerate() {
        ws.write_string(0, i as u16, *h).unwrap();
    }
    for (r, (serial, vals)) in rows.iter().enumerate() {
        let row = r as u32 + 1;
        if let Ok(n) = serial.parse::<f64>() {
            ws.write_number(row, 0, n).unwrap();
        } else if !serial.is_empty() {
            ws.write_string(row, 0, *serial).unwrap();
        }
        for (h, v) in vals.iter() {
            let col = headers.iter().position(|x| x == h).unwrap() as u16;
            ws.write_number(row, col, *v).unwrap();
        }
    }
    wb.save_to_buffer().unwrap()
}

fn enrollments_dump(conn: &Connection) -> Vec<String> {
    seed::dump(conn).unwrap().into_iter().filter(|l| l.starts_with("enrollments_course:")).collect()
}

#[test]
fn t07_12_grades_import_preview_then_apply() {
    let mut conn = seeded();
    let w = staff();
    // Luis (10483) gets new grades; 99999 has no match; an empty serial; an out-of-range grade.
    let file = xlsx(&[
        ("10483", &[("Examen", 82.0), ("Promedio final", 86.4)]),
        ("99999", &[("Examen", 70.0)]),
        ("", &[("Examen", 70.0)]),
        ("10486", &[("Promedio final", 120.0)]),
        ("10482", &[("Examen", 94.0), ("Promedio final", 92.0)]), // Elena: same values -> no change
    ]);
    let rows = grades::read_xlsx(&file).unwrap();
    let before = enrollments_dump(&conn);
    let count_before: i64 = conn.query_row("SELECT COUNT(*) FROM enrollments_course", [], |r| r.get(0)).unwrap();
    let p = grades::preview(&conn, &w, &rows).unwrap();
    assert_eq!(enrollments_dump(&conn), before, "the preview changes nothing");
    assert_eq!(p.changes.len(), 1);
    assert_eq!(p.changes[0].serial, 10483);
    assert_eq!(p.changes[0].new_score, Some(86));
    assert_eq!(p.unchanged.len(), 1);
    let reasons: Vec<&str> = p.skipped.iter().map(|s| s.reason).collect();
    assert_eq!(reasons, ["sin pareja", "vacío", "nota fuera de 0 a 100"]);

    // T-05-12: another user's token, or an expired one, is not found.
    let tx = begin(&mut conn).unwrap();
    assert_eq!(grades::apply(&tx, &admin(), &p.token), Err(DomainError::NotFound));
    drop(tx);
    let tx = begin(&mut conn).unwrap();
    let applied = grades::apply(&tx, &w, &p.token).unwrap();
    tx.commit().unwrap();
    assert_eq!((applied.updated, applied.unmatched), (1, 3));
    let (examen, fin, score): (f64, f64, i64) = conn
        .query_row("SELECT examen, promedio_final, puntuacion FROM enrollments_course WHERE serial = 10483", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!((examen, fin, score), (82.0, 86.4, 86), "Puntuacion follows the final average");
    let nota: String =
        conn.query_row("SELECT nota FROM v_enrollments_course_lines WHERE serial = 10483", [], |r| r.get(0)).unwrap();
    assert_eq!(nota, "B");
    let count_after: i64 = conn.query_row("SELECT COUNT(*) FROM enrollments_course", [], |r| r.get(0)).unwrap();
    assert_eq!(count_after, count_before, "unmatched serials never create enrollments");
    assert_eq!(one::<i64>(&conn, "SELECT COUNT(*) FROM audit_log WHERE action = ?1", "grades.import"), 1);
    let tx = begin(&mut conn).unwrap();
    assert_eq!(grades::apply(&tx, &w, &p.token), Err(DomainError::NotFound), "the token is spent");
    drop(tx);

    let p2 = grades::preview(&conn, &w, &rows).unwrap();
    grades::age_preview_for_tests(&p2.token, grades::PREVIEW_MINUTES + 1);
    let tx = begin(&mut conn).unwrap();
    assert_eq!(grades::apply(&tx, &w, &p2.token), Err(DomainError::NotFound), "expired");
}

#[test]
fn grades_export_round_trip_changes_nothing() {
    let conn = seeded();
    let cal: String = conn
        .query_row("SELECT course_calendar_uuid FROM enrollments_course WHERE serial = 10482", [], |r| r.get(0))
        .unwrap();
    let file = grades::export_xlsx(&conn, Some(&cal)).unwrap();
    let rows = grades::read_xlsx(&file).unwrap();
    assert_eq!(rows.len(), 18, "the 18 enrollments of FHA 9/14/2026");
    let p = grades::preview(&conn, &staff(), &rows).unwrap();
    assert!(p.changes.is_empty() && p.skipped.is_empty(), "{:?}", p.changes);
    assert!(grades::read_xlsx(b"not a zip").is_err());
}

// --- T-07-13 (BR-ENR-08, AP-05) -------------------------------------------------------------------------------------

#[test]
fn t07_13_paid_enrollment_is_cancelled_not_deleted() {
    let mut conn = seeded();
    let s = new_student(&mut conn, "Borrar Prueba", None);
    let (_, cal) = calendar(&mut conn, Kind::Course, "Confirmado");
    let tx = begin(&mut conn).unwrap();
    let paid = enrollments::enroll(&tx, &staff(), Kind::Course, &s, &cal).unwrap().uuid;
    let free = enrollments::enroll(&tx, &staff(), Kind::Course, &s, &cal).unwrap().uuid;
    tx.execute("UPDATE enrollments_course SET pago = 5000 WHERE uuid = ?1", [&paid]).unwrap();
    for who in [staff(), admin()] {
        let e = enrollments::delete_enrollment(&tx, &who, Kind::Course, &paid, true).unwrap_err();
        assert_eq!((e.status().as_u16(), &e), (409, &DomainError::EnrollmentPaid));
    }
    enrollments::cancel_enrollment(&tx, &staff(), Kind::Course, &paid).unwrap();
    assert_eq!(one::<String>(&tx, "SELECT status FROM enrollments_course WHERE uuid = ?1", &paid), "BAJA");
    assert_eq!(enrollments::delete_enrollment(&tx, &staff(), Kind::Course, &free, true), Err(DomainError::Forbidden));
    assert_eq!(
        enrollments::delete_enrollment(&tx, &admin(), Kind::Course, &free, false),
        Err(DomainError::ConfirmRequired)
    );
    enrollments::delete_enrollment(&tx, &admin(), Kind::Course, &free, true).unwrap();
    assert_eq!(one::<i64>(&tx, "SELECT COUNT(*) FROM enrollments_course WHERE uuid = ?1", &free), 0);
    tx.commit().unwrap();
    // A seminar enrollment cancels to Canceló.
    let (_, sc) = calendar(&mut conn, Kind::Seminar, "Confirmado");
    let tx = begin(&mut conn).unwrap();
    let e = enrollments::enroll(&tx, &staff(), Kind::Seminar, &s, &sc).unwrap().uuid;
    enrollments::cancel_enrollment(&tx, &staff(), Kind::Seminar, &e).unwrap();
    assert_eq!(one::<String>(&tx, "SELECT status FROM enrollments_seminars WHERE uuid = ?1", &e), "Canceló");
}

// --- T-07-18 (BR-CRS-03, BR-RPT-02, -03) ----------------------------------------------------------------------------

#[test]
fn t07_18_calendar_counts_and_revenue() {
    let conn = seeded();
    for kind in Kind::ALL {
        let cals: Vec<(String, String)> = conn
            .prepare(&format!("SELECT uuid, {} FROM {}", kind.calendar_offering_col(), kind.calendar_table()))
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(!cals.is_empty(), "{kind:?} seeded");
        for (cal, _) in &cals {
            let row = offerings::calendar_row(&conn, kind, cal).unwrap().unwrap();
            let t = kind.enrollment_table();
            let c = kind.enrollment_calendar_col();
            let n: i64 = one(&conn, &format!("SELECT COUNT(*) FROM {t} WHERE {c} = ?1"), cal);
            let rev: i64 = one(&conn, &format!("SELECT COALESCE(SUM(pago), 0) FROM {t} WHERE {c} = ?1"), cal);
            assert_eq!((row.enrollments, row.revenue_total), (n, rev), "{kind:?} {cal}");
            assert_eq!(row.needed, row.quorum.map(|q| q - n), "BR-CRS-03 needed = quorum - enrollments");
            if kind == Kind::Course {
                let by = |payer: &str| -> i64 {
                    conn.query_row(
                        "SELECT COALESCE(SUM(pago), 0) FROM enrollments_course WHERE course_calendar_uuid = ?1 AND costeado_por = ?2",
                        [cal.as_str(), payer],
                        |r| r.get(0),
                    )
                    .unwrap()
                };
                assert_eq!(
                    (row.revenue_member, row.revenue_individual),
                    (Some(by("COMPAÑIA")), Some(by("INDIVIDUAL")))
                );
                let st = |s: &str| -> i64 {
                    conn.query_row(
                        "SELECT COUNT(*) FROM enrollments_course WHERE course_calendar_uuid = ?1 AND status = ?2",
                        [cal.as_str(), s],
                        |r| r.get(0),
                    )
                    .unwrap()
                };
                assert_eq!((row.count_activo, row.count_baja), (Some(st("ACTIVO")), Some(st("BAJA"))));
            }
        }
    }
    // The design's FHA 9/14/2026 row: 18 enrolled, quorum 12.
    let cal: String = conn
        .query_row("SELECT course_calendar_uuid FROM enrollments_course WHERE serial = 10482", [], |r| r.get(0))
        .unwrap();
    let r = offerings::calendar_row(&conn, Kind::Course, &cal).unwrap().unwrap();
    assert_eq!(
        (r.enrollments, r.quorum, r.status.as_str(), r.first_date.as_deref()),
        (18, Some(12), "Confirmado", Some("2026-09-14"))
    );
}

// --- T-08-11, T-05-07 (AP-07) ---------------------------------------------------------------------------------------

#[test]
fn t08_11_calendar_rows_are_never_deleted_domain() {
    let mut conn = seeded();
    let (_, cal) = calendar(&mut conn, Kind::Course, "Pendiente");
    let tx = begin(&mut conn).unwrap();
    for kind in Kind::ALL {
        let e = offerings::delete_calendar(&tx, &admin(), kind, &cal).unwrap_err();
        assert_eq!((e.status().as_u16(), &e), (405, &DomainError::NoDelete));
    }
    // Cancel instead (BR-CRS-05).
    drop(tx);
    assert!(is_saved(&patched(&mut conn, &staff(), Resource::Calendar(Kind::Course), &cal, "status", "Cancelado")));
}

#[tokio::test]
async fn t08_11_t05_07_no_delete_route() {
    let app = support::App::new();
    let admin = app.admin().await;
    let cal: String = app.conn().query_row("SELECT uuid FROM calendar_course LIMIT 1", [], |r| r.get(0)).unwrap();
    for kind in ["course", "seminar", "short-course"] {
        let req = Request::delete(format!("/calendar/{kind}/{cal}"))
            .header(header::COOKIE, &admin.cookie)
            .header("x-csrf-token", &admin.csrf)
            .body(Body::empty())
            .unwrap();
        let res = app.send(req).await;
        assert!(
            matches!(res.status(), StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED),
            "{kind}: {}",
            res.status()
        );
    }
    assert_eq!(
        app.count("SELECT COUNT(*) FROM calendar_course WHERE uuid = (SELECT uuid FROM calendar_course LIMIT 1)"),
        1
    );
}

// --- Gate: BR-STU-02 (affiliate by key) and BR-AFF-02 ---------------------------------------------------------------

#[test]
fn br_stu_02_affiliate_by_key_copies_the_name() {
    let mut conn = seeded();
    let banco = uuid_of(&conn, "affiliates", "socio", "Banco Ejemplo");
    let acme = uuid_of(&conn, "affiliates", "socio", "Acme Mortgage Demo");
    let s = new_student(&mut conn, "Afiliado Prueba", Some(&banco));
    assert_eq!(one::<String>(&conn, "SELECT compania FROM students WHERE uuid = ?1", &s), "Banco Ejemplo");
    assert!(is_saved(&patched(&mut conn, &staff(), Resource::Student, &s, "member_uuid", &acme)));
    let (m, c): (String, String) = conn
        .query_row("SELECT member_uuid, compania FROM students WHERE uuid = ?1", [&s], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!((m.as_str(), c.as_str()), (acme.as_str(), "Acme Mortgage Demo"));
    assert!(matches!(
        patched(&mut conn, &staff(), Resource::Student, &s, "member_uuid", "no-such-uuid"),
        Outcome::Invalid(_)
    ));
    assert!(is_saved(&patched(&mut conn, &staff(), Resource::Student, &s, "member_uuid", "")));
    let (m, c): (Option<String>, String) = conn
        .query_row("SELECT member_uuid, compania FROM students WHERE uuid = ?1", [&s], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!((m, c.as_str()), (None, "Acme Mortgage Demo"), "unlink keeps the typed company");
    // BR-AFF-02: deleting an affiliate unlinks its students, their company stays; ROLE-ADMIN only.
    let tx = begin(&mut conn).unwrap();
    assert_eq!(affiliates::delete_affiliate(&tx, &staff(), &banco), Err(DomainError::Forbidden));
    let unlinked = affiliates::delete_affiliate(&tx, &admin(), &banco).unwrap();
    assert!(unlinked >= 3);
    let elena: (Option<String>, String) = tx
        .query_row("SELECT member_uuid, compania FROM students WHERE nombre = 'Elena Muestra Ficticia'", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!((elena.0, elena.1.as_str()), (None, "Banco Ejemplo"));
}

// --- Gate: D-06 (no SSN) --------------------------------------------------------------------------------------------

#[test]
fn d06_no_ssn_column_or_field() {
    let conn = seeded();
    let cols: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('students')")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for c in &cols {
        let l = c.to_lowercase();
        assert!(!(l.contains("ssn") || l.contains("social") || l == "ss" || l == "s_s"), "{c}");
    }
    for f in mba_server::school::registries::STUDENT.fields {
        assert!(!f.name.contains("ssn") && !f.label.to_lowercase().contains("seguro social"), "{}", f.name);
    }
    let dump = seed::dump(&conn).unwrap().join("\n").to_lowercase();
    assert!(!dump.contains("seguro social") && !dump.contains("s.s."), "no SSN text in the seed");
}

// --- AP retests on school resources (AP-03..06, AP-08, AP-09) -------------------------------------------------------

#[test]
fn ap_retests_school() {
    let mut conn = seeded();
    let elena = uuid_of(&conn, "students", "nombre", "Elena Muestra Ficticia");
    let fha = uuid_of(&conn, "courses", "course", "Suscripción de Préstamos FHA");
    let tx = begin(&mut conn).unwrap();
    // AP-04: staff 403; admin 409 with dependents.
    assert_eq!(students::delete_student(&tx, &staff(), &elena), Err(DomainError::Forbidden));
    assert!(matches!(students::delete_student(&tx, &admin(), &elena), Err(DomainError::HasEnrollments(n)) if n >= 7));
    assert_eq!(offerings::delete_offering(&tx, &staff(), Kind::Course, &fha), Err(DomainError::Forbidden));
    assert_eq!(offerings::delete_offering(&tx, &admin(), Kind::Course, &fha), Err(DomainError::HasCalendar(5)));
    // AP-08: configuration is admin only.
    assert_eq!(certifications::create_certification(&tx, &staff(), "X", ""), Err(DomainError::Forbidden));
    assert_eq!(certifications::create_professor(&tx, &staff(), "Prof. X"), Err(DomainError::Forbidden));
    let cert: String =
        tx.query_row("SELECT uuid FROM certifications WHERE certification = 'COH'", [], |r| r.get(0)).unwrap();
    assert!(
        matches!(certifications::delete_certification(&tx, &admin(), &cert), Err(DomainError::InUse(_))),
        "graduated rows keep it"
    );
    // An admin deletes a student without enrollments; certification rows cascade.
    let fresh = students::create_student(
        &tx,
        &staff(),
        &students::NewStudent { nombre: "Para Borrar".into(), ..Default::default() },
    )
    .unwrap();
    students::delete_student(&tx, &admin(), &fresh).unwrap();
    assert_eq!(one::<i64>(&tx, "SELECT COUNT(*) FROM student_certifications WHERE student_uuid = ?1", &fresh), 0);
    tx.commit().unwrap();
    // AP-08 on the autosave entry point.
    assert!(matches!(
        patch(&mut conn, &staff(), None, Resource::Certification, &cert, "description", "x"),
        Err(mba_server::error::AppError::Forbidden)
    ));
    assert!(matches!(
        patch(&mut conn, &admin(), None, Resource::Certification, &cert, "description", "Originador Hipotecario")
            .unwrap(),
        Patched::Done(_)
    ));
    // AP-06 / BR-STU-05: interests up to 6, no duplicates.
    let s = new_student(&mut conn, "Intereses Prueba", None);
    let tx = begin(&mut conn).unwrap();
    let certs: Vec<String> = tx
        .prepare("SELECT uuid FROM certifications")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let i1 = students::add_interest(&tx, &staff(), &s, &certs[0]).unwrap();
    assert!(matches!(students::add_interest(&tx, &staff(), &s, &certs[0]), Err(DomainError::Validation(..))));
    for c in &certs[1..4] {
        students::add_interest(&tx, &staff(), &s, c).unwrap();
    }
    let extra: Vec<String> =
        (0..3).map(|k| certifications::create_certification(&tx, &admin(), &format!("E{k}"), "").unwrap()).collect();
    students::add_interest(&tx, &staff(), &s, &extra[0]).unwrap();
    students::add_interest(&tx, &staff(), &s, &extra[1]).unwrap();
    assert_eq!(students::add_interest(&tx, &staff(), &s, &extra[2]), Err(DomainError::LimitReached(6)));
    students::remove_interest(&tx, &staff(), &s, &i1).unwrap();
    students::add_interest(&tx, &staff(), &s, &extra[2]).unwrap();
    tx.commit().unwrap();
    // AP-09: staff may preview an import (the scope is the file's matched serials).
    assert!(grades::preview(&conn, &staff(), &[]).is_ok());
}

// --- Other rules ------------------------------------------------------------------------------------------------

#[test]
fn offerings_rules() {
    let mut conn = seeded();
    let fha = uuid_of(&conn, "courses", "course", "Suscripción de Préstamos FHA");
    let nmls = uuid_of(&conn, "short_courses", "short_course", "Educación Continua NMLS (8 h)");
    let tx = begin(&mut conn).unwrap();
    // BR-CRS-02: Pendiente, prices copied once.
    let c = offerings::open_calendar(&tx, &staff(), Kind::Course, &fha, &Default::default()).unwrap();
    let (st, m, i): (String, i64, i64) = tx
        .query_row("SELECT status, cost_member, cost_individual FROM calendar_course WHERE uuid = ?1", [&c], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!((st.as_str(), m, i), ("Pendiente", 35_000, 42_500));
    // OP-CRS-05: no enrollments -> message.
    assert_eq!(offerings::calendar_enrollments(&tx, Kind::Course, &c), Err(DomainError::NoEnrollments));
    // OP-CRS-03: short course dates up to 15; a seminar takes no extra date.
    let sc = offerings::open_calendar(&tx, &staff(), Kind::ShortCourse, &nmls, &Default::default()).unwrap();
    for d in 1..=15 {
        offerings::add_class_date(&tx, &staff(), Kind::ShortCourse, &sc, &format!("2027-04-{d:02}"), None).unwrap();
    }
    assert_eq!(
        offerings::add_class_date(&tx, &staff(), Kind::ShortCourse, &sc, "2027-04-20", None),
        Err(DomainError::LimitReached(15))
    );
    let hours: f64 =
        tx.query_row("SELECT hours FROM calendar_short_course WHERE uuid = ?1", [&sc], |r| r.get(0)).unwrap();
    assert_eq!(hours, 8.0, "short course hours copied");
    // BR-CRS-04: NMLS total = 8 h x $1.50.
    assert_eq!(offerings::nmls_total(&tx, &nmls).unwrap(), Some(1_200));
    assert_eq!(Kind::from_slug("short-course"), Some(Kind::ShortCourse));
    assert_eq!(Kind::from_slug("x"), None);
}

#[test]
fn students_find_names_and_maps() {
    let conn = seeded();
    let elena = uuid_of(&conn, "students", "nombre", "Elena Muestra Ficticia");
    assert_eq!(students::quick_find(&conn, "zzzz-nadie").unwrap(), students::Found::None);
    assert_eq!(students::quick_find(&conn, "Elena Muestra").unwrap(), students::Found::One(elena.clone()));
    assert_eq!(students::quick_find(&conn, "4821").unwrap(), students::Found::One(elena.clone()));
    assert!(matches!(students::quick_find(&conn, "Banco Ejemplo").unwrap(), students::Found::Many(v) if v.len() > 1));
    assert_eq!(
        students::display_name("Elena Muestra Ficticia", Some("Banco Ejemplo")),
        "Elena Muestra Ficticia (Banco Ejemplo)"
    );
    // EXT-02.
    let url = students::maps_url(&conn, &elena).unwrap().unwrap();
    assert!(url.starts_with("https://www.google.com/maps/search/?api=1&query=Calle+Ficticia+123"), "{url}");
    // The design's academic summary of Elena (BR-STU-06).
    let a = students::academic(&conn, &elena).unwrap();
    assert_eq!(
        (a.average_points, a.total_credits, a.total_hours, a.passed_count, a.is_affiliate),
        (Some(90), 12.0, 180.0, 4, true)
    );
    let balance: i64 = conn
        .query_row("SELECT SUM(balance) FROM v_enrollments_course_lines WHERE student_uuid = ?1", [&elena], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(balance, 57_500, "6 + 1 enrollments, balance $575.00");
}

#[test]
fn registry_columns_exist() {
    let conn = seeded();
    use mba_server::school::registries::*;
    for reg in [
        &STUDENT,
        &STUDENT_CERT,
        &AFFILIATE,
        &COURSE,
        &SEMINAR,
        &SHORT_COURSE,
        &CAL_COURSE,
        &CAL_SEMINAR,
        &CAL_SHORT,
        &DATE_COURSE,
        &DATE_SHORT,
        &ENR_COURSE,
        &ENR_SEMINAR,
        &ENR_SHORT,
        &CERTIFICATION,
        &REQUIREMENT,
        &PROFESSOR,
    ] {
        for f in reg.fields {
            conn.prepare(&format!("SELECT {} FROM {} LIMIT 0", f.column, reg.table))
                .unwrap_or_else(|e| panic!("{}.{}: {e}", reg.table, f.column));
        }
    }
}
