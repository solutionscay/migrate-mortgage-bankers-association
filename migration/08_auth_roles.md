# Identity and Authorization Contract

Scope: one web app, one SQLite database, three modules (D-01). Local sign-in: username + argon2id password, server session cookie, CSRF token on forms; no SSO, no MFA (D-50). Demo build: the real FileMaker accounts are not mapped. Each active source account has a dummy account (fake name and username), and the seed tool creates those dummy users (D-07, D-02).

Labels: **fact** = read from the object file cited; **product decision** = operator answer or Step 5 default (D-id given); **inference** = drawn from structure.

Forward ids were filled in the reconcile pass (task 3c868fb3): routes and EXT- ids are in `05_api_design.md`, screens UI-nn in `06_ui_spec.md`. Operations OP- and rules BR- are in `07_business_logic.md`. Tables are in `04_database_schema.sql`. The full table-by-role matrix is `08_auth_roles_appendix.md`.

## Authority and evidence

| Source | Count | Authority/use | Gaps |
|---|---|---|---|
| `accounts`, `privilege-sets` object files | 10 accounts, 9 privilege sets (3 standard sets per file) | Primary identity/authorization catalog | Password hashes absent (DDR has none). Real accounts not mapped; dummy accounts stand in (D-07, demo). |
| Record/field/layout privileges in privilege-set files | 9 | Primary enforcement evidence. Every set is a FileMaker standard set: `[Full Access]` = all; `[Data Entry Only]` = Records CreateEditDelete, Layouts and ValueLists ViewOnly, Scripts ExecutableOnly; `[Read-Only Access]` = Records ViewOnly. No custom record rule, no field rule. | none |
| Script/value-list privileges in privilege-set files | 9 | Operation/lookup evidence: all scripts executable by every set; 0 scripts with `runFullAccess=True` (212 of 212 False) | none |
| `extended-privileges` object files | 18 (6 per file) | Access-channel evidence: only `fmapp` and `fmreauthenticate10` are granted | none |
| Layout explorer (hide and format rules) | 0 hide rules, 11 format lines | Secondary UI evidence; no security decision found | high-bit object flags undecoded (D-27) |
| Menu explorer (removed or gated commands) | 111 menus, 15 sets, `menu=All` in every set | Secondary UI evidence; no menu is role-gated | none |
| Discovery/IdP policy | D-50, D-07, D-08, D-09, D-40 | Target product decision | D-08 and D-09 not answered; Step 5 defaults used |

Hide-object formulas are not used as policy. No ownership column exists in the source; no record-ownership predicate is invented.

## Identity provisioning

Account names: this plan does not print the real FileMaker account names. Each active source account has a dummy account with a fake person name and username. Neither comes from the DDR. The dummy keeps the privilege set and status of the source account in each file. The built-in `[Guest]` account keeps its name.

How to read an account citation: `objects/<file>/accounts/<id>-<account>.txt`. `<id>` is the FileMaker account id in that file. `<account>` stands for the real account name, which stays in the private object file. Example: `objects/MBS.fmp12/accounts/3-<account>.txt` is account id 3 of MBS, the dummy `Inieves` below.

| Source account/category | Status/auth type | Target identity/IdP mapping | Invite/reset/cutover | Role assignment owner | Audit/exception |
|---|---|---|---|---|---|
| `mvega` (Marisol Vega Ortiz): account id 2 in MBA, MBS and MBS_SendMail, `[Full Access]` in each (`objects/MBA.fmp12/accounts/2-<account>.txt`, `objects/MBS.fmp12/accounts/2-<account>.txt`, `objects/MBS_SendMail.fmp12/accounts/2-<account>.txt`) | active, FileMaker auth | seeded demo user `mvega` with ROLE-ADMIN (demo, D-07) | the seed tool sets the first password and prints it once; no e-mail invite (D-51) | ROLE-ADMIN | `auth.user_created` |
| `inieves` / `Inieves` (Ivelisse Nieves Arroyo): account id 3 in MBA and MBS, `[Full Access]`; account id 5 in MBS_SendMail, `[Data Entry Only]`; MBS spells the username `Inieves` (`objects/MBA.fmp12/accounts/3-<account>.txt`, `objects/MBS.fmp12/accounts/3-<account>.txt`, `objects/MBS_SendMail.fmp12/accounts/5-<account>.txt`) | active, FileMaker auth | seeded demo user `inieves` with ROLE-ADMIN (D-08 default). The target has one role per user, so the MBS_SendMail `[Data Entry Only]` set is not kept (D-84, out of scope). Usernames are case-insensitive in the target (`users.username COLLATE NOCASE`), as in FileMaker. | as above | ROLE-ADMIN | as above |
| `equintana` (Edwin Quintana Roldán): account id 4, MBS_SendMail only, `[Data Entry Only]` (`objects/MBS_SendMail.fmp12/accounts/4-<account>.txt`) | active, shared | seeded demo user `equintana` with ROLE-STAFF | as above | ROLE-ADMIN | as above |
| `[Guest]`: account id 1 in each file, `[Read-Only Access]` (`objects/<file>/accounts/1-<account>.txt`) | inactive | dropped | n/a | n/a | n/a |
| Credential-passing login scripts: `objects/MBS.fmp12/scripts/8-login_user_password.txt`, `objects/MBS_SendMail.fmp12/scripts/16-g.login_user_pass.txt` | take account and password as a script parameter | not rebuilt (D-50) | n/a | n/a | n/a |
| Service principal | none in the source | `RESEND_API_KEY` in the server environment is the only service credential (D-05) | n/a | operator | never rendered or logged |

Demo seed users (`mba-seed`, OP-SYS-03; also listed in the users seed comment of `04_database_schema.sql`):

| Username | Display name | Role | Source account (FileMaker id per file, privilege set) |
|---|---|---|---|
| `mvega` | Marisol Vega Ortiz | ROLE-ADMIN | MBA 2, MBS 2, MBS_SendMail 2: `[Full Access]` |
| `inieves` | Ivelisse Nieves Arroyo | ROLE-ADMIN | MBA 3, MBS 3: `[Full Access]`; MBS_SendMail 5: `[Data Entry Only]` (D-84) |
| `equintana` | Edwin Quintana Roldán | ROLE-STAFF | MBS_SendMail 4: `[Data Entry Only]` |

`email` is NULL. Each password is random, printed once by the seed tool, and never stored in the repository.

The DDR has no password or hash; no password migration. Disabled users keep their row (`users.active = 0`) and cannot sign in. Deprovisioning = disable; delete is not offered (audit rows keep `user_id`). Break-glass: the CLI seed tool can reset the admin password (OP-SYS-03). MFA: none (D-50).

## Roles and capabilities

| Source privilege set/rule | Target role/capability | Permission boundary | Explicit denies | Source evidence | Approval owner |
|---|---|---|---|---|---|
| `[Full Access]` (3 files) | ROLE-ADMIN (`users.role = 'admin'`) | everything ROLE-STAFF can do, plus: hard delete of parent records (AP-04), void payments (AP-05), configuration (AP-08), user management (AP-11) | none inside the app; cannot read `RESEND_API_KEY` (AP-13) | `objects/MBA.fmp12/privilege-sets/1-Full_Access.txt` (manageAccounts=True, overrideValidationWarning=True); same in MBS and MBS_SendMail | operator (demo) |
| `[Data Entry Only]` (3 files) | ROLE-STAFF (`users.role = 'staff'`) | create, view, edit every domain record in the three modules; print and export; run the domain operations of 07 | hard delete of parent records, payment void, configuration, user management (AP-04, AP-05, AP-08, AP-11) | `objects/MBA.fmp12/privilege-sets/2-Data_Entry_Only.txt` (Records CreateEditDelete, manageAccounts=False, exporting=True, printing=True) | operator |
| `[Read-Only Access]` (3 files) | none: retired | n/a | n/a | `objects/MBA.fmp12/privilege-sets/3-Read-Only_Access.txt`; only the inactive `[Guest]` uses it and it lacks `fmapp` | D-08 default |

Default deny: a request with no valid session, or a role without an allow rule below, is refused. ROLE-STAFF is narrower than `[Data Entry Only]` on deletes; this is a deliberate change (D-09 default). Staff work in both modules (D-08 default).

## Entity operation matrix

Records are single-file in the source; each rule comes from the file that holds the table. A rule on a parent does not limit child tables except where the row says so. Full table-by-role matrix: `08_auth_roles_appendix.md`.

| Entity/domain | Role/capability | Create | View/query | Edit | Delete | Bulk/export/admin | Enforcement owner |
|---|---|---|---|---|---|---|---|
| REG: `registration` | ROLE-STAFF | allow | allow | allow | conditional: only without payments; else cancel (AP-05) | export/print allow (AP-10) | axum handler + 07 operation |
| REG: `registration` | ROLE-ADMIN | allow | allow | allow | conditional: as staff | allow | same |
| REG: `registration_items`, `registration_guests` | both | allow | allow | allow | allow after confirm (AP-06) | plan rebuild = bulk delete inside OP-REG-03 (AP-09) | same |
| REG: `payments` | ROLE-STAFF | allow | allow | edit until voided | deny (AP-05) | none | same |
| REG: `payments` | ROLE-ADMIN | allow | allow | edit until voided | deny; void with reason (AP-05) | none | same |
| EVT: `events`, `packages` | ROLE-STAFF | allow | allow | allow | deny (AP-04) | print allow | same |
| EVT: `events`, `packages` | ROLE-ADMIN | allow | allow | allow | conditional: no registrations (AP-04) | allow | same |
| EVT: `event_items`, `package_items` | both | allow | allow | allow | conditional: no registration item references it (FK) (AP-06) | none | same + FK |
| SPN: `sponsor`, `billing` | both | allow | allow | allow | allow after confirm (AP-06) | print allow | same |
| PPL: `people` | ROLE-STAFF / ROLE-ADMIN | allow | allow | allow | staff deny; admin conditional: no registrations (AP-04) | export allow | same |
| AFF: `affiliates` | ROLE-STAFF / ROLE-ADMIN | allow | allow | allow | staff deny; admin allow (students unlinked, BR-AFF-02) (AP-04) | print allow | same |
| STU: `students` | ROLE-STAFF / ROLE-ADMIN | allow | allow | allow | staff deny; admin conditional: no enrollments (AP-04) | print allow | same |
| CRS: `courses`, `seminars`, `short_courses` | ROLE-STAFF / ROLE-ADMIN | allow | allow | allow | staff deny; admin conditional: no calendar rows or enrollments (AP-04) | none | same |
| CRS: `calendar_course`, `calendar_short_course`, `calendar_seminar` | both | allow | allow | allow (cancel by status) | deny for every role (AP-07) | e-mail to enrolled students (OP-MAIL-01) | same |
| CRS: calendar date rows | both | allow | allow | allow | allow after confirm (AP-06) | none | same |
| CRS: `professors` | ROLE-ADMIN | allow | allow (both roles view) | allow | conditional: not referenced | none | same (AP-08) |
| ENR: three enrollment tables | both | allow | allow | allow | conditional: only when `pago` = 0, after confirm; else cancel (AP-05) | grades export (AP-10), grades import (AP-09) | same |
| CRT: `certifications`, `certification_requirements` | ROLE-ADMIN | allow | allow (both roles view) | allow | admin conditional: no student progress rows | none | same (AP-08) |
| CRT: `student_certifications` | both | system-created (BR-STU-01) | allow | allow (graduation date, note) | cascade with the student only | certificate print (AP-10) | same |
| MAIL: `mail_batches`, `mail_emails` | both | allow | allow | allow while draft | allow while no e-mail is sent (AP-06) | send all (AP-09) | same |
| MAIL: `mail_templates`, `mail_profiles` | ROLE-ADMIN | allow | allow (both roles view) | allow | allow when unused | none | same (AP-08) |
| MAIL: `mail_outbox` | both | system only | allow (AP-13) | deny | deny | none | same |
| SYS: `app_settings`, `document_templates`, `value_list_items` | ROLE-ADMIN | allow | allow (both roles read through pages) | allow | deny (settings), allow (template reset) | none | same (AP-08) |
| AUTH: `users` | ROLE-ADMIN | allow | allow | allow (role, active, password) | deny (disable instead) | none | same (AP-11) |
| AUTH: `sessions`, `audit_log` | system | system | admin view of `audit_log` only | deny | deny | none | same |

## Record-level policies

| Policy id | Entity/operation | Exact source predicate | Target predicate and fields | Null/edge semantics | Enforcement owner | Tests |
|---|---|---|---|---|---|---|
| AP-01 | every route except `/login` and static files | `fmapp` network access + account sign-in (`objects/MBA.fmp12/extended-privileges/3-fmapp.txt`); `fmreauthenticate10` (`objects/MBA.fmp12/extended-privileges/4-fmreauthenticate10.txt`) | valid row in `sessions` (by SHA-256 of the cookie token), `users.active = 1`, `last_seen_at` within `app_settings.session.idle_minutes` (10), `expires_at` not passed | missing or unknown cookie -> redirect to `/login` (HTML) or 401 (HTMX) | axum middleware before every handler | T-08-01, T-08-02, T-08-03 |
| AP-02 | every POST, PUT, PATCH, DELETE, including HTMX | none in source (product decision, D-50) | form field or `HX-` header token equals `sessions.csrf_token`; cookie `HttpOnly`, `Secure`, `SameSite=Lax` | missing token -> 403 | axum middleware | T-08-04 |
| AP-03 | create, view, edit of domain records (all modules) | `[Data Entry Only]` Records value=CreateEditDelete on all tables | `users.role IN ('admin', 'staff')` | none | handler guard `require_role(Staff)` | T-08-05 |
| AP-04 | hard delete of parent records: `people`, `students`, `events`, `packages`, `affiliates`, `courses`, `seminars`, `short_courses`, `certifications` | Source: both sets may delete; FileMaker cascades (relationships MBA 1, 4, 5, 7; MBS 2, 18, 25, 30) | `users.role = 'admin'` AND no dependent rows: no `registration` for a person, event or plan; no enrollment for a student or offering; no calendar row for an offering; no `student_certifications` for a certification | dependent rows present -> 409 with the count | handler guard + pre-check in the same transaction; FK `NO ACTION` as backstop | T-08-06, T-08-07 |
| AP-05 | rows that carry money: `payments`, `registration`, enrollments | Source: delete allowed, cascades (relationship MBA 4 `cDelete`); `gEnrollment.deleteRecord` deletes after confirm (`objects/MBS.fmp12/scripts/165-gEnrollment.deleteRecord.txt`) | payments: no delete for any role; void = `users.role = 'admin'` + non-empty reason -> `voided_at/by/void_reason`. Registration: delete only when no non-voided payment exists, else status Canceló. Enrollment: delete only when `pago` = 0, else status change. | `pago` NULL is treated as 0 (column is NOT NULL DEFAULT 0) | handler guard | T-08-08, T-08-09 |
| AP-06 | child rows without money: `registration_items`, `registration_guests`, `package_items`, `event_items`, calendar date rows, `student_certification_interests`, `sponsor`, `billing`, draft `mail_emails`/`mail_batches` | `[Data Entry Only]` CreateEditDelete | `users.role IN ('admin', 'staff')` + confirmation step in the UI; FK refuses when referenced | e-mail rows: only while `sent = 0` for every e-mail of the batch | handler guard + FK | T-08-10 |
| AP-07 | delete of `calendar_course`, `calendar_short_course`, `calendar_seminar` | every Borrar button passes access `0` to `g.deletePortalRow`, so delete is always denied (`objects/MBS.fmp12/scripts/164-g.deletePortalRow_access.txt`; `objects/MBS.fmp12/layouts/57-Cursos_Especializados.txt`) | no delete route exists; status Cancelado instead | none | no handler | T-08-11 |
| AP-08 | configuration: `users`, `app_settings`, `document_templates`, `value_list_items`, `certifications`, `certification_requirements`, `professors`, `mail_templates`, `mail_profiles` | `manageAccounts=True` only in `[Full Access]`; the rest is a product decision (D-19: admin screens) | `users.role = 'admin'` for create, edit, delete; staff may read these values where a page shows them | none | handler guard `require_role(Admin)` | T-08-12 |
| AP-09 | bulk operations: plan rebuild (OP-REG-03), send all (OP-MAIL-04), grades import (OP-ENR-07) | no restriction in source (Scripts ExecutableOnly for every set) | `users.role IN ('admin', 'staff')`; confirm step (rebuild, send all) or preview step (import); scope fixed by the server (registration uuid, batch uuid, matched serials), never by a client list of ids | empty scope -> no-op with a message | handler guard + server-side scope | T-08-13 |
| AP-10 | print pages and exports (CSV, XLSX) | `exporting=True`, `printing=True` in every set | `users.role IN ('admin', 'staff')`; the export applies the same filters as the list | none | handler guard | T-08-14 |
| AP-11 | sign-in, disable, role change | `[Guest]` inactive (fact); `manageAccounts` only Full Access | inactive user cannot sign in; 5 failed sign-ins lock for 15 minutes (`failed_login_count`, `locked_until`); disable or role change deletes the user's `sessions` rows; an admin cannot disable or demote the last active admin | locked: same generic error as a bad password | handler + `users` columns | T-08-15, T-08-16 |
| AP-12 | change own password | `allowModifyPassword=True` in every set | current password required; new hash argon2id | none | handler | T-08-17 |
| AP-13 | outbox and mail secrets | SMTP password and licence key in clear text in `objects/MBS_SendMail.fmp12/scripts/10-g.smtpConnect.txt` and `9-g.pluginRegister.txt` (values not copied) | both roles view `mail_outbox`; `RESEND_API_KEY` exists only in the server environment and is never rendered, logged or stored | no key -> mock provider (D-05) | config loader + templates | T-08-18 |
| AP-14 | external access channels | `fmwebdirect`/`fmiwp`, `fmxdbc`, `fmxml`, `fmphp` held by no set (`objects/*/extended-privileges/`) | no public API, ODBC, XML or PHP channel; the seed tool runs only from the server CLI | none | no route | T-08-19 |

No source predicate refers to a record owner, so no ownership filter exists. Every predicate above uses real columns of `04_database_schema.sql`.

## Field-level policies

| Entity/field | Role/capability | Read | Create/write | Update | Masking/omission behavior | Source restriction | Tests |
|---|---|---|---|---|---|---|---|
| `users.password_hash` | all | never serialized | server only (argon2id) | server only (AP-12, AP-11) | omitted from every page and export | none (product decision) | T-08-20 |
| `sessions.token_hash`, `sessions.csrf_token` | all | never serialized (CSRF token only into the user's own forms) | server only | server only | omitted | none | T-08-20 |
| `payments.voided_at`, `voided_by`, `void_reason` | ROLE-ADMIN write; both read | allow | admin only (AP-05) | admin only, once | none | none | T-08-08 |
| `students` SSN | n/a | n/a | n/a | n/a | no column exists (D-06) | `objects/MBS.fmp12/tables/131-students.txt` #19 `S.S.` shown on `objects/MBS.fmp12/layouts/45-Tabla_de_Estudiantes.txt` | T-08-21 |
| `RESEND_API_KEY`, SMTP and licence values | n/a | n/a | n/a | n/a | not in the database (AP-13) | scripts 9, 10 | T-08-18 |

No FileMaker privilege set restricts a field; the rows above are target additions.

## Operation, script, and value-list access

| Target operation/lookup | Source script/value-list evidence | Allowed capabilities | Record/field dependency | Deny behavior | Tests |
|---|---|---|---|---|---|
| OP-REG-01..07, OP-REG-09 (cancel), OP-REG-10 | MBA scripts 21, 33, 35, 37, 38, 39, 41, 42, 44, 45 | ROLE-STAFF, ROLE-ADMIN (AP-03, AP-06, AP-09) | registration delete: AP-05 | 403 page / HTMX error toast | T-08-05 |
| OP-REG-08 void payment | product decision (D-09) | ROLE-ADMIN (AP-05) | reason required | 403 | T-08-08 |
| OP-EVT-01..05, OP-SPN-01..02, OP-PPL-01 | MBA scripts 13, 15, 22, 25, 26, 48, 49, 50 | both (AP-03, AP-06) | parent delete: AP-04 | 403 / 409 | T-08-06 |
| OP-PPL-02, OP-STU-04, OP-AFF-01 (delete) | custom menus `people.records`, `Records_Students` | ROLE-ADMIN (AP-04) | dependents | 409 | T-08-07 |
| OP-STU-01..03, OP-STU-05 | MBS scripts 33, 34, 35, 45 | both (AP-03) | none | 403 | T-08-05 |
| OP-CRS-01..03, OP-CRS-05 | MBS scripts 52, 54, 58, 60, 64, 65 | both (AP-03); calendar delete: none (AP-07) | none | no route | T-08-11 |
| OP-CRS-04, OP-CRT-01, OP-MAIL-06, OP-MAIL-07, OP-SYS-01, OP-SYS-02, OP-AUTH-04 | layouts MBS 59, 66, 165; MBS_SendMail 12 (no screen reaches them, D-19) | ROLE-ADMIN (AP-08) | none | 403 | T-08-12 |
| OP-ENR-01..05, OP-ENR-08 | MBS scripts 37, 38, 39, 165, 166 | both (AP-03, AP-05) | delete: `pago` = 0 | 409 | T-08-09 |
| OP-ENR-06, OP-RPT-01..05, OP-CRT-03 | print and export scripts (07 appendix §1) | both (AP-10) | none | 403 | T-08-14 |
| OP-ENR-07 grades import, OP-MAIL-04 send all | MBS 128; MBS_SendMail 18 | both (AP-09) | server-side scope | 403 | T-08-13 |
| OP-MAIL-01..03, OP-MAIL-05 | MBS 173; MBS_SendMail 1, 7, 8, 11, 17, 21 | both (AP-03) | edit only while draft | 409 | T-08-10 |
| OP-MAIL-08 outbox | product decision (D-05) | both (AP-13) | none | 403 | T-08-18 |
| OP-AUTH-01..03 | Re-Login scripts (MBA 2, MBS 2, MBS_SendMail 20) | any active user (AP-01, AP-11, AP-12) | none | generic sign-in error | T-08-15 |
| OP-SYS-03 seed tool | product decision (D-02) | server CLI only (AP-14) | none | no web route | T-08-19 |
| Value-list lookups (all 36 lists) | `ValueLists value=ViewOnly` for Data Entry, `all` for Full Access | read: both roles; edit of `value_list_items`: ROLE-ADMIN (AP-08) | related-only filters apply (07 BR-CRS-01) | 403 on edit | T-08-12 |

## Route/layout and access-channel matrix

| Route/page/channel | Source layout/extended privilege | Allowed capabilities | Underlying operations/data rules | Direct-access test |
|---|---|---|---|---|
| Web app over HTTPS (all pages; `05` "Route catalog", 150 routes) | `fmapp` (MBA: Full Access only; MBS and MBS_SendMail: Full Access and Data Entry) | ROLE-ADMIN, ROLE-STAFF | AP-01, AP-02 | T-08-01 |
| Session re-authentication | `fmreauthenticate10` (all sets) | all | AP-01: idle 10 min | T-08-03 |
| Convention pages (events, people, registrations, plans, sponsors) UI-03 to UI-13 | MBA layouts 25, 29, 35, 40, 45, 98, 99 | both | REG, EVT, SPN, PPL operations | T-08-05 |
| School pages (students, affiliates, offerings, enrollments, reports) UI-14 to UI-24 | MBS layouts 42, 43, 48, 57, 60, 64, 82, 103, 113, 137 | both | STU, AFF, CRS, ENR, CRT, RPT operations | T-08-05 |
| Mail pages (batches, e-mails, outbox) UI-25 to UI-28 | MBS_SendMail layouts 4, 8, 9 | both | MAIL operations | T-08-18 |
| Admin pages (users, settings, templates, certifications, professors, profiles) UI-29 to UI-32 (`/admin/*`) | MBS 59, 66, 165; MBS_SendMail 12 | ROLE-ADMIN | AP-08 | T-08-12 |
| Print and export routes `/print/convention/{doc}`, `/print/school/{doc}`, `/reports/{code}`, `/dashboard/enrollments`, `/exports/*` (UI-23, UI-24, UI-34, UI-35) | 81 report-print layouts (72 print pages; `05_api_design_appendix.md` §1) | both | AP-10 | T-08-14 |
| WebDirect / IWP, ODBC/JDBC, XML, PHP | `fmwebdirect`, `fmiwp`, `fmxdbc`, `fmxml`, `fmphp`: held by no set | none: retired | AP-14 | T-08-19 |

Route guards and hidden navigation only supplement AP-03..AP-14, which the handlers check on every request.

## UI visibility and exceptions

| Layout file | Object key | Condition | Related server policy | Target UI behavior | Unresolved risk |
|---|---|---|---|---|---|
| `objects/MBS.fmp12/layouts/57-Cursos_Especializados.txt`, `60-Seminarios.txt`, `64-Cursos_Cortos.txt` | Borrar buttons | always pass access 0 (a commented-out parameter allowed delete when status = Pendiente) | AP-07 | no delete button on calendar rows | none |
| `objects/MBS.fmp12/layouts/45-Tabla_de_Estudiantes.txt` | `students::S.S.` column | none: visible to every user | no SSN column (D-06) | column absent | none |
| Admin-only actions on shared pages | delete, void, configuration links | `users.role = 'admin'` | AP-04, AP-05, AP-08 | hidden for ROLE-STAFF; server still refuses a direct request | none |
| Layouts with high-bit object flags (MBA 98, MBS 43, 82, 103, 113, 172, MBS_SendMail 4) | buttons, portals | unknown | none | treated as visible and active (D-27) | a hidden or locked control could become visible; no data exposure found |

No hide rule is the only control on sensitive data.

## Session and global state

| FM global/`$$` value | Observed initialization/read/write | Target scope | Trusted for authorization? | Server recomputation/validation |
|---|---|---|---|---|
| `$$registration_uuid` (MBA scripts 44, 45) | set before the plan picker, read by the select script, cleared at the end | per-request: registration uuid in the route | no | server loads the registration and checks AP-03 |
| `$$windowLeft1` etc. (MBA, MBS scripts 5, 6) | window geometry | none (retired) | no | none |
| `globals::g_text1`, `g_text2` (MBA script 37) | dialog input buffers | form fields of one request | no | validated per BR-REG-06 |
| `globals::qty`, `numero`, `g_events`, `g_boolean` (MBA scripts 81, 82, 85, 95) | label quantity, pivot options | form fields | no | quantity range check |
| `globals::g_number`, `g_date_range`, `g_courses_left`, `g_certification_uuids`, `g_report_years`, `date_range`, `status1-3` (MBS reports) | report criteria; `globals_start` fills report years | per-request filter parameters (D-33) | no | parsed and bounded server-side |
| `globals::MBS Logo`, `logo_stored` | logo containers | durable setting `app_settings.logo_file_uuid` | no | admin edit only (AP-08) |
| `courses::student_uuid` (global) | transcript context (MBS script 91) | route parameter | no | AP-03 |
| `MBS_SendMail globals::contact_ids` | contact list between files | server-side query of the calendar's enrollments (BR-MAIL-07) | no | never a client-supplied list |
| `Get ( AccountName )` in `log_*_by` fields | creation and modification account | `created_by` / `modified_by` = `users.username` from the session | no (audit only) | set by the server |

## Privilege bypass and application-level permissions

| Source | Evidence | What it bypasses or adds | Target policy |
|---|---|---|---|
| Scripts with `runFullAccess=True` | none: 212 of 212 scripts are `runFullAccess=False` (`# options:` header of each script file) | nothing | none needed |
| Permission flags stored as data | none found in 43 tables | nothing | none |
| Commands removed by custom menus | `MBA registrations.records` (`objects/MBA.fmp12/custom-menus/40-registrations.records.txt`) has no New Record; `MBS Records_Course_Enrollments` (`objects/MBS.fmp12/custom-menus/49-Records_Course_Enrollments.txt`) has no New, Duplicate or Delete All; menus are not role-gated (`menu=All`) | UI shape only | registrations are created from a person (OP-REG-01); enrollments from a student (OP-ENR-01..03); no "delete all" route (AP-09 scope) |
| Delete denied by script parameter | `objects/MBS.fmp12/scripts/164-g.deletePortalRow_access.txt` (access 0 from every caller) | denies calendar deletes for every account | AP-07 |
| File access authorization (`auth-files`) | `objects/MBA.fmp12/auth-files/` (and MBS, MBS_SendMail): `requireAuthorization=False` | any file may open these files | n/a: one app, one database |

## Service principals and integrations

| Principal/integration | Direction | Allowed operations/data | Secret owner/rotation | Network/access channel | Audit/tests |
|---|---|---|---|---|---|
| Resend API EXT-01 (outbox mock without a key) | outbound HTTPS | send one message: from, to, cc, bcc, subject, HTML, attachment | `RESEND_API_KEY` in the server environment, owned by the operator; rotation out of scope (demo) | server egress only | `mail_outbox` row per send; T-08-18 |
| Mock provider | none | writes `mail_outbox` with provider `mock` | none | none | T-08-18 |
| SMTP server and 360Works licence (source) | outbound | n/a: retired | values in scripts 9 and 10 are not copied (D-05) | n/a | n/a |
| `sci_admin` log (source) | outbound | n/a: out of scope (D-04) | none | n/a | n/a |

## Negative authorization tests

| Test id | Identity/role | Attempt | Expected denial/filter/masking | Layer exercised | Evidence |
|---|---|---|---|---|---|
| T-08-01 | anonymous | GET any page or POST any form without a cookie | redirect to `/login` (HTML) or 401 (HTMX) | middleware | AP-01 |
| T-08-02 | user with a forged or deleted session token | any request | 401 / redirect | middleware | AP-01 |
| T-08-03 | ROLE-STAFF idle for 11 minutes | next request | session ended, redirect | middleware | AP-01, BR-AUTH-02 |
| T-08-04 | ROLE-STAFF | POST without CSRF token or with another session's token | 403, no change | middleware | AP-02 |
| T-08-05 | ROLE-STAFF | create/edit a registration, student, enrollment, batch by direct POST | allowed | handler | AP-03 |
| T-08-06 | ROLE-STAFF | DELETE a person, student, event, affiliate, course by direct request | 403 | handler | AP-04 |
| T-08-07 | ROLE-ADMIN | DELETE a student with an enrollment; an event with a registration | 409 with count, no change | handler + FK | AP-04 |
| T-08-08 | ROLE-STAFF; ROLE-ADMIN | void a payment (staff); void without reason (admin); DELETE a payment (both) | 403; 422; no delete route | handler | AP-05 |
| T-08-09 | ROLE-STAFF | delete an enrollment with `pago` > 0; a registration with a payment | 409; status change offered | handler | AP-05 |
| T-08-10 | ROLE-STAFF | edit an e-mail of a batch that has a sent e-mail | 409 | handler | AP-06 |
| T-08-11 | ROLE-ADMIN | DELETE a calendar row by direct request | 404/405: no route | router | AP-07 |
| T-08-12 | ROLE-STAFF | open or POST users, settings, document templates, certifications, professors, mail profiles or templates | 403 | handler | AP-08 |
| T-08-13 | ROLE-STAFF | grades import with a crafted list of other enrollment ids; send all with a crafted batch of another user's e-mails | server uses only matched serials of the file and the batch uuid; no foreign rows touched | handler | AP-09 |
| T-08-14 | anonymous | GET a print or export URL | 401 | middleware | AP-10 |
| T-08-15 | disabled user | sign in with the right password | generic error, no session | handler | AP-11 |
| T-08-16 | ROLE-ADMIN | demote or disable the last active admin; change a role of a signed-in user | refused; target user's sessions removed | handler | AP-11 |
| T-08-17 | ROLE-STAFF | change own password without the current one; change another user's password | 422; 403 | handler | AP-12, AP-08 |
| T-08-18 | ROLE-STAFF, ROLE-ADMIN | look for `RESEND_API_KEY` or SMTP values in any page, export, log or table | not present | templates, logs, DB | AP-13 |
| T-08-19 | anonymous / network | call an API, ODBC or seed route over HTTP | no such route | router | AP-14 |
| T-08-20 | ROLE-ADMIN | view users list or export | no password hash or token in the output | serializer | field policy |
| T-08-21 | ROLE-ADMIN | look for an SSN field in students | no column, no field | schema | D-06 |

## Open decisions and unavailable evidence

| ID | Missing/conflicting evidence | Security impact | Required proof/owner | Status |
|---|---|---|---|---|
| OD-08-01 | D-08 not answered: two roles enough, no read-only role, staff in both modules | role list | operator | Step 5 default (ROLE-ADMIN, ROLE-STAFF) |
| OD-08-02 | D-09 not answered: delete rules (AP-04..AP-07) | data loss vs. staff friction | operator | Step 5 default |
| OD-08-03 | High-bit object flags not decoded (D-27) | a control may be hidden or locked in the source | screenshots of the live app | open; no data exposure found |
| OD-08-04 | Idle timeout of 10 minutes copies `fmreauthenticate10` (FileMaker Go background), which may be stricter than staff expect in a browser | usability | operator | default 10 min (D-40); setting `session.idle_minutes` |
| OD-08-05 | Lockout threshold (5 failures, 15 minutes) | brute force vs. lockout | operator | product decision for the demo |

## Completion gate

- [x] Every privilege-set dimension maps to target enforcement (records, layouts, value lists, scripts, extended privileges, menu, export, print, manage accounts, modify password).
- [x] Every extended privilege has a target access channel or a retirement decision (AP-01, AP-14).
- [x] Every entity operation defaults deny and has conditional rules where needed (appendix matrix).
- [x] Every field restriction is enforced before read/write serialization (field-level policies).
- [x] Every record predicate references real target columns with equivalent meaning (04).
- [x] Hide/CF rules are secondary UI evidence only.
- [x] Identity provisioning covers resets, disabled users and service principals (demo: no IdP, D-50).
- [ ] Negative tests pass through non-UI entry points: written above; they run in the implementation (03).
