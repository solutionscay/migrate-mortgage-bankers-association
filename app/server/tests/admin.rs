//! UI-29 and UI-30 routes: T-08-16 (last admin, sessions revoked), T-08-20 (users list without secrets), user
//! create and set password (OP-AUTH-04), staff 403 on the new routes, OP-SYS-02 sanitizing, value lists.

#[path = "autosave_support.rs"]
mod support;

use axum::http::StatusCode;
use mba_server::seed;
use support::{body, location, App};

/// T-08-16: the last active admin cannot be demoted or disabled; a role change of a signed-in user removes the
/// user's sessions.
#[tokio::test]
async fn last_admin_is_protected_and_role_change_ends_sessions() {
    let app = App::new();
    let admin = app.admin().await;
    let staff = app.staff().await;
    let me = app.user_id("mvega");

    // Leave mvega as the only active admin.
    app.conn().execute("UPDATE users SET active = 0 WHERE role = 'admin' AND username <> 'mvega'", []).unwrap();

    let res = app.patch(&format!("/admin/users/{me}"), &admin, "role=staff", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = body(res).await;
    assert!(html.contains("último administrador") && html.contains("data-unsaved"), "{html}");
    let res = app.patch(&format!("/admin/users/{me}"), &admin, "active=0", true).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    // The disable dialog (plain form, _method=PATCH) is refused the same way.
    let res = app
        .post_form(&format!("/admin/users/{me}"), Some(&admin), &format!("_csrf={}&_method=PATCH&active=0", admin.csrf))
        .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let (role, active): (String, i64) = app
        .conn()
        .query_row("SELECT role, active FROM users WHERE id = ?1", [me], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!((role.as_str(), active), ("admin", 1));
    assert_eq!(app.get("/admin/users", Some(&admin)).await.status(), StatusCode::OK, "admin still signed in");

    // Promote a signed-in staff user: the user's sessions end.
    let eq = app.user_id("equintana");
    assert_eq!(app.get("/", Some(&staff)).await.status(), StatusCode::OK);
    let res = app.patch(&format!("/admin/users/{eq}"), &admin, "role=admin", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(app.count(&format!("SELECT COUNT(*) FROM sessions WHERE user_id = {eq}")), 0);
    assert_eq!(app.get("/", Some(&staff)).await.status(), StatusCode::SEE_OTHER);

    // With a second active admin, mvega may be demoted.
    let res = app.patch(&format!("/admin/users/{me}"), &admin, "role=staff", true).await;
    assert_eq!(res.status(), StatusCode::OK);
}

/// Disable through the dialog form: 303 to the list, sessions end, the user cannot sign in.
#[tokio::test]
async fn disable_ends_sessions_and_sign_in() {
    let app = App::new();
    let admin = app.admin().await;
    let staff = app.staff().await;
    let eq = app.user_id("equintana");
    let res = app
        .post_form(&format!("/admin/users/{eq}"), Some(&admin), &format!("_csrf={}&_method=PATCH&active=0", admin.csrf))
        .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&res), "/admin/users?hecho=desactivado");
    assert_eq!(app.get("/", Some(&staff)).await.status(), StatusCode::SEE_OTHER);
    assert!(app.sign_in("equintana", seed::DEMO_PASSWORD).await.is_none());
}

/// T-08-20: the users list and the edit row carry no password hash, session token hash or other CSRF token.
#[tokio::test]
async fn users_list_has_no_secrets() {
    let app = App::new();
    let admin = app.admin().await;
    let staff = app.staff().await;
    let res = app.get("/admin/users", Some(&admin)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let mut html = body(res).await;
    for u in ["mvega", "inieves", "equintana", "admin.demo", "staff.demo"] {
        assert!(html.contains(u), "{u} listed");
    }
    html.push_str(&body(app.get(&format!("/admin/users/{}", app.user_id("equintana")), Some(&admin)).await).await);
    let conn = app.conn();
    let mut secrets: Vec<String> = conn
        .prepare("SELECT password_hash FROM users")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    secrets.extend(
        conn.prepare("SELECT token_hash FROM sessions")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap),
    );
    secrets.push(staff.csrf.clone());
    for s in &secrets {
        assert!(!html.contains(s.as_str()));
    }
    assert!(!html.contains("$argon2"));
}

/// OP-AUTH-04: create (dialog with Guardar), then the new user signs in; set password closes the user's sessions.
#[tokio::test]
async fn create_user_and_set_password() {
    let app = App::new();
    let admin = app.admin().await;

    let bad = format!("_csrf={}&username=X&display_name=&role=staff&active=1&nueva=corta&repetir=otra", admin.csrf);
    let res = app.post_form("/admin/users", Some(&admin), &bad).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = body(res).await;
    assert!(html.contains("data-open=\"dlg-nuevo\"") && html.contains("No se creó el usuario."));

    let ok = format!(
        "_csrf={}&username=lrivera&display_name=Luis+Rivera&role=staff&active=1&nueva=clave-nueva-1&repetir=clave-nueva-1",
        admin.csrf
    );
    let res = app.post_form("/admin/users", Some(&admin), &ok).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&res), "/admin/users?hecho=creado");
    let dup = app.post_form("/admin/users", Some(&admin), &ok).await;
    assert_eq!(dup.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let first = app.sign_in("lrivera", "clave-nueva-1").await.expect("new user signs in");
    assert_eq!(app.get("/admin/users", Some(&first)).await.status(), StatusCode::FORBIDDEN);
    assert_eq!(app.count("SELECT COUNT(*) FROM audit_log WHERE action = 'auth.user_created'"), 1);

    let id = app.user_id("lrivera");
    let res = app
        .post_form(
            &format!("/admin/users/{id}/password"),
            Some(&admin),
            &format!("_csrf={}&nueva=otra-clave-22&repetir=otra-clave-22", admin.csrf),
        )
        .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(app.get("/", Some(&first)).await.status(), StatusCode::SEE_OTHER, "old session closed");
    assert!(app.sign_in("lrivera", "clave-nueva-1").await.is_none());
    assert!(app.sign_in("lrivera", "otra-clave-22").await.is_some());
}

/// AP-08 / T-05-05: ROLE-STAFF gets 403 on every new admin route, and nothing changes.
#[tokio::test]
async fn staff_is_refused_on_admin_routes() {
    let app = App::new();
    let staff = app.staff().await;
    let me = app.user_id("equintana");
    for path in ["/admin/users", "/admin/settings", "/admin/templates/mba.sponsor_letter", "/admin/value-lists"] {
        assert_eq!(app.get(path, Some(&staff)).await.status(), StatusCode::FORBIDDEN, "{path}");
    }
    assert_eq!(
        app.patch(&format!("/admin/users/{me}"), &staff, "role=admin", true).await.status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(app.patch("/admin/settings", &staff, "org.name=X", true).await.status(), StatusCode::FORBIDDEN);
    let res = app
        .post_form(
            &format!("/admin/users/{me}/password"),
            Some(&staff),
            &format!("_csrf={}&nueva=x&repetir=x", staff.csrf),
        )
        .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let role: String = app.conn().query_row("SELECT role FROM users WHERE id = ?1", [me], |r| r.get(0)).unwrap();
    assert_eq!(role, "staff");
}

/// OP-SYS-02 (D-18): the body is sanitized with ammonia; merge fields stay. More in `tests/templates.rs`.
#[tokio::test]
async fn template_body_is_sanitized() {
    let app = App::new();
    let admin = app.admin().await;
    let page = body(app.get("/admin/settings", Some(&admin)).await).await;
    assert!(page.contains("/admin/templates/mbs.graduation_certificate") && page.contains("tipo_de_pago"));
    let res = app
        .patch(
            "/admin/templates/mbs.graduation_certificate",
            &admin,
            "body_html=%3Cp+onclick%3D%22x()%22%3EHola+%7B%7Bnombre%7D%7D%3C%2Fp%3E%3Cscript%3Ealert(1)%3C%2Fscript%3E",
            true,
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body(res).await;
    assert!(html.contains("rte-source") && !html.contains("hx-swap-oob"));
    let stored: String = app
        .conn()
        .query_row("SELECT body_html FROM document_templates WHERE code = 'mbs.graduation_certificate'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(stored, "<p>Hola {{nombre}}</p>");
    assert_eq!(app.get("/admin/templates/no-such", Some(&admin)).await.status(), StatusCode::NOT_FOUND);
}

/// `POST /admin/value-lists`: add and move; a value a CHECK constraint names cannot be renamed.
#[tokio::test]
async fn value_lists_add_move_and_locked_rename() {
    let app = App::new();
    let admin = app.admin().await;
    let csrf = &admin.csrf;
    let res = app
        .post_form(
            "/admin/value-lists",
            Some(&admin),
            &format!("_csrf={csrf}&op=add&list=tipo_de_pago&new_value=ATH+M%C3%B3vil"),
        )
        .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let order: i64 =
        app.count("SELECT sort_order FROM value_list_items WHERE list = 'tipo_de_pago' AND value = 'ATH Móvil'");
    assert_eq!(order, 6);
    app.post_form(
        "/admin/value-lists",
        Some(&admin),
        &format!("_csrf={csrf}&op=move&dir=up&list=tipo_de_pago&value=ATH+M%C3%B3vil"),
    )
    .await;
    assert_eq!(
        app.count("SELECT sort_order FROM value_list_items WHERE list = 'tipo_de_pago' AND value = 'ATH Móvil'"),
        5
    );
    // The new value is not in any CHECK, so it can be renamed; VISA is, so it cannot.
    app.post_form(
        "/admin/value-lists",
        Some(&admin),
        &format!("_csrf={csrf}&op=rename&list=tipo_de_pago&value=ATH+M%C3%B3vil&new_value=ATH"),
    )
    .await;
    assert_eq!(app.count("SELECT COUNT(*) FROM value_list_items WHERE list = 'tipo_de_pago' AND value = 'ATH'"), 1);
    let res = app
        .post_form(
            "/admin/value-lists",
            Some(&admin),
            &format!("_csrf={csrf}&op=rename&list=tipo_de_pago&value=VISA&new_value=Visa"),
        )
        .await;
    assert!(location(&res).ends_with("error=bloqueado"));
    assert_eq!(app.count("SELECT COUNT(*) FROM value_list_items WHERE list = 'tipo_de_pago' AND value = 'VISA'"), 1);
    assert_eq!(app.get("/admin/value-lists?lista=tipo_de_pago", Some(&admin)).await.status(), StatusCode::OK);
}
