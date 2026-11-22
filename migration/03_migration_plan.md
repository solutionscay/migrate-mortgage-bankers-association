# Migration Plan

Demo build (operator, 2026-11-21, `decisions.md`). The plan rebuilds MBA (convention), MBS (school) and MBS_SendMail (bulk e-mail) as one web app with one SQLite database (D-01, D-43), on synthetic seed data (D-02). No FileMaker data is imported. Production cutover, data profiling, cutover rehearsal, secret rotation, IdP and hosting study are out of scope (demo). The shape follows migrate-karbonfm `migration/03_migration_plan.md` and its `design/` milestone.

Labels: **fact** (object file or repository), **product decision** (operator answer or D-id), **inference**. Rules BR-, operations OP- (`07`); policies AP- (`08`); routes, JOB- and EXT- (`05`); screens UI- (`06`); tables (`04`).

## Scope and evidence baseline

| Item | Value/status |
|---|---|
| Source implementations/files | MBA.fmp12, MBS.fmp12, MBS_SendMail.fmp12 (DDR of 2026-11-13). `sci_admin` is outside the export and out of scope (D-04). |
| DDR export and `objects/index.tsv` | 806 objects; catalogs verified by `catalog_contract.py verify` for every explorer (Step 3 reports). Raw DDR stays out of version control (`filemaker-source/` is git-ignored). |
| In-scope workflows/data/history | Convention: people, events, plans, registrations, guests, payments, sponsors, billing files, door check-in, print suite. School: students, affiliates, courses, seminars, short courses, calendars, enrollments, grades, certifications, reports. E-mail: batches to enrolled students, templates, sender profiles, outbox. History: none (D-02, D-44: the 2010-2014 years are sample data). |
| Target stack/architecture decision | Rust axum + askama + rusqlite, HTMX, small vanilla JS, SQLite WAL, money as integer cents, print-CSS pages (`decisions.md`; `02_recommendations.md`). |
| Parallel-operation/cutover model | none: demo. Nothing writes back to FileMaker; FileMaker is not replaced in production by this plan. |
| Unavailable evidence/open decisions | D-08, D-09, D-11, D-13 to D-42 not answered: Step 5 defaults apply (`01`). High-bit object flags (D-27). Real screenshots are client material; only the fake-data mockups in `mockups/` are used. |

A new DDR export needs a new explode and new catalogs. `catalog_contract.py verify` fails on a catalog made from the old one.

## Release increments

Order (operator, 2026-11-21; it replaces the earlier order where the design milestone was I1):

1. **M1 Design milestone.** Components and a page shell for every kept screen.
2. **M2 Promo video milestone.** The first promo video, made from the M1 design pages.
3. **I1 to I5 build increments.** Foundation, convention, school, bulk e-mail, print and reports.

Id scheme: a letter and a number, as the increments have. `M` marks a milestone that ships no application code (M1, M2). `I` marks a build increment (I1 to I5). Every other artifact cites these ids; `01` to `08` were remapped on 2026-11-21: the old design increment became M1, and the old foundation to print increments each moved one number down.

### Build gate (G-BUILD)

> No build work starts before the promo video (M2) is done and the operator approves it.
>
> Build work is any work of I1 to I5: the `app/server` crate, migrations, the `mba-seed` tool, route handlers, Rust templates and their tests. Before the gate opens, only M1 and M2 work is allowed. The operator records the approval date in the M2 epic and in the "Gate log" below. A change to the design after the approval goes through M1 again and does not close the gate, unless the operator says so.

Gate log:

| Gate | Opened on | Approved by | Evidence |
|---|---|---|---|
| G-BUILD | not open | operator | M2 video link and approval note |

### Increment table

The demo is one release after I5; no increment goes live alone, because the demo has no users and no source data (D-02). The modules share `affiliates`, `users`, `stored_files` and settings, which I1 builds first.

| Increment | User/business outcome | Object files | Schema/operations/UI | Authorization | Dependencies | Size | Exit evidence | Rollback |
|---|---|---|---|---|---|---|---|---|
| M1 | **Design milestone.** Static HTML design pages: a component gallery and a page shell for every kept screen of `06` (UI-01 to UI-35), with the same synthetic data as the before mockups. The meaty screens are drawn in full. The operator approves the look before the video and the build. | `objects/MBA.fmp12/layouts/25-Convencionista.txt`, `objects/MBA.fmp12/layouts/29-Registro.txt`, `objects/MBA.fmp12/layouts/35-Convencion.txt`, `objects/MBA.fmp12/layouts/40-Planes.txt`, `objects/MBA.fmp12/layouts/57-Lista_de_Facturacion.txt`, `objects/MBS.fmp12/layouts/43-Estudiante.txt`, `objects/MBS.fmp12/layouts/82-Matricula_de_Curso.txt`, `objects/MBS_SendMail.fmp12/layouts/4-Emails.txt`; the source layout of every other kept screen (`06_ui_spec_appendix.md` §1) | `design/` (shape of migrate-karbonfm `design/`): `partials/` (head, icons, top bar with module tabs, nav tabs, footer), `assets/app.css`, `assets/app.js`, `tools/sync_partials.py`, `components.html` (gallery, see "M1 component set"), one page per kept screen (see "M1 page list"), `README.md` with the page table, `screenshots/` at 1440x900. No Rust. | design shows role differences (void and delete buttons and the Admin module tab for ROLE-ADMIN only) | none | L | operator approves `components.html` and each page; 35 screens have a page; before/after pairs exist for the 5 screens with a mockup (UI-04, UI-06, UI-09 with 3 tabs, UI-13, UI-34); print preview of `lista-facturacion.html` fits letter paper (R-01 in `02`); T-03-01 | discard the folder |
| M2 | **Promo video milestone.** Episode 1 of the promo series (follows the KarbonFM promo series, `plan.md`): the migrate-filemaker plan, the before screens and the M1 design. | none (the video uses no object file and no DDR screenshot) | Input pack under `promo/ep01/`: shot list, before screens (only the fake-data PNGs in `mockups/`), after screens (M1 screenshots and screen recordings of the design pages), plan excerpts (this table, the `06` meaty-screen table). The video is cut outside this project (Admiral or the operator, `plan.md`). | n/a | M1 approved | S | the video exists; the operator approves it; T-03-06; no frame shows the SMTP password, the SMTP host or the licence key, and no frame comes from `filemaker-source/` | re-cut the video |
| I1 | **Foundation.** The server runs, a user signs in, the shell shows three modules, the seed tool fills a database. | `objects/MBA.fmp12/privilege-sets/1-Full_Access.txt`, `objects/MBA.fmp12/extended-privileges/4-fmreauthenticate10.txt`, file triggers (07 appendix §6) | Crate `app/server` from the karbonfm base without Tauri; migration 0001 = `04_database_schema.sql`; `mba-seed` (OP-SYS-03); OP-AUTH-01 to OP-AUTH-04, OP-SYS-01, OP-SYS-02, OP-SYS-04; JOB-02; UI-01, UI-02, UI-29, UI-30, UI-33; base layout, partials and CSS from M1 | AP-01, AP-02, AP-08, AP-11, AP-12, AP-14 | M2 and G-BUILD open | M | T-05-01, T-05-02, T-05-05, T-05-13, T-05-15; `08` tests T-08-01 to T-08-04, T-08-12, T-08-15 to T-08-17, T-08-19 to T-08-21; seed run creates a database that passes `PRAGMA integrity_check` and `foreign_key_check` | drop the database file and reseed |
| I2 | **Convention.** Staff register people for a convention: event, plan, items, guests, payments, sponsors, billing files, door check-in. | MBA scripts 21, 25, 26, 33, 35, 37, 38, 39, 41, 42, 44, 45, 48, 49, 50 (`objects/MBA.fmp12/scripts/`); layouts 25, 26, 29, 30, 35, 36, 37, 40, 41, 45, 55 | OP-REG-01 to OP-REG-10, OP-EVT-01 to OP-EVT-05, OP-SPN-01, OP-SPN-02, OP-PPL-01, OP-PPL-02; views `v_registration_item_lines`, `v_package_price`, `v_registration_totals`, `v_event_totals`; UI-03 to UI-13 | AP-03, AP-04, AP-05, AP-06, AP-09 (owned here; retested in I3 to I5) | I1 | L | T-07-01 to T-07-06; T-05-03, T-05-04, T-05-06; `08` tests T-08-05 to T-08-10, T-08-13 (AP-03 to AP-06, AP-09); UI-03 to UI-13 match their M1 design pages | revert the branch; reseed |
| I3 | **School.** Staff enroll students in courses, seminars and short courses at the right price, record grades, and track certifications. | MBS scripts 33, 34, 35, 37, 38, 39, 41, 42, 43, 45, 48, 52, 54, 127, 128, 164, 165, 166 (`objects/MBS.fmp12/scripts/`); layouts 42, 43, 48, 49, 57, 58, 59, 60, 64, 65, 66, 81, 82, 102, 103, 111, 113 | OP-AFF-01, OP-STU-01 to OP-STU-05, OP-CRS-01 to OP-CRS-05, OP-ENR-01 to OP-ENR-08, OP-CRT-01 to OP-CRT-03; views `v_enrollments_course_lines`, `v_calendar_course_summary`, `v_calendar_seminar_summary`, `v_calendar_short_course_summary`, `v_student_academic`, `v_student_certification_progress`; EXT-02; UI-14 to UI-22, UI-32 | AP-07 (owned here); retests AP-03 to AP-06, AP-08, AP-09 | I1 | L | T-07-07 to T-07-13, T-07-18; T-05-07, T-05-11, T-05-12; `08` test T-08-11; UI-14 to UI-22 and UI-32 match their M1 design pages | revert the branch; reseed |
| I4 | **Bulk e-mail.** Staff e-mail the students of a calendar row, send one or all, and see every message in the outbox; with a Resend key the mail leaves the server. | `objects/MBS.fmp12/scripts/173-sciSendMai.caller_module.txt`; MBS_SendMail scripts 1, 7, 8, 11, 17, 18, 21 (`objects/MBS_SendMail.fmp12/scripts/`); layouts 4, 7, 8, 9, 10, 12 | OP-MAIL-01 to OP-MAIL-08; EXT-01 (Resend adapter + outbox mock); JOB-01; view `v_mail_batch_counts`; UI-25 to UI-28, UI-31 | AP-13 (owned here); retests AP-03, AP-06, AP-08, AP-09 | I3 (recipients come from enrollments) | M | T-07-14 to T-07-16, T-07-20; T-05-08 to T-05-10; `08` test T-08-18; with no key: zero network calls and one `mail_outbox` row per send | revert the branch; reseed |
| I5 | **Print, reports, exports.** Staff print every convention and school document, run the admin reports and the dashboard, and export CSV and XLSX files. | print layouts and scripts in `05_api_design_appendix.md` §1 and §2; `objects/MBA.fmp12/scripts/95-registrations.createAccountingReport_lastTimestamp.txt`; `objects/MBS.fmp12/layouts/137-Reportes_Administrativos.txt` | OP-RPT-01 to OP-RPT-05; 72 print pages (25 convention, 47 school); 8 admin reports; `/exports/accounting.csv` (user picks the event items; plan items in or out; every item column summed, BR-RPT-12), `/exports/csbs.csv`; labels and tickets print 1 to qty (BR-RPT-10); `document_templates` texts; UI-23, UI-24, UI-34, UI-35 | AP-10 (owned here); retests AP-03 | I2, I3 | L | T-07-17, T-07-19; T-05-14; `08` test T-08-14; Lista de Facturación matches its M1 design page in print preview | revert the branch |

Each build increment implements its server-side invariants and its negative authorization tests before its screens are wired.

Data migration, reconciliation and cutover: out of scope (demo, D-02). The provisional "data migration" increment of the Step 4 draft is not an increment; `01` now points to this table.

### M1 component set

`design/components.html` shows every component in its states. The Rust templates reuse the same class names and partials (I1 onward).

| Group | Components |
|---|---|
| Shell | top bar (app name, user menu: Cambiar contraseña, Salir); module tabs in the top bar (Convención, Escuela, Correos, Admin for ROLE-ADMIN only) and a nav tab bar with the screens of the current module; module tiles with counts; breadcrumb; page header with title and status |
| Actions | buttons: primary, secondary, danger, icon, disabled, busy; split button "Imprimir ▾"; role-gated button (admin only) |
| Forms | text, textarea, select, date, money input (cents at the edge), number, checkbox, radio, file chip; required mark; field error; form error summary; read-only value |
| Search and lists | search box (HTMX, 300 ms delay); filter bar; list/table view toggle; dense data table (32 px rows, sticky header, sortable header, 50 rows per page, pagination); row actions; empty state |
| Record parts | tabs as links (`hx-get` + `hx-push-url`); portal table with an inline add row and row delete; key-value summary card; totals panel (total, paid, balance) |
| Dialogs | native `<dialog>`: picker (event, plan, calendar row), confirm with a count ("Enviar 23 correos"), form dialog; focus return and Esc |
| Feedback | flash and toast (success, error); status badges (neutral, green, red, amber, always with a text label); banners (mock mode, info, warning); progress bar with poll; inventory-only marker ("borrador", D-17, D-19); 403, 404 and 409 message pages |
| Mail | sandboxed preview frame (`iframe srcdoc`); token list beside a template editor |
| Import | upload step; preview table with old → new values and an unmatched list; apply button |
| Charts | inline SVG bar chart drawn on the server (UI-24) |
| Print | print frame (screen toolbar hidden in `@media print`, letter page, page break per group, header with logo and signer); label grid in mm; "No hay registros para imprimir" page |

### M1 page list

A page shell is the screen inside the app shell with its header, its components in place and a few synthetic rows. The meaty screens (`06` "Meaty screens") are drawn in full, with all states. One page per kept screen of `06`:

| Design page | Screen | Depth | Before screen |
|---|---|---|---|
| `design/login.html` | UI-01 | shell | none |
| `design/index.html` | UI-02 | shell | none (`mockups/index.png` is the mockup index, not a screen) |
| `design/convencionistas.html` | UI-03 | shell | none |
| `design/convencionista.html` | UI-04 | full | `mockups/convencionista.png` |
| `design/registros.html` | UI-05 | shell | none |
| `design/registro.html` | UI-06 | full (meaty) | `mockups/registro.png` |
| `design/registro-puerta.html` | UI-07 | shell (tablet width) | none |
| `design/convenciones.html` | UI-08 | shell | none |
| `design/convencion.html` (3 tabs) | UI-09 | full (meaty) | `mockups/convencion.png`, `mockups/convencion-auspiciadores.png`, `mockups/convencion-facturacion.png` |
| `design/actividad.html` | UI-10 | shell | none |
| `design/auspiciador.html` | UI-11 | shell | none |
| `design/planes-lista.html` | UI-12 | shell | none |
| `design/planes.html` | UI-13 | full (meaty) | `mockups/planes.png` |
| `design/estudiantes.html` | UI-14 | shell | none |
| `design/estudiante.html` | UI-15 | full (meaty) | none |
| `design/afiliados.html` | UI-16 | shell | none |
| `design/afiliado.html` | UI-17 | shell | none |
| `design/ofrecimiento.html` (tabs Calendario, Detalles) | UI-18 | shell | none |
| `design/fecha-curso.html` (dialog over UI-18) | UI-19 | shell | none |
| `design/matricula.html` (dialog) | UI-20 | full (meaty) | none |
| `design/matriculas.html` | UI-21 | shell | none |
| `design/importar-notas.html` | UI-22 | shell (3 steps) | none |
| `design/reportes.html` | UI-23 | shell | none |
| `design/panel-matriculas.html` | UI-24 | shell, inventory-only marker | none |
| `design/envios.html` | UI-25 | shell | none |
| `design/correos.html` | UI-26 | full (meaty) | none |
| `design/vista-previa.html` | UI-27 | shell | none |
| `design/bandeja-salida.html` | UI-28 | shell | none |
| `design/admin-usuarios.html` | UI-29 | shell | none |
| `design/admin-configuracion.html` | UI-30 | shell | none |
| `design/admin-correo.html` | UI-31 | shell | none |
| `design/admin-certificaciones.html` | UI-32 | shell, inventory-only marker | none |
| `design/cambiar-contrasena.html` | UI-33 | shell | none |
| `design/lista-facturacion.html` | UI-34 | full (meaty, print) | `mockups/lista-facturacion.png` |
| `design/print-escuela.html` (one enrollment list) | UI-35 | shell (print) | none |

`design/correos.html`, `design/admin-correo.html` and `design/bandeja-salida.html` show the Resend mock banner and sender profiles with `ejemplo.com` addresses. No design page shows an SMTP host, an SMTP password or a licence key (D-05).

### M2 promo video inputs

- Before screens: only the fake-data PNGs in `mockups/`. Never `filemaker-source/screenshots`.
- After screens: M1 screenshots and screen recordings of the design pages.
- Content: the migrate-filemaker plan (this increment order, the `06` meaty screens), the before screens, the proposed design.
- Secrets: no frame, caption or narration shows the SMTP password, the SMTP host or the licence key (MBS_SendMail scripts 9 and 10). The input pack holds none of them.
- Coverage: the meaty screens with a before screen carry the before/after part; the other design pages may appear without a pair.

## Foundation gates

- [ ] G-BUILD is open: M2 done and approved by the operator (see "Build gate").
- [ ] Repository/runtime versions are reproducible: `rust-toolchain.toml`, `Cargo.lock` (I1). CI quality gates: out of scope (demo); `cargo test` runs locally.
- [x] Database schema decisions resolve source keys, TO aliases, compound relationships, validation changes, containers, and timezone policy (`04`: uuid keys, D-11 key links, CHECKs, `stored_files`, Puerto Rico local time D-67).
- [ ] Identity provisioning and default-deny authorization skeleton exist before user data operations (I1: AP-01, AP-02, `require_role`).
- [x] Audit actor/time/reason and sensitive-field logging/masking rules are defined (`05` "Errors, concurrency, and observability"; BR-SYS-02).
- [ ] Backup, restore, observability, secret management, and environment promotion: out of scope (demo). The database is one file; the seed tool recreates it.
- [x] Source-to-target traceability identifiers are retained (object paths in `04` to `08`; layout ids in the `06` appendix; print routes in the `05` appendix).

## Rules and policies by increment

Every rule of `07` and every policy of `08` lands in exactly one build increment. Later increments retest the policies they use. M1 and M2 own no rule and no policy.

| Increment | Business rule ids (`07`) | Authorization policy ids (`08`) | Discovery answers (`D-`) |
|---|---|---|---|
| M1 | none (design only) | none (role differences shown in the design) | D-17, D-18, D-27, D-39, D-73 |
| M2 | none (video only) | none | D-05 (no secret in any frame) |
| I1 | BR-AUTH-01, BR-AUTH-02, BR-AUTH-03, BR-AUTH-04, BR-SYS-01, BR-SYS-02, BR-SYS-03, BR-SYS-04 | AP-01, AP-02, AP-08, AP-11, AP-12, AP-14 | D-01, D-02, D-07, D-08, D-36, D-40, D-50 |
| I2 | BR-REG-01, BR-REG-02, BR-REG-03, BR-REG-04, BR-REG-05, BR-REG-06, BR-REG-07, BR-REG-08, BR-REG-09, BR-REG-10, BR-REG-11, BR-EVT-01, BR-EVT-02, BR-EVT-03, BR-EVT-04, BR-EVT-05, BR-EVT-06, BR-EVT-07, BR-SPN-01, BR-SPN-02, BR-PPL-01, BR-PPL-02 | AP-03, AP-04, AP-05, AP-06, AP-09 | D-03, D-09, D-11, D-15, D-16, D-31, D-32 |
| I3 | BR-AFF-01, BR-AFF-02, BR-STU-01, BR-STU-02, BR-STU-03, BR-STU-04, BR-STU-05, BR-STU-06, BR-STU-07, BR-CRS-01, BR-CRS-02, BR-CRS-03, BR-CRS-04, BR-CRS-05, BR-CRS-06, BR-ENR-01, BR-ENR-02, BR-ENR-03, BR-ENR-04, BR-ENR-05, BR-ENR-06, BR-ENR-07, BR-ENR-08, BR-ENR-09, BR-CRT-01, BR-CRT-02, BR-CRT-03, BR-CRT-04 | AP-07 | D-04, D-06, D-09, D-11, D-14, D-19, D-23, D-26 |
| I4 | BR-MAIL-01, BR-MAIL-02, BR-MAIL-03, BR-MAIL-04, BR-MAIL-05, BR-MAIL-06, BR-MAIL-07, BR-MAIL-08, BR-MAIL-09 | AP-13 | D-05, D-19, D-20, D-29 |
| I5 | BR-RPT-01, BR-RPT-02, BR-RPT-03, BR-RPT-04, BR-RPT-05, BR-RPT-06, BR-RPT-07, BR-RPT-08, BR-RPT-09, BR-RPT-10, BR-RPT-11, BR-RPT-12 | AP-10 | D-13, D-16, D-17, D-18, D-25, D-33, D-34 |

Count: 79 rules, 14 policies.

## UI implementation

| Screen/flow | Evidence readiness | Data/operations | States/errors/accessibility | Source trace | Acceptance test |
|---|---|---|---|---|---|
| M1 design pages (UI-01 to UI-35; meaty in full: UI-04, UI-06, UI-09, UI-13, UI-15, UI-20, UI-26, UI-34) | behavior-ready → implementation-ready when approved | static HTML with synthetic data | empty and error states drawn for UI-06 and UI-26; every state in `components.html`; keyboard path through dialogs | `06` "Meaty screens", "M1 page list" above | operator approval; before/after screenshots |
| M2 promo video | n/a | M1 screenshots and recordings; `mockups/*.png` | no secret in any frame | `plan.md` | operator approval; T-03-06 |
| UI-01, UI-02, UI-29, UI-30, UI-33 (I1) | behavior-ready; implementation-ready after M1 | OP-AUTH-01 to OP-AUTH-04, OP-SYS-01, OP-SYS-02 | generic sign-in error; 403 on admin routes for staff | D-50 | T-05-01, T-05-05 |
| UI-03 to UI-13 (I2) | implementation-ready after M1 | REG, EVT, SPN, PPL operations | confirm dialogs (plan change, item delete); 409 messages on refused deletes | layouts MBA 25-45, 55 | T-07-01 to T-07-06, T-05-03, T-05-04 |
| UI-14 to UI-22, UI-32 (I3) | implementation-ready after M1; UI-32 inventory-only (D-19) | STU, AFF, CRS, ENR, CRT operations | price shown before save; import preview changes nothing | layouts MBS 42-113 | T-07-07 to T-07-13, T-05-11 |
| UI-25 to UI-28, UI-31 (I4) | implementation-ready after M1 | MAIL operations, EXT-01, JOB-01 | mock banner; progress poll; error rows | layouts MBS_SendMail 4-12 | T-07-14 to T-07-16, T-05-08 to T-05-10 |
| UI-23, UI-24, UI-34, UI-35 (I5) | implementation-ready after M1; UI-24 inventory-only (D-17) | OP-RPT-01 to OP-RPT-05 | "No hay registros" page; print CSS | `05` appendix §1, §2 | T-07-17, T-05-14 |

UI-24 and UI-32 stay inventory-only until the owner answers D-17 and D-19. The demo builds them on the stated defaults and marks them so.

## Integration and reporting plan

| Boundary/report | Direction | Contract | Auth/secrets | Retry/idempotency/audit | Reconciliation test |
|---|---|---|---|---|---|
| EXT-01 Resend adapter with outbox mock (I4) | outbound | `05` "External integrations": `POST https://api.resend.com/emails`; without `RESEND_API_KEY` the `OutboxMock` writes `mail_outbox` only | `RESEND_API_KEY` from the environment; never stored, logged or rendered (AP-13). No SMTP host, password or licence key anywhere (D-05). | no automatic retry; idempotency key = e-mail uuid; JOB-01 skips sent rows; `mail_outbox` row per attempt | T-05-08 (mock), T-05-09 (local HTTP mock of Resend), T-07-15, T-07-16 |
| EXT-02 Google Maps link (I3) | outbound (browser link) | URL built on the server from the address | none | n/a | none |
| Print pages (I5) | n/a | 72 print-CSS pages (`05` appendix §1) | AP-10 | n/a | T-07-17 |
| Accounting CSV, CSBS CSV, grades XLSX (I3, I5) | outbound file | `05` RPT and ENR routes | AP-10 | n/a | T-05-14 |
| Grades XLSX import (I3) | inbound file | upload, preview, apply; rows match on the enrollment `serial` (source `id` #61, not `ID #:`); `Puntuacion`, `NOTA` and `is_passed` follow the imported final average (BR-ENR-06); XLSX is a product decision (BR-ENR-07), the DDR gives no file type | AP-09 | apply in one transaction; rerun gives the same values | T-07-12 |

No inbound webhook exists. No `Insert from URL` step exists in the source.

## Demo seed data (replaces the data migration)

No FileMaker data is migrated (D-02). A seed tool, like karbonfm `tools/synth`, writes synthetic data (OP-SYS-03).

- Tool: `mba-seed` (Rust binary in the same workspace). Arguments: `--db <path>`, `--seed <u64>` (default fixed, so every run gives the same data), `--size small|demo`, `--today <date>` (default: the current date).
- Output: a fresh SQLite file with the `04` schema, the `04` value-list and settings seeds, three users (the dummy accounts of `08` Identity provisioning: `mvega` and `inieves` with ROLE-ADMIN, `equintana` with ROLE-STAFF), and the module data. The tool prints the three demo passwords once; they are not stored in the repository.
- Dates follow today (D-44: years, dates and signers become data). The current convention is next year; past ones are one and two years back.
- Names and places are synthetic and match the mockups, so the M1 design pages, the running app and the before screens show the same records: person "María Ejemplo Demo", company "Cooperativa Muestra", event "Convención Demo <year>" at "Hotel Demo Plaza", plan "P2(B) - 15 marzo", phone numbers in the 555 range, `ejemplo.com` e-mail addresses.
- No SSN (D-06). No real person, company or address. No content from `filemaker-source/`.

| Area | `small` (tests) | `demo` (matches the mockups where a mockup shows a count) |
|---|---:|---:|
| People | 20 | 2,318 (`mockups/convencionista.html`: record 214 of 2,318) |
| Events | 2 | 6 (`mockups/convencion.html`: 3 of 6) |
| Plans | 3 | 27 (`mockups/planes.html`: 1 of 27) |
| Registrations | 15 | 512 (`mockups/registro.html`: found set of 512); the current event has 184 registrations and 41 guests (`mockups/convencion.html`) |
| Payments | 10 | about 1 per registration, some partial, 3 voided |
| Sponsors | 2 | 12 across the current event |
| Affiliates | 5 | 40 (ACTIVO and INACTIVO) |
| Students | 20 | 600 |
| Courses / seminars / short courses | 2 / 1 / 1 | 12 / 6 / 8 |
| Calendar rows | 6 | 3 years of rows, every status |
| Enrollments | 30 | about 2,000, with grades on past course rows |
| Certifications | 2 | 4, with requirements; some students graduated |
| Mail batches | 1 | 3 (one draft, one sent, one with errors) and their outbox rows |

The seed writes through the domain layer where a rule creates rows (BR-STU-01 certification rows, BR-EVT-06 plan items, BR-REG-02 registration items), so seeded data obeys the same invariants as data entered in the app.

## Data migration runbook

Out of scope (demo, D-02). The template's extract, transform, load and validation steps do not apply: no source export is read. The rows below stay as a record of what a real migration would need.

| Step | Status |
|---|---|
| Extract and stage (freeze, hashes, counts, encoding, containers) | out of scope (demo) |
| Transform and load (dependency order, FK cycles, counters from the source maximum) | out of scope (demo). The seed tool sets each serial counter (`registration_number`, enrollment serials) from its own data. |
| Validation matrix (counts, profiles, checksums, orphans, containers, totals) | out of scope (demo). Seed checks: `PRAGMA integrity_check`, `PRAGMA foreign_key_check`, and the view totals of T-07-03 and T-07-06 on the `small` seed. |

## Test strategy

Effort stays small: targeted tests that prove each rule and policy. The operator runs broader testing.

- Semantic fixtures for tier1 rules: T-07-01 to T-07-21 (`07`), each on an in-memory SQLite with the `small` seed or a hand-built fixture.
- Route tests: T-05-01 to T-05-15 (`05`), calling the axum router in process and checking the HTML with `scraper`.
- Authorization negative tests: the `08` list (T-08-nn), run in the increment that owns each policy and rerun in later ones.
- Integration tests: EXT-01 against a local HTTP mock (T-05-09); mock mode (T-05-08).
- UI: manual walk of the meaty screens against the M1 design pages; keyboard walk of UI-06 and UI-20.
- Migration rehearsal and parallel-run reconciliation: out of scope (demo).

| Test id | Increment | Case |
|---|---|---|
| T-03-01 | M1 | each design page opens from disk with no console error; every kept screen UI-01 to UI-35 has a page; print preview of `lista-facturacion.html` breaks pages per registration |
| T-03-02 | I1 | `mba-seed --size small` twice with the same seed gives byte-identical table contents |
| T-03-03 | I1 | the seeded database passes `integrity_check` and `foreign_key_check` |
| T-03-04 | I2 | seeded registrations: `v_registration_totals` matches a hand sum for 3 sample registrations |
| T-03-05 | I4 | `grep` of the repository and the seeded database finds no SMTP host, SMTP password or licence-key text |
| T-03-06 | M2 | `grep` of `design/` and `promo/ep01/` finds no SMTP host, SMTP password or licence-key text; the shot list names no file under `filemaker-source/`; the operator watches the cut and confirms no secret is on screen |

## Cutover and rollback

Out of scope (demo). The demo has no production users, no source freeze and no write-back to FileMaker.

| Decision/check | Owner | Deadline | Evidence |
|---|---|---|---|
| Final source freeze or delta strategy | n/a | out of scope (demo) | none |
| Account invitation/IdP mapping | n/a | out of scope (demo); demo users come from the seed tool (D-07, D-50) | none |
| Final migration validation sign-off | n/a | out of scope (demo) | none |
| Monitoring/support/training | n/a | out of scope (demo) | none |
| Rollback trigger and decision authority | operator | per increment: revert the branch, reseed | git history |
| Target-to-FileMaker write-back capability/limit | n/a | none: the demo never writes to FileMaker | none |

## Risk and decision register

| ID | Risk/open decision | Evidence | Likelihood/impact | Mitigation/proof | Owner/status |
|---|---|---|---|---|---|
| OD-03-01 | Seed sizes match the mockup counts | mockup record counters | low / low | `demo` size table above | operator / default set |
| OD-03-02 | Design pages use plain CSS (no framework) | `decisions.md` (no build step) | low / medium | M1 | operator / default set |
| OD-03-03 | Order of I3 and I4 (e-mail needs enrollments) | BR-MAIL-07 | low / low | I4 after I3 | operator / default set |
| OD-03-04 | UI-24 and UI-32 built on defaults while D-17 and D-19 are open | `06` | medium / low | mark inventory-only in the app (a "borrador" note) | operator / open |
| OD-03-05 | Increment order: design (M1), promo video (M2), then build (I1 to I5), with the G-BUILD gate | operator, 2026-11-21 | n/a | "Release increments" above | operator / decided |
| R-03-01 | Print CSS differs between browsers | R-01 in `02` | medium / medium | test in M1 on Chrome and Firefox | design milestone |
| R-03-02 | Secret text from MBS_SendMail scripts 9 and 10 leaks into the repository or a video | D-05, D-87; `plan.md` | low / high | T-03-05, T-03-06; promo uses only `mockups/` and `design/` | operator / mitigated |
| R-03-03 | High-bit object flags hide controls the target shows | D-27 | medium / low | visible by default | operator / open |
| R-03-04 | 35 page shells make M1 larger than the earlier 8-page scope and delay the build | operator rule (all kept screens) | medium / low | shells reuse `components.html`; only the 8 meaty pages are drawn in full | operator / accepted |

## Completion gate

- [x] `catalog_contract.py verify` passes for every explorer (Step 3 reports; the scripts catalog rerun after the D-87 redaction).
- [x] Every logic-bearing catalog has a disposition (`07` appendix §1 to §6; `06` appendix §1).
- [x] Every source security dimension has an enforcement owner and negative test (`08`; `02` "Authorization architecture").
- [x] Every relationship predicate and target key is resolved or explicitly open (`04` appendix; D-11, D-12).
- [ ] Data validation exceeds counts/samples and all rejects are accounted for: out of scope (demo, no data load).
- [ ] Rollback, restore, and cutover rehearsal evidence is recorded: out of scope (demo).
