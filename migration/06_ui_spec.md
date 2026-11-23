# UI Evidence and Target Specification

Target: one web app, server-rendered HTML with HTMX partials and a small vanilla JS layer for focus, keyboard and dialogs (product decision, `decisions.md`). Three modules share one shell: convention (MBA), school (MBS), e-mail (MBS_SendMail) (D-01). UI text stays in Spanish, as the source layouts are (inference: the users work in Spanish; labels are copied from the layout files). Routes are in `05_api_design.md`; rules in `07_business_logic.md`; policies in `08_auth_roles.md`.

Labels: **fact** = read from the layout object file or catalog; **product decision** = operator answer or Step 5 default (D-id given); **inference** = drawn from names or structure.

Full layout inventory (181 layouts, with disposition and target screen), the retired layouts, and the object maps of the meaty screens: `06_ui_spec_appendix.md`.

## Evidence

| Source | Coverage | Missing |
|---|---|---|
| Layout object files (`objects/<file>/layouts/`) | objects, keys, bounds, labels, fields, value lists, tabs, portals, buttons, triggers, hide rules, conditional formats, tooltips | theme styling, how the screen looks |
| Layout explorer CSVs (`explorers/layouts/csv`) | 181 of 181 layouts: purpose, context, buttons, triggers, portals, value lists, target | meaning of high-bit object flags (D-27) |
| Script explorer CSVs (`explorers/scripts/csv`) | what buttons and triggers do | none |
| Menu explorer CSVs (`explorers/menus/csv`) | 14 menus run scripts (07 appendix §5): New Record, Duplicate, Quick Find overrides | standard-menu removals (D-41, no impact) |
| Before screens (fake-data mockups) | 7 PNG and 6 HTML files in `mockups/`, all convention (MBA): Convencionista, Registro, Convención (3 tabs), Lista de Facturación, Planes. Built from screenshots or from the DDR, with synthetic data (`mockups/index.html`). | no school or e-mail mockup |
| Discovery/design decisions | D-01, D-05, D-17, D-18, D-19, D-27, D-31, D-33, D-39, D-50; `decisions.md` (stack, print as HTML) | D-72 to D-77 not answered: Step 5 defaults |

Overall readiness: behavior-ready. Each kept screen becomes implementation-ready at the end of the design milestone (M1 in `03_migration_plan.md`): a component set plus an HTML page shell for every kept screen (UI-01 to UI-35), with the meaty screens drawn in full, reviewed by the operator.

The real FileMaker screenshots (`filemaker-source/screenshots`) are client material. Only the fake-data mockups in `mockups/` are used as before screens (operator, `plan.md`).

## Meaty screens

The screens with the most logic: several portals, a write that changes other rows, a trigger, a confirm step, or live totals. The design milestone (M1) draws these in full; every other kept screen gets a page shell. The promo video (M2) is made from the design pages, in the private promo kit, vault and share (not in this repository); its before/after part uses only the meaty screens with a before screen (operator, `plan.md`).

| Screen | Why it is meaty | Before screen (mockup) | Design page (M1) |
|---|---|---|---|
| UI-06 Registro | 3 portals (items, guests, payments), event and plan pickers, plan change deletes and rebuilds items after a confirm (BR-REG-02), guest change recomputes every item quantity not set by hand (BR-REG-03, D-88), live total and balance (BR-REG-04, BR-REG-08), payment void (BR-REG-09) | `mockups/registro.png`, `mockups/registro.html` | `design/registro.html` |
| UI-09 Convención | event header with live totals (BR-EVT-04), 3 tabs (Actividades, Auspiciadores, Facturación), print launchers for invoices and sponsor letters | `mockups/convencion.png`, `mockups/convencion.html` (Actividades); `mockups/convencion-auspiciadores.png` (Auspiciadores); `mockups/convencion-facturacion.png` (Facturación) | `design/convencion.html` |
| UI-13 Planes | event pick builds plan items (BR-EVT-06, was an OnObjectSave trigger), active toggle with conditional style, live plan price (BR-EVT-05) | `mockups/planes.png`, `mockups/planes.html` | `design/planes.html` |
| UI-34 Lista de Facturación (print) | billing list per registration: items, plan, costs, totals (BR-REG-04, BR-REG-05); the model for every print page | `mockups/lista-facturacion.png`, `mockups/lista-facturacion.html` | `design/lista-facturacion.html` |
| UI-15 Estudiante | 4 portals (certifications, course, seminar, short-course enrollments), certification progress (BR-CRT-02), academic summary (BR-STU-06), enroll buttons, certificate print | none | `design/estudiante.html` |
| UI-20 Matrícula | calendar pick limited to active rows (BR-CRS-01), price by affiliate status (BR-ENR-01), grades with letter and pass (BR-ENR-03 to BR-ENR-05), delete guard (BR-ENR-08) | none | `design/matricula.html` |
| UI-26 Envío de correos | batch header, one row per e-mail with status badge (BR-MAIL-05), send one, send all with progress (JOB-01), outbox mock banner (EXT-01) | none | `design/correos.html` |

Supporting screen with a mockup: UI-04 Convencionista (`mockups/convencionista.png`, `mockups/convencionista.html`). It is the entry to UI-06 (OP-REG-01), so the design milestone draws it too (`design/convencionista.html`), but it holds little logic.

## Source screen inventory

Main screens only. Every one of the 181 layouts, including print layouts and retired ones, is in `06_ui_spec_appendix.md` §1.

| Layout file | TO → base table | Purpose | Fields/portals/buttons | Hide/CF rules | Screenshot | Readiness | Disposition |
|---|---|---|---|---|---|---|---|
| `objects/MBA.fmp12/layouts/25-Convencionista.txt` | people → people | person form | portal registration; 8 buttons (Añadir Registro, nav) | none | `mockups/convencionista.png` | behavior-ready | keep → UI-04 |
| `objects/MBA.fmp12/layouts/26-Lista_de_Convencionistas.txt`, `objects/MBA.fmp12/layouts/28-Tabla_de_Convencionistas.txt` | people → people | list, table | 5 buttons (nav) | none | none | behavior-ready | keep, merge → UI-03 |
| `objects/MBA.fmp12/layouts/29-Registro.txt` | registration → registration | registration form | portals items, payments, guests; 12 buttons; trigger key 56 OnObjectExit | none | `mockups/registro.png` | behavior-ready | keep → UI-06 |
| `objects/MBA.fmp12/layouts/30-Lista_de_Registro.txt`, `objects/MBA.fmp12/layouts/31-Tabla_de_Registro.txt` | registration → registration | list, table | 5 buttons | none | none | behavior-ready | keep, merge → UI-05 |
| `objects/MBA.fmp12/layouts/32-Planes_Activos.txt`, `objects/MBA.fmp12/layouts/33-Eventos_Activos.txt` | package_items, event_items | picker dialogs | 2 buttons each | none | none | behavior-ready | merge → UI-06 dialogs |
| `objects/MBA.fmp12/layouts/35-Convencion.txt`, `objects/MBA.fmp12/layouts/98-Convencion_View.txt`, `objects/MBA.fmp12/layouts/99-Convencion_Edit.txt` | events → events | event form (start layout of MBA) | portals event_items, sponsor, billing; 18 buttons | none | `mockups/convencion.png`, `mockups/convencion-auspiciadores.png`, `mockups/convencion-facturacion.png` | behavior-ready | keep, merge → UI-09 |
| `objects/MBA.fmp12/layouts/36-Detalle_de_Evento.txt` | event_items → event_items | item detail | portal registration items; 9 buttons | none | none | behavior-ready | keep → UI-10 |
| `objects/MBA.fmp12/layouts/37-Lista_de_Eventos.txt`, `objects/MBA.fmp12/layouts/38-Tabla_de_Eventos.txt` | events → events | list, table | 5 buttons | none | none | behavior-ready | keep, merge → UI-08 |
| `objects/MBA.fmp12/layouts/40-Planes.txt` | packages → packages | plan form | portal package_items; trigger key 21 OnObjectSave; 7 buttons | CF key 5 (active) | `mockups/planes.png` | behavior-ready | keep → UI-13 |
| `objects/MBA.fmp12/layouts/41-Lista_de_Planes.txt`, `objects/MBA.fmp12/layouts/44-Tabla_de_Planes.txt` | packages → packages | list, table | 5 buttons | none | none | behavior-ready | keep, merge → UI-12 |
| `objects/MBA.fmp12/layouts/45-Auspiciadores.txt` | sponsor → sponsor | sponsor form | no buttons | none | none | behavior-ready | keep → UI-11 |
| `objects/MBA.fmp12/layouts/55-Registro_Puerta.txt` | registration_items | door list (print) | value list 8 `registro_puerta_status` | none | none | behavior-ready | keep → UI-07 and UI-34 |
| `objects/MBA.fmp12/layouts/57-Lista_de_Facturacion.txt` | registration_items | billing list (print) | sub-summaries by registration | none | `mockups/lista-facturacion.png` | behavior-ready | keep → UI-34 |
| `objects/MBS.fmp12/layouts/42-Lista_de_Estudiantes.txt`, `objects/MBS.fmp12/layouts/45-Tabla_de_Estudiantes.txt` | students → students | list, table (start layout of MBS) | 7 buttons | none | none | behavior-ready | keep, merge → UI-14 |
| `objects/MBS.fmp12/layouts/43-Estudiante.txt` | students → students | student form | portals student_certifications, course, seminar, short-course enrollments; 18 buttons | object flags (D-27) | none | behavior-ready | keep → UI-15 |
| `objects/MBS.fmp12/layouts/48-Afiliado.txt` | affiliates → affiliates | affiliate form | 8 buttons | none | none | behavior-ready | keep → UI-17 |
| `objects/MBS.fmp12/layouts/49-Lista_de_Afiliados.txt`, `objects/MBS.fmp12/layouts/50-Tabla_de_Afiliados.txt` | affiliates → affiliates | list, table | 6 buttons | none | none | behavior-ready | keep, merge → UI-16 |
| `objects/MBS.fmp12/layouts/57-Cursos_Especializados.txt`, `objects/MBS.fmp12/layouts/60-Seminarios.txt`, `objects/MBS.fmp12/layouts/64-Cursos_Cortos.txt` | courses, seminars, short_courses | offering forms | portal calendar (sort id desc); Abrir Curso, Fechas, Ver lista, Borrar (always denied), mail icon | none | none | behavior-ready | keep, merge → UI-18 |
| `objects/MBS.fmp12/layouts/58-Fechas_de_Curso.txt`, `objects/MBS.fmp12/layouts/61-Fechas_del_Seminarios.txt`, `objects/MBS.fmp12/layouts/65-Fechas_del_Curso_Corto.txt` | calendar rows | date dialogs | portal calendar_course_dates; 15 repeating dates (short course) | none | none | behavior-ready | keep, merge → UI-19 |
| `objects/MBS.fmp12/layouts/82-Matricula_de_Curso.txt`, `objects/MBS.fmp12/layouts/103-Matricula_de_Seminario.txt`, `objects/MBS.fmp12/layouts/113-Matricula_de_Curso_Corto.txt` | enrollments → enrollments | enrollment dialogs | Guardar; trigger OnObjectSave on the calendar field (commit) | object flags (D-27) | none | behavior-ready | keep, merge → UI-20 |
| `objects/MBS.fmp12/layouts/81-Lista_de_Matrículas_de_Cursos_Especializados.txt`, `objects/MBS.fmp12/layouts/102-Lista_de_Matrículas_de_Seminarios.txt`, `objects/MBS.fmp12/layouts/111-Lista_de_Matrículas_de_Cursos_Cortos.txt` | enrollments | lists | 0-1 buttons | none | none | behavior-ready | keep, merge → UI-21 |
| `objects/MBS.fmp12/layouts/137-Reportes_Administrativos.txt` | globals | report launcher | criteria fields (globals), 14 buttons | none | none | behavior-ready | keep → UI-23 |
| `objects/MBS.fmp12/layouts/76-Bar_-_Cursos_por_Año.txt`, `objects/MBS.fmp12/layouts/77-Bar_-_Matrículas_por_Año.txt` (+ 75, 78, 139, 141) | course_enrollments, course_calendar | charts | chart objects | none | none | inventory-only | merge → UI-24 (D-17) |
| `objects/MBS_SendMail.fmp12/layouts/7-Batches.txt` | batches → batches | batch list | 1 button | none | none | behavior-ready | keep → UI-25 |
| `objects/MBS_SendMail.fmp12/layouts/4-Emails.txt` | emails → emails | batch + e-mail rows | 6 buttons (send one, send all, preview, attachment) | CF key 65 (Draft, Sent, Error) | none | behavior-ready | keep → UI-26 |
| `objects/MBS_SendMail.fmp12/layouts/9-Template_Preview.txt`, `objects/MBS_SendMail.fmp12/layouts/10-Email_Preview.txt` | templates, emails | previews | 1 button | none | none | behavior-ready | keep → UI-27 |
| `objects/MBS_SendMail.fmp12/layouts/8-Templates.txt`, `objects/MBS_SendMail.fmp12/layouts/12-profiles.txt` | templates, profiles | admin forms | 1 button | none | none | behavior-ready | keep → UI-31 (D-19) |
| `objects/MBS.fmp12/layouts/59-Certificaciones.txt`, `objects/MBS.fmp12/layouts/66-Tabla_de_Profesores.txt` | certifications, professors | admin forms, no caller | none | none | none | inventory-only | keep → UI-32 (D-19) |
| `objects/MBS.fmp12/layouts/165-Globals.txt` | globals | developer layout | none | none | none | inventory-only | merge → UI-30 settings |

Totals (appendix §1): 112 keep, 13 merge, 56 retire. 72 print layouts become print pages (UI-34, UI-35). Layout `virtual_table` (MBA 103) is retired; the accounting CSV replaces it (D-16).

## Object map

The full per-object maps of the meaty screens are in `06_ui_spec_appendix.md` §3. Key objects:

| Layout file | Object key | Type/bounds/panel | Label/control/value list | Action/trigger | Hide/CF rule | Target component |
|---|---|---|---|---|---|---|
| `objects/MBA.fmp12/layouts/29-Registro.txt` | 50 | field button | `registration::event_display` | script 41 (event picker) | none | "Evento" field with a "Cambiar" button → event dialog (`GET /registrations/{id}/event-picker`) |
| `objects/MBA.fmp12/layouts/29-Registro.txt` | 62 | field button | `registration::package_name` | script 44 (plan picker, warns) | none | "Plan" field + plan dialog; confirm dialog when a plan is set |
| `objects/MBA.fmp12/layouts/29-Registro.txt` | 2 | portal | `registration_items`; value list `registrationSelectEventItems` | key 11 script 37 (attributes); key 105 delete row | none | items table with add row (select of event items), attribute button per row, delete with confirm |
| `objects/MBA.fmp12/layouts/29-Registro.txt` | 54, 56 | portal, field | `registration_guests::name_badge` | OnObjectExit → script 38 | none | guests list; `hx-trigger="change"` posts the badge and refreshes item quantities (OOB) |
| `objects/MBA.fmp12/layouts/29-Registro.txt` | 17, 31 | portal, button | `payments` sorted by creation desc; Agregar Pago | script 35 | none | payments table; "Agregar pago" inline form; "Anular" for ROLE-ADMIN |
| `objects/MBA.fmp12/layouts/35-Convencion.txt` | 24, 52, 128 | portals in tabs | event_items, sponsor, billing | keys 21, 79, 98, 99 | none | three tabs loaded by HTMX (`?tab=`) |
| `objects/MBA.fmp12/layouts/35-Convencion.txt` | 18, 73-76, 80 | buttons | Ver Registros, sponsor prints, invoice prints | scripts 28, 86-89, 60, 61 | none | header actions menu "Imprimir ▾" linking to `/print/convention/...` |
| `objects/MBA.fmp12/layouts/40-Planes.txt` | 21 | field | `packages::event_key`; value list `package_active_events` | OnObjectSave → script 50 | none | event select; change posts the plan and builds items |
| `objects/MBA.fmp12/layouts/40-Planes.txt` | 5, 13 | field, button | `package_items::active` | script 49 toggle | CF: active = 1 → style | row checkbox "Incluir"; active rows in normal text, inactive rows muted |
| `objects/MBS.fmp12/layouts/43-Estudiante.txt` | 3, 4 | portal, field button | `student_certifications::certification_uuid` | script 48 (certificate) | none | certification progress table; "Certificado" link only when graduated |
| `objects/MBS.fmp12/layouts/43-Estudiante.txt` | 75, 90, 95, 115, 118, 138 | portals, buttons | course, seminar, short-course enrollments; Matricular en … | scripts 37-39, 41-43 | object flags (D-27) | one tab per kind with an "Matricular" button opening UI-20 |
| `objects/MBS.fmp12/layouts/43-Estudiante.txt` | 37, 50 | buttons | Show in Google Maps | Open URL | none | link EXT-02 |
| `objects/MBS.fmp12/layouts/82-Matricula_de_Curso.txt` | 21 | field | `course_enrollments::course_calendar_uuid`; value list `course_get_calendar_date` | OnObjectSave → script 7 commit | none | calendar select filled by `GET /calendar-options/course`; price shown on pick |
| `objects/MBS.fmp12/layouts/82-Matricula_de_Curso.txt` | 59 | button | Guardar | script 166 | none | "Guardar" submit; dialog closes; student tab refreshes |
| `objects/MBS_SendMail.fmp12/layouts/4-Emails.txt` | 65 | field | `emails::status` | none | CF: Draft / Sent / Error → style | status badge (neutral, green, red) with text |
| `objects/MBS_SendMail.fmp12/layouts/4-Emails.txt` | 70, 183, 150 | buttons | send one, send all, Preview | scripts 11, 18; Go to Layout | object flags (D-27) | row "Enviar"; header "Enviar todos" (confirm, JOB-01); row "Ver" |
| `objects/MBS.fmp12/layouts/57-Cursos_Especializados.txt` | 66 | button | Borrar | script 164 with access 0 (always denied) | none | no delete control; status select with Cancelado (BR-CRS-05) |
| `objects/MBS.fmp12/layouts/57-Cursos_Especializados.txt` | 75 | button | mail icon | script 173 (module ce) | none | "Enviar correo" on each calendar row → `POST /calendar/course/{id}/mail` |

Navigation that a script builds from a parameter: `global.navigation ( module )` (MBA script 8) and the `global.load*` scripts (MBS 10 to 15) pick the target layout by name or by the current view. The target shell has fixed nav tabs per module under the top bar, so no computed navigation remains.

## Navigation and workflows

| User workflow | Entry/route | Steps and states | Server operations | Role/record/field access | Source trace | Acceptance evidence |
|---|---|---|---|---|---|---|
| Register a person for the convention | UI-03 → UI-04 → UI-06 | find or create person; "Añadir registro"; pick event; pick plan (confirm if replacing); add items; add guests; record payment | OP-PPL-01, OP-REG-01, OP-REG-02, OP-REG-03, OP-REG-04, OP-REG-06, OP-REG-07 | ROLE-STAFF and ROLE-ADMIN (AP-03, AP-06, AP-09) | MBA scripts 21, 33, 41, 42, 44, 45, 35, 38 | T-07-01, T-07-02, T-07-03, T-05-03 |
| Set up an event and its plans | UI-08 → UI-09 → UI-13 | create event; add activities; add sponsors; create plan; pick event (builds items); toggle items | OP-EVT-01, OP-EVT-03, OP-SPN-01, OP-EVT-05, OP-EVT-04 | AP-03 | MBA scripts 26, 25, 48, 49, 50 | T-07-05, T-07-06 |
| Print convention documents | UI-09 → UI-34 | choose a document from "Imprimir ▾"; browser print | OP-RPT-01 | AP-10 | MBA scripts 59 to 91 (05 appendix §1) | T-07-17 |
| Door check-in | UI-09 → UI-07 | open door list; set Asistió / No Asistió / Canceló per row | OP-REG-10 | AP-03 | `objects/MBA.fmp12/layouts/55-Registro_Puerta.txt` | row status saved |
| Enroll a student | UI-14 → UI-15 → UI-20 | find student; "Matricular en curso"; pick offering; pick active date (price shown); save | OP-STU-05, OP-ENR-01..03, OP-ENR-04 | AP-03 | MBS scripts 45, 37-39, 166 | T-07-10, T-05-11 |
| Open a course date and e-mail the class | UI-18 → UI-19 → UI-26 | "Abrir curso"; set dates; later "Enviar correo" on the row; edit message; send all | OP-CRS-02, OP-CRS-03, OP-MAIL-01, OP-MAIL-02, OP-MAIL-04 | AP-03, AP-09 | MBS scripts 52, 173; MBS_SendMail 1, 18 | T-07-14, T-07-16, T-05-10 |
| Grades round trip | UI-21 → UI-22 | export XLSX; fill grades; upload; preview; apply | OP-ENR-06, OP-ENR-07 | AP-09, AP-10 | MBS scripts 127, 128 | T-07-12 |
| Graduation | UI-15, UI-23 | report of candidates; set graduation date; print certificate | OP-RPT-03, OP-CRT-02, OP-CRT-03 | AP-03, AP-10 | MBS scripts 92, 93, 48 | T-07-09 |
| Admin setup | UI-29 to UI-32 | users, settings, templates, certifications, professors, mail templates and profiles | OP-AUTH-04, OP-SYS-01, OP-SYS-02, OP-CRT-01, OP-CRS-04, OP-MAIL-06, OP-MAIL-07 | ROLE-ADMIN (AP-08) | D-19 | T-05-05 |

A trigger becomes an HTMX request to a server operation. Client visibility and route guards never replace the handler checks (AP-01 to AP-14).

## Target page specifications

### UI-01 Iniciar sesión (sign in)

- Readiness: behavior-ready. Route: `GET /login`, `POST /login`.
- Use case: sign in with username and password (D-50, BR-AUTH-01). No source layout (FileMaker asks for the account in its own dialog).
- States: empty, error (generic "Usuario o contraseña incorrectos"), locked (same message, AP-11).
- Accessibility: labels on both fields, autofocus on username, `autocomplete="username"` / `current-password`.

### UI-02 Inicio y estructura (shell and home)

- Readiness: behavior-ready. Route: `GET /`.
- Shell: top bar (app name, user menu with "Cambiar contraseña" and "Salir"), module tabs in the top bar and a nav tab bar per module: Convención (Convencionistas, Registros, Convenciones, Planes), Escuela (Estudiantes, Afiliados, Cursos especializados, Seminarios, Cursos cortos, Matrículas, Reportes), Correos (Envíos, Bandeja de salida). Admin module tab for ROLE-ADMIN only (visibility only; AP-08 guards the routes).
- Home: three module tiles with counts (active events, active students, draft batches). Source: the four nav buttons on every MBA layout (`global.navigation`, MBA script 8) and the MBS nav buttons (scripts 10 to 15); file triggers open MBA on Convención and MBS on the student list (BR-SYS-01).
- Session expiry after 10 idle minutes redirects to UI-01 (BR-AUTH-02).

### UI-03 Convencionistas (people list)

- Readiness: behavior-ready. Route: `GET /people`. Source: `objects/MBA.fmp12/layouts/26-Lista_de_Convencionistas.txt`, `objects/MBA.fmp12/layouts/28-Tabla_de_Convencionistas.txt`.
- List view and table view (`?view=`), search box (`hx-get` with `hx-trigger="input changed delay:300ms"`), "Nuevo" button. Row click opens UI-04.
- States: empty ("No hay convencionistas"), populated, no results.

### UI-04 Convencionista (person)

- Readiness: behavior-ready. Route: `GET /people/{id}`. Source: `objects/MBA.fmp12/layouts/25-Convencionista.txt`. Before screen: `mockups/convencionista.png`.
- Fields: Nombre, Apellido, Compañía (datalist from value list `people.company`), Teléfono, Teléfono personal, Fax, Dirección, Ciudad/Estado/Zip, E-mail, Nombre badge (`people` columns `name_first`, `name_last`, `company`, `phone1`, `phone2`, `fax`, `address`, `city`, `state`, `zip`, `email`, `name_badge`).
- Collection: Registros (event, place, start, end, status, total from `v_registration_totals`); row opens UI-06; "Añadir registro" (OP-REG-01); row delete only without payments (OP-REG-09).
- Delete person: ROLE-ADMIN, refused with registrations (BR-PPL-02).

### UI-05 Registros (registrations list)

- Readiness: behavior-ready. Route: `GET /registrations`. Source: `objects/MBA.fmp12/layouts/30-Lista_de_Registro.txt`, `objects/MBA.fmp12/layouts/31-Tabla_de_Registro.txt`.
- Filters: event (default: the latest Activo event), status, search. Columns: #, name, company, type (Convencionista / Participante, BR-REG-07), status, total, balance.
- "Ver Registros" on UI-09 opens this list filtered by the event (MBA script 28).

### UI-06 Registro (registration) — meaty

- Readiness: behavior-ready; implementation-ready after the M1 design page. Route: `GET /registrations/{id}`.
- Use case and users: staff record a registration, its plan, items, guests and payments.
- Source: `objects/MBA.fmp12/layouts/29-Registro.txt`, pickers `objects/MBA.fmp12/layouts/32-Planes_Activos.txt` and `objects/MBA.fmp12/layouts/33-Eventos_Activos.txt`; scripts MBA 35, 37, 38, 39, 41, 42, 44, 45. Before screen: `mockups/registro.png`, `mockups/registro.html`.
- Data queries: `registration_get` (05). Totals from `v_registration_totals`, item lines from `v_registration_item_lines`.
- Commands: OP-REG-01 to OP-REG-09. Invariant owners: BR-REG-02 (plan), BR-REG-03 (quantities), BR-REG-06 (attributes), BR-REG-09 (void).
- Hierarchy: title "Registro #<number> · <name> · <type>" with status badge; left column person block (read-only, link to UI-04) and header fields; right column summary card (Total, Pagos, Balance). Below: Evento and Plan line, then items table, guests list, payments table.
- States: loading (HTMX indicator on the changed block), empty items ("Sin actividades: escoja un plan o añada una actividad"), saving (footer "Guardando…"; a line change marks the row and the totals block until the server returns them), validation (inline, per field: value kept, red border, message; footer "1 campo sin guardar"; not saved), forbidden (void button absent for staff; route returns 403), conflict (none; last write wins per field).
- Save: per-field autosave (design system "Save model"); no Edit, Cancel or Guardar button on the page. Line fields (price, qty, no charge, attributes) patch the item and return the row plus the totals block out of band (`PATCH /registrations/{id}/items/{item_id}`).
- Keyboard: dialogs are native `<dialog>` with focus trap and Esc to close; Enter submits the inline payment form.
- Destructive actions: plan change with a plan set and item removal open a confirm dialog (05 "Confirmations"). Payment void asks for a reason.

#### Fields and controls

| Label | Data field | Control | Stored/display values | Required/validation owner | Read/write rule | Source evidence |
|---|---|---|---|---|---|---|
| Estatus | `registration.status` | select | value list `registration_status` (Nuevo, Registrado, Asistió, Canceló, No Canceló) | CHECK (BR-REG-10) | AP-03 | layout 29; `objects/MBA.fmp12/value-lists/2-registration_status.txt` |
| Costeado por | `registration.costeado_por` | radio | Afiliado / Individuo (value list `costeado_por`) | CHECK | AP-03 | layout 29 |
| Nombres badge (acompañantes) | `registration.guest_name_badges` | text | text | none | AP-03 | layout 29 |
| Comentarios | `registration.comments` | textarea | text | none | AP-03 | layout 29 |
| Evento | `registration.event_uuid` | button + dialog | active events only; button hidden once an event is set | BR-REG-01 | AP-03 | key 50 |
| Plan | `registration.package_uuid` | button + dialog | plans of the event | BR-REG-02 | AP-03, AP-09 | key 62 |
| Total, Pagos, Balance | `v_registration_totals.total`, `payment_total`, `balance` | read-only figures | dollars from cents | BR-REG-04, BR-REG-08 | read | `objects/MBA.fmp12/tables/133-registration.txt` #14, #19, #29 |
| Tipo | `v_registration_totals.registration_type` | badge | Convencionista / Participante | BR-REG-07 | read | #49, #50 |

#### Collections/portals

| Collection | Relationship/query | Columns/actions | Sort/filter/page | Create/delete rule | Empty/error behavior | Source evidence |
|---|---|---|---|---|---|---|
| Actividades | `registration_items` of this registration, `v_registration_item_lines` | type, name, start, end, price, qty, total, attributes (hotel reservation / nights / table), "Editar atributos", delete | event-item date | add: event items of the same event (OP-REG-04); delete with confirm (AP-06) | empty hint | key 2, key 11, value list `registrationSelectEventItems` |
| Acompañantes | `registration_guests` | name badge, delete | entry order | add/delete recompute qty, except qty set by hand (BR-REG-03, D-88) | "Sin acompañantes" | keys 54, 56 |
| Pagos | `payments` | date, amount, type, number, voided marker, "Anular" (admin) | created desc | add OP-REG-07; never deleted (AP-05) | "Sin pagos" | key 17, key 31 |

#### Conditional presentation

| Object key/component | Condition | Effect (style or hide) | Server rule cross-reference | Evidence |
|---|---|---|---|---|
| Pagos row | `voided_at` set | struck through, muted, "Anulado" badge | BR-REG-09 | product decision (D-09) |
| "Anular" button | role is not admin | not rendered | AP-05 | product decision |
| Attributes cell | item type Hotel / seating / other | shows reservation and nights, or table, or nothing | BR-REG-06 | `objects/MBA.fmp12/scripts/37-registration_items_edit_attributes.txt` |

### UI-07 Registro en puerta (door check-in)

- Readiness: behavior-ready. Route: `GET /events/{id}/door`. Source: `objects/MBA.fmp12/layouts/55-Registro_Puerta.txt`, value list `objects/MBA.fmp12/value-lists/8-registro_puerta_status.txt`.
- One row per registration of the event: #, name, company, guests, three status buttons (Asistió, No Asistió, Canceló) (OP-REG-10). Large touch targets (inference: used at a door on a tablet). Search box filters rows client-side.

### UI-08 Convenciones (events list)

- Readiness: behavior-ready. Route: `GET /events`. Source: layouts MBA 37, 38. Columns: name, place, start, end, status, registrations, total.

### UI-09 Convención (event) — meaty

- Readiness: behavior-ready; implementation-ready after the M1 design page. Route: `GET /events/{id}?tab=`.
- Source: `objects/MBA.fmp12/layouts/35-Convencion.txt` (also 98 view, 99 edit: one page with an edit mode); scripts MBA 25, 28, 60, 61, 86-89, 93. Before screens: `mockups/convencion.png` (Actividades), `mockups/convencion-auspiciadores.png` (Auspiciadores), `mockups/convencion-facturacion.png` (Facturación), `mockups/convencion.html`.
- Data: `event_get`; header totals from `v_event_totals` (registrations, guests, total people, revenue, payments, balance, sponsor promised, received, total) (BR-EVT-04).
- Commands: OP-EVT-01, OP-EVT-02, OP-EVT-03, OP-SPN-01, OP-SPN-02; print links (OP-RPT-01); accounting CSV (OP-RPT-04).
- Hierarchy: header (name, place, dates, status, PDF chip) with stat tiles; action bar "Ver registros", "Registro en puerta", "Imprimir ▾" (Lista de facturación, Registros individuos, Registros afiliados, badges, labels, cartas), "Exportar contabilidad (CSV)"; tabs below.
- States: tab loads with HTMX (`hx-get ...?tab=sponsors`, `hx-push-url`); empty tab hints; delete refused when registrations exist (409 message).
- Save: per-field autosave (design system "Save model"); no Edit, Cancel or Guardar button on the page. Header fields return the stat tiles out of band.

#### Fields and controls

| Label | Data field | Control | Stored/display values | Required/validation owner | Read/write rule | Source evidence |
|---|---|---|---|---|---|---|
| Nombre | `events.name` | text | text | required | AP-03 | layout 35 |
| Lugar | `events.location` | text | text | none | AP-03 | layout 35 |
| Comienza / Termina | `events.date_start`, `events.date_end` | date | ISO date; end defaults to start | BR-EVT-01 | AP-03 | `objects/MBA.fmp12/tables/131-events.txt` #15 |
| Estatus | `events.status` | select | value list `event_status` | BR-EVT-02 | AP-03 | layout 35 |
| PDF | `events.pdf_file_uuid` | file chip | `stored_files` | BR-SYS-04 | AP-03 | `objects/MBA.fmp12/scripts/13-event.deletePDF.txt` |
| Registros / Acompañantes / Total | `v_event_totals.count_registrations`, `count_guests`, `count_total` | stat tiles | integers | BR-EVT-04 | read | #21-#31 |

#### Collections/portals

| Collection | Relationship/query | Columns/actions | Sort/filter/page | Create/delete rule | Empty/error behavior | Source evidence |
|---|---|---|---|---|---|---|
| Actividades | `event_items` of the event | start, end, type, name, place/room, price; "Ver detalle" (UI-10); delete | date_start, time_start asc | "Añadir actividad" (BR-EVT-03); delete refused when used (FK) | hint | key 24, key 21, key 37, key 98 |
| Auspiciadores | `sponsor` of the event | name, category, requested, promised, received; "Ver" (UI-11); letter and label prints per sponsor | sponsor_name asc | add inline; delete (AP-06) | hint | key 52, keys 73-79, key 99 |
| Facturación | `billing` of the event | timestamp, file link, delete | timestamp desc | upload (OP-SPN-02, D-16) | "Sin archivos" | key 128 |

#### Conditional presentation

| Object key/component | Condition | Effect (style or hide) | Server rule cross-reference | Evidence |
|---|---|---|---|---|
| Status badge | status Activo / other | green / neutral | BR-EVT-02 | value list `event_status` |
| Delete button | role is not admin | not rendered | AP-04 | product decision |

### UI-10 Detalle de actividad (event item)

- Readiness: behavior-ready. Route: `GET /event-items/{id}`. Source: `objects/MBA.fmp12/layouts/36-Detalle_de_Evento.txt`.
- Item fields (type, name, place, dates, times, price) and the registrations that include it (portal key 18), with a print link to "Lista de facturación por actividad".

### UI-11 Auspiciador (sponsor)

- Readiness: behavior-ready. Route: `GET /sponsors/{id}`. Source: `objects/MBA.fmp12/layouts/45-Auspiciadores.txt`.
- Fields of `sponsor` (name, category, contact, title, greeting, address, phone, requested, promised, received, logo). Print links: letter ES / EN, labels.

### UI-12 Planes (plans list)

- Readiness: behavior-ready. Route: `GET /plans`. Source: layouts MBA 41, 44. Filter by event. Columns: event, plan, price (`v_package_price`).

### UI-13 Plan — meaty

- Readiness: behavior-ready; implementation-ready after the M1 design page. Route: `GET /plans/{id}`.
- Source: `objects/MBA.fmp12/layouts/40-Planes.txt`; scripts MBA 48, 49, 50. Before screen: `mockups/planes.png`, `mockups/planes.html`.
- Data: `plan_get`; price from `v_package_price` (BR-EVT-05).
- Commands: OP-EVT-05, OP-EVT-04 (on event change and "Reconstruir").
- Hierarchy: title "Plan · <event> · <plan>", event select, name, price override; items table "Detalles" with Incluir checkbox, type, name, place, qty, start, end; price summary.
- States: event without items -> message "El Evento seleccionado no tiene detalles..." (BR-EVT-06); no event -> "Favor de seleccionar el evento."
- Save: per-field autosave (design system "Save model"); no Edit, Cancel or Guardar button on the page. Incluir and qty patch the plan item and return the plan price out of band.

#### Fields and controls

| Label | Data field | Control | Stored/display values | Required/validation owner | Read/write rule | Source evidence |
|---|---|---|---|---|---|---|
| Evento | `packages.event_uuid` | select | active events (value list `package_active_events`) | BR-EVT-02 | AP-03 | key 21 |
| Plan | `packages.name` | text | text | required | AP-03 | layout 40 |
| Precio fijo | `packages.price_override` | money | cents; empty = computed | BR-EVT-05 | AP-03 | `objects/MBA.fmp12/tables/136-packages.txt` #13 |
| Precio | `v_package_price` | read-only | dollars | BR-EVT-05 | read | #13 |

#### Collections/portals

| Collection | Relationship/query | Columns/actions | Sort/filter/page | Create/delete rule | Empty/error behavior | Source evidence |
|---|---|---|---|---|---|---|
| Detalles | `package_items` of the plan with event item data | Incluir (toggle), type, name, place, qty, price, dates; delete | event item order | build adds missing items only (BR-EVT-06); delete (AP-06) | messages above | key 4, key 13, key 18 |

#### Conditional presentation

| Object key/component | Condition | Effect (style or hide) | Server rule cross-reference | Evidence |
|---|---|---|---|---|
| Item row (key 5) | `package_items.active` = 1 | normal text and a check; inactive rows muted | BR-EVT-05 | `objects/MBA.fmp12/layouts/40-Planes.txt` key 5 |

### UI-14 Estudiantes (students list)

- Readiness: behavior-ready. Route: `GET /students`. Source: `objects/MBS.fmp12/layouts/42-Lista_de_Estudiantes.txt`, `objects/MBS.fmp12/layouts/45-Tabla_de_Estudiantes.txt`; Quick Find override (MBS script 45).
- Search: 0 hits shows a message, 1 hit opens UI-15, more shows the list (OP-STU-05). Columns: name, company, e-mail, phone, status (ACTIVO / INACTIVO from `v_student_academic`).

### UI-15 Estudiante (student) — meaty

- Readiness: behavior-ready; implementation-ready after the M1 design page. Route: `GET /students/{id}?tab=`.
- Source: `objects/MBS.fmp12/layouts/43-Estudiante.txt`; scripts MBS 33-35, 37-39, 41-43, 48; menus New Record / Duplicate Record overrides. Before screen: none.
- Data: `student_get`: student, affiliate, `v_student_academic`, `v_student_certification_progress`, enrollments per kind, interests.
- Commands: OP-STU-01 to OP-STU-04, OP-CRT-02, OP-ENR-01 to OP-ENR-03 (through UI-20), print certificate (OP-CRT-03), transcript (`/print/school/transcripcion-de-creditos`).
- Hierarchy: header (name, company, status badge, affiliate badge when the affiliate is ACTIVO); summary tiles (average, credits, hours, last course date) (BR-STU-06); tabs Certificaciones, Cursos especializados, Seminarios, Cursos cortos, Datos personales.
- States: new student shows Datos personales only until saved; delete refused with enrollments (409).
- Save: per-field autosave (design system "Save model"); no Edit, Cancel or Guardar button on the page. The "Nuevo estudiante" dialog keeps Guardar (it creates the record).

#### Fields and controls

| Label | Data field | Control | Stored/display values | Required/validation owner | Read/write rule | Source evidence |
|---|---|---|---|---|---|---|
| Nombre | `students.nombre` | text | text | required | AP-03 | layout 43 |
| Afiliado | `students.member_uuid` | typeahead (`GET /affiliates/options`) | affiliate name; sets `compania` | BR-STU-02 | AP-03 | value list `affiliates_by_name` |
| Compañía | `students.compania` | text | text | none | AP-03 | layout 43 |
| E-mail, E-mail trabajo | `students.email`, `students.email_work` | email | text | none | AP-03 | layout 43 |
| Teléfonos | `students.telefono_r`, `students.telefono_t`, `students.phone_mobile` | tel | text | none | AP-03 | layout 43 |
| Dirección residencial | `students.dir_residencial`, `residencial_2`, `home_city`, `home_state`, `home_zip` | text + map link (EXT-02) | text | none | AP-03 | keys 37, 50 |
| NMLS | `students.nmls` | text | text | none | AP-03 | layout 43 |
| Contacto de emergencia | `students.emergency_contact`, `emergency_phone`, `emergency_contact_relationship` | text | text | none | AP-03 | layout 43 |
| Certificaciones de interés | `student_certification_interests` | chips + add select | up to 6 | BR-STU-05 | AP-03, AP-06 | `objects/MBS.fmp12/tables/131-students.txt` #23, #24 |

No SSN field exists (D-06).

#### Collections/portals

| Collection | Relationship/query | Columns/actions | Sort/filter/page | Create/delete rule | Empty/error behavior | Source evidence |
|---|---|---|---|---|---|---|
| Certificaciones | `v_student_certification_progress` | certification, total, completed, left, courses left, average, credits, hours, graduation date (editable), "Certificado" | certification name | rows created with the student (BR-STU-01); graduation via OP-CRT-02 | none | key 3, key 4 |
| Cursos especializados | `enrollments_course` + calendar + course | course, date, status, price, paid, balance, score, letter; row opens UI-20 | course date desc | "Matricular en curso" (OP-ENR-01) | "Sin matrículas" | keys 75, 88, 90 |
| Seminarios | `enrollments_seminars` | seminar, date, status, price, balance | id desc | "Matricular en seminario" (OP-ENR-02) | as above | keys 95, 105, 115 |
| Cursos cortos | `enrollments_short_course` | course, date, status, price, balance | id desc | "Matricular en curso corto" (OP-ENR-03) | as above | keys 118, 119, 138 |

#### Conditional presentation

| Object key/component | Condition | Effect (style or hide) | Server rule cross-reference | Evidence |
|---|---|---|---|---|
| "Certificado" link | graduation date empty | not rendered | BR-CRT-04 | `objects/MBS.fmp12/scripts/48-students_print_certification.txt` |
| Status badge | ACTIVO / INACTIVO | green / neutral | BR-STU-03 | `objects/MBS.fmp12/scripts/21-sase.updateStudentRecord.txt` |
| Enroll buttons, portals (object flags) | unknown | shown and active | none | D-27 |

### UI-16 Afiliados (affiliates list) and UI-17 Afiliado (affiliate)

- Readiness: behavior-ready. Routes: `GET /affiliates`, `GET /affiliates/{id}`. Source: `objects/MBS.fmp12/layouts/49-Lista_de_Afiliados.txt`, `objects/MBS.fmp12/layouts/50-Tabla_de_Afiliados.txt`, `objects/MBS.fmp12/layouts/48-Afiliado.txt`.
- UI-16 filters by type (ACTIVO / INACTIVO) and links the affiliate print pages (list, directory, members, e-mails, labels). UI-17 holds the `affiliates` fields and the list of its students; delete unlinks students (BR-AFF-02).

### UI-18 Ofrecimientos (courses, seminars, short courses)

- Readiness: behavior-ready. Routes: `GET /offerings/{kind}`, `GET /offerings/{kind}/{id}`. Source: `objects/MBS.fmp12/layouts/57-Cursos_Especializados.txt`, `objects/MBS.fmp12/layouts/60-Seminarios.txt`, `objects/MBS.fmp12/layouts/64-Cursos_Cortos.txt`.
- One page design for three kinds. Tabs Calendario and Detalles. Calendario rows: date, semester, status, member price, individual price, enrollments, quorum, needed (BR-CRS-03), revenue; actions "Fechas" (UI-19), "Ver matrícula" (UI-21, OP-CRS-05), "Enviar correo" (OP-MAIL-01). "Abrir curso" adds a row (BR-CRS-02). No delete for calendar rows (BR-CRS-05, AP-07).
- Short courses show the NMLS total (BR-CRS-04).

### UI-19 Fecha del curso (calendar row dialog)

- Readiness: behavior-ready. Route: `GET /calendar/{kind}/{id}`. Source: `objects/MBS.fmp12/layouts/58-Fechas_de_Curso.txt`, `objects/MBS.fmp12/layouts/61-Fechas_del_Seminarios.txt`, `objects/MBS.fmp12/layouts/65-Fechas_del_Curso_Corto.txt`.
- Status select (`course_statuses`), prices, professor (value list from `professors`), class dates list with day type Día de Clase / Día de Fiesta (BR-CRS-06). Short course: up to 15 dates (D-26, `calendar_short_course_dates`).

### UI-20 Matrícula (enrollment dialog) — meaty

- Readiness: behavior-ready; implementation-ready after the M1 design page. Routes: `GET /students/{id}/enrollments/{kind}/new`, `GET /enrollments/{kind}/{id}`.
- Source: `objects/MBS.fmp12/layouts/82-Matricula_de_Curso.txt`, `objects/MBS.fmp12/layouts/103-Matricula_de_Seminario.txt`, `objects/MBS.fmp12/layouts/113-Matricula_de_Curso_Corto.txt`; scripts MBS 37-39, 165, 166. Before screen: none.
- Data: `enrollment_get`, `calendar_options` (active rows only, BR-CRS-01).
- Commands: OP-ENR-01 to OP-ENR-05, OP-ENR-08.
- Hierarchy: dialog (wide). Top: offering select, then date select (HTMX loads options); price card shows member or individual price and why ("Afiliado ACTIVO: precio de socio") (BR-ENR-01). Middle: payer, payment, date of payment, check number, invoice number, notes. Course kind: Notas section with Examen, Primera to Octava, Promedio asignaciones, Asistencia y puntualidad, Promedio final, Puntuación with live letter and pass badge (BR-ENR-03 to BR-ENR-05). Footer: Guardar, Dar de baja, Borrar (confirm; refused with payment). The enrollment dialog keeps Guardar: it creates or settles an enrollment (payment, grades) and validates them together.

#### Fields and controls

| Label | Data field | Control | Stored/display values | Required/validation owner | Read/write rule | Source evidence |
|---|---|---|---|---|---|---|
| Curso / Fecha | `enrollments_course.course_uuid`, `course_calendar_uuid` | selects | active rows only | BR-CRS-01 | AP-03 | key 21; value lists `course_uuids`, `course_get_calendar_date` |
| Costo | `enrollments_course.costo` | read-only on create | cents, stored at pick | BR-ENR-01 | read after create | `objects/MBS.fmp12/tables/139-enrollments_course.txt` #10 |
| Costeado por | `enrollments_course.costeado_por` | select | COMPAÑIA / INDIVIDUAL (`course_enrollment_paid_by`) | CHECK | AP-03 | layout 82 |
| Pago, Fecha de pago | `pago`, `fecha_de_pago` | money, date | cents | none | AP-03 | keys 16, 24 (object flag, D-27) |
| Balance | `v_enrollments_course_lines` | read-only | dollars | BR-ENR-02 | read | #11 |
| Puntuación | `puntuacion` | number 0-100 | integer | CHECK + form (BR-ENR-03) | AP-03 | #57 |
| Examen, Primera … Octava, promedios | `examen`, `primera` … `octava`, `promedio_asignaciones`, `asistencia_y_puntualidad`, `promedio_final` | numbers | numeric | none | AP-03 | layout 82 |
| Estatus | `status` | select | ACTIVO, BAJA, … | BR-ENR-09 | AP-03, AP-05 | #35 |

#### Conditional presentation

| Object key/component | Condition | Effect (style or hide) | Server rule cross-reference | Evidence |
|---|---|---|---|---|
| Letter and pass badge | score set | letter A-F; "Aprobado" when > 69 | BR-ENR-04, BR-ENR-05 | #28, #82 |
| Borrar | payment > 0 | disabled with a hint "Use Dar de baja" | BR-ENR-08 | script 165 |
| Notas section | kind is not course | not rendered | none | layouts 103, 113 have no grade fields (fact) |

### UI-21 Matrículas (enrollment lists)

- Readiness: behavior-ready. Routes: `GET /enrollments/{kind}`, `GET /calendar/{kind}/{id}/enrollments`. Source: layouts MBS 81, 102, 111.
- Filters: calendar row, status, search. Actions: "Exportar notas (XLSX)" (OP-ENR-06), "Importar notas" (UI-22), CSBS CSV (short courses, BR-RPT-06), print links for the calendar row (lists, attendance, certificates, labels, letters; 05 appendix §1).

### UI-22 Importar notas (grades import)

- Readiness: behavior-ready. Route: `GET /grades/import`. New screen (changed: preview step, D-23).
- Steps: upload XLSX → preview table (matched rows with old → new values; unmatched serials in a separate list) → "Aplicar" (OP-ENR-07). Nothing changes before "Aplicar".

### UI-23 Reportes administrativos (reports)

- Readiness: behavior-ready. Routes: `GET /reports`, `GET /reports/{code}`. Source: `objects/MBS.fmp12/layouts/137-Reportes_Administrativos.txt` (and 68, merged).
- Criteria form per report (years, date range, certifications, number of courses left `left`, BR-RPT-07) in the query string (D-33); report years come from the data (BR-RPT-11). 8 reports (05 appendix §2). Each result page has print CSS.

### UI-24 Panel de matrículas (enrollment dashboard)

- Readiness: inventory-only (charts not specified by the owner, D-17). Route: `GET /dashboard/enrollments`. Source: layouts MBS 75, 76, 77, 78, 139, 141.
- Year range; counts per status and per offering type (BR-RPT-02, D-13 default); revenue by payer (BR-RPT-03). Charts as inline SVG drawn on the server (no chart library).

### UI-25 Envíos (mail batches)

- Readiness: behavior-ready. Route: `GET /mail/batches`. Source: `objects/MBS_SendMail.fmp12/layouts/7-Batches.txt`.
- Columns: title, created, sender, e-mails, sent, errors, summary text ("n enviado(s)" plus " m error(es)" when errors > 0, "Nuevo" when nothing is sent; BR-MAIL-05).

### UI-26 Envío de correos (batch and e-mails) — meaty

- Readiness: behavior-ready; implementation-ready after the M1 design page. Route: `GET /mail/batches/{id}`.
- Source: `objects/MBS_SendMail.fmp12/layouts/4-Emails.txt`; scripts MBS_SendMail 1, 8, 11, 17, 18, 21. Before screen: none.
- Data: `batch_get`; counts from `v_mail_batch_counts`.
- Commands: OP-MAIL-02, OP-MAIL-03, OP-MAIL-04 (JOB-01), OP-MAIL-05; integration EXT-01.
- Hierarchy: mock banner when no `RESEND_API_KEY` ("Modo de prueba: ningún correo sale del servidor; vea la bandeja de salida"); batch header (title, sender profile, subject, message editor, attachment chip); counts; e-mail table (name, to, status badge, sent on, error first line, actions Enviar / Ver / Borrar). "Enviar todos" opens a confirm with the count of unsent e-mails, then shows a progress bar that polls every second.
- States: draft, sending (buttons disabled, progress), done, partial errors (error rows red with the message), forbidden (none: both roles, AP-03).
- Save: per-field autosave (design system "Save model"); no Edit, Cancel or Guardar button on the page. Batch fields patch only while no e-mail of the batch is sent.

#### Fields and controls

| Label | Data field | Control | Stored/display values | Required/validation owner | Read/write rule | Source evidence |
|---|---|---|---|---|---|---|
| Título | `mail_batches.title` | text | default "Sin título" | BR-MAIL-01 | AP-03 | `objects/MBS_SendMail.fmp12/tables/132-batches.txt` #23 |
| Remitente | `mail_batches.from_email` | select | `mail_profiles` | BR-MAIL-01 | AP-03 | value list `profiles` |
| Asunto | `mail_batches.subject` | text | from template | BR-MAIL-01 | AP-03 | #10 |
| Mensaje | `mail_batches.body_entry` | textarea | plain text, rendered as HTML into `<SCI_BODY>` | BR-MAIL-03 | AP-03 | `objects/MBS_SendMail.fmp12/scripts/17-emails.setBody.txt` |
| Adjunto | `mail_batches.attachment_file_uuid` | file chip | `stored_files` | BR-SYS-04 | AP-03 | key 190, key 192 |

#### Collections/portals

| Collection | Relationship/query | Columns/actions | Sort/filter/page | Create/delete rule | Empty/error behavior | Source evidence |
|---|---|---|---|---|---|---|
| Correos | `mail_emails` of the batch | name, to, cc, status, sent on, error; Enviar, Ver, Borrar | name | created by OP-MAIL-01; delete only while not sent (AP-06) | "Sin correos" | layout 4 (list body) |

#### Conditional presentation

| Object key/component | Condition | Effect (style or hide) | Server rule cross-reference | Evidence |
|---|---|---|---|---|
| Status badge (key 65) | Draft | neutral badge "Borrador" | BR-MAIL-05 | `objects/MBS_SendMail.fmp12/layouts/4-Emails.txt` key 65 |
| Status badge (key 65) | Sent | green badge "Enviado" | BR-MAIL-05 | key 65 |
| Status badge (key 65) | Error | red badge "Error" + message | BR-MAIL-04 | key 65 |
| Mock banner | no `RESEND_API_KEY` | shown | BR-MAIL-08 | D-05 |

### UI-27 Vista previa (e-mail and template preview)

- Readiness: behavior-ready. Routes: `GET /mail/emails/{id}/preview`, `GET /admin/mail/templates/{id}/preview`. Source: `objects/MBS_SendMail.fmp12/layouts/10-Email_Preview.txt`, `objects/MBS_SendMail.fmp12/layouts/9-Template_Preview.txt`.
- Rendered HTML in a sandboxed `iframe srcdoc` (no scripts, no same-origin).

### UI-28 Bandeja de salida (outbox)

- Readiness: behavior-ready. Routes: `GET /mail/outbox`, `GET /mail/outbox/{id}`. New screen (D-05, OP-MAIL-08).
- List: time, provider (mock / resend), to, subject, status, error. Detail: headers, HTML preview, attachment link. The API key never appears (AP-13).

### UI-29 Usuarios (users) — admin

- Readiness: behavior-ready. Route: `GET /admin/users`. New screen (D-50, OP-AUTH-04).
- Table: username, name, role, active, locked until, last sign-in; inline edit row; "Nueva contraseña" dialog. The last active admin cannot be disabled or demoted (AP-11).

### UI-30 Configuración (settings, document templates, value lists) — admin

- Readiness: behavior-ready. Routes: `GET /admin/settings`, `GET /admin/templates/{code}`, `GET /admin/value-lists`. Source: `objects/MBS.fmp12/layouts/165-Globals.txt` (globals become settings); 25 print layouts with fixed text (D-18).
- Tabs: Organización (name, logo, signers), Escuela (NMLS rate, CSBS constants), Correo (default template), Sesión (idle minutes), Plantillas de documentos (one editor per letter, certificate and policy), Listas de valores.

### UI-31 Plantillas y remitentes de correo — admin

- Readiness: behavior-ready. Routes: `GET /admin/mail/templates`, `GET /admin/mail/profiles`. Source: `objects/MBS_SendMail.fmp12/layouts/8-Templates.txt`, `objects/MBS_SendMail.fmp12/layouts/12-profiles.txt`.
- Template editor (name, subject, HTML with the four tokens listed beside it, BR-MAIL-03) with preview (UI-27). Profiles: e-mail (unique, BR-MAIL-09), name, title, default.

### UI-32 Certificaciones y profesores — admin

- Readiness: inventory-only (no caller in the source; D-19). Routes: `GET /admin/certifications`, `GET /admin/professors`. Source: `objects/MBS.fmp12/layouts/59-Certificaciones.txt`, `objects/MBS.fmp12/layouts/66-Tabla_de_Profesores.txt`.
- Certification with requirement rows (course, type Requisito / Electiva, BR-CRT-01). Professors: a name list.

### UI-33 Cambiar contraseña (my password)

- Readiness: behavior-ready. Route: `GET /account/password`. Current, new, repeat (OP-AUTH-03, AP-12).

### UI-34 Impresos de convención (convention print pages)

- Readiness: behavior-ready; "Lista de Facturación" is meaty and gets an M1 design page. Route: `GET /print/convention/{doc}`. 25 pages (05 appendix §1).
- Source for the meaty one: `objects/MBA.fmp12/layouts/57-Lista_de_Facturacion.txt`; script `objects/MBA.fmp12/scripts/64-registrations.print_Lista_de_Facturacion.txt`. Before screen: `mockups/lista-facturacion.png`, `mockups/lista-facturacion.html`.
- Shared print frame: screen toolbar (Imprimir, Volver) hidden in `@media print`; letter size; page breaks per registration (`break-after: page` where the layout had a sub-summary with page break); logo and signer from `app_settings`; fixed texts from `document_templates` (D-18). Badges, labels and tickets use a CSS grid with the label size in mm; quantity from `?qty=` (BR-RPT-10). Hotel-only filter default on, with a toggle (OD-07-01).
- Lista de Facturación layout: header (org, event, "Lista de Facturación"); per registration: number, name, invoice, status, company, plan; item table (activity, start, end, cost, people, total); totals per registration and grand total (BR-REG-04, BR-REG-05).
- States: no records -> a message page "No hay registros para imprimir" (changed: some source scripts close the window with no message, for example MBA 60 and 61, BR-RPT-01; OP-RPT-01).

### UI-35 Impresos de la escuela (school print pages)

- Readiness: behavior-ready. Route: `GET /print/school/{doc}`. 47 pages (05 appendix §1): enrollment lists, attendance sheets, policies and acknowledgments, letters, certificates, labels, transcript, affiliate lists, graduates.
- Same print frame as UI-34. Spanish dates and lists use `format_date_es_long` and `list_to_sentence_es` (BR-RPT-08). White-text hiding on `Print Estudiante` becomes conditional rendering (D-39).

## Design system and responsive decisions

| Decision | Value | Source fact or product decision | Acceptance check |
|---|---|---|---|
| Typography/color/spacing | One CSS file, system font stack, 14 px base for data screens, neutral grays with one accent color; status colors green / red / amber / neutral with text labels (never color alone) | product decision (Step 5 default, D-74 not answered) | design pages in M1 approved by the operator |
| Breakpoints/input devices | Desktop first (≥ 1280 px); usable down to 1024 px; UI-07 door check-in works on a tablet (≥ 768 px) | inference: office staff on desktops (D-72 default) | manual check at 1280, 1024, 768 |
| Data-density/table behavior | Dense tables (32 px rows), sticky header, 50 rows per page, sortable where the source sorted | FileMaker list layouts (fact) | lists render 50 rows |
| Modal/tab/navigation behavior | Native `<dialog>` for pickers and enrollment; tabs are links with `hx-get` + `hx-push-url`; fixed module tabs in the top bar and nav tabs per module; three nav levels, each with one mark: module = a filled block in the nav bar colour (no underline), section = a stone tab that opens into the page bar, record tab = the teal top rule (only this level uses teal) | layouts 32, 33, 58, 65, 82 open as windows (fact) | back button restores the tab |
| Print/PDF behavior | Print CSS pages; browser print or save as PDF | `decisions.md` | Lista de Facturación prints on letter paper with page breaks |
| Accessibility target | WCAG 2.2 AA for the meaty screens: labels on every control, focus visible, keyboard path through dialogs, contrast ≥ 4.5:1 | Step 5 default (D-75 not answered) | keyboard walk of UI-06 and UI-20 |
| Save model | Record pages are always editable; no Edit, Cancel or Guardar button. A changed field saves on change/blur: htmx sends only that field (`hx-patch`, `hx-trigger="change"`, 05 "Per-field autosave"); the server validates, saves and returns the field fragment, and for a line field also the server-computed totals (`hx-swap-oob`). Error: the value stays as typed, red border and message, not saved. Esc reverts the field to its saved value before the user leaves it (FileMaker Revert Record). The fixed footer shows "Guardando…", "Guardado h:mm p. m." or "N campo(s) sin guardar" in a fixed-width slot. Last write wins per field; every change leaves an `audit_log` row. Guardar stays only in dialogs that create or settle a record (payment, enrollment, new student, new user) and in submit forms (sign-in, password change). | operator decision (2026-11-21) | `design/registro.html?state=guardando`, `?state=sin-guardar`; T-05-13, T-05-14 |
| JavaScript | htmx (vendored) + `app.js` < 250 lines: dialog open/close, focus return, Esc (closes a dialog; in a changed field, reverts it), `Ctrl+S` saves the focused field now or submits the dialog form, save-state footer from htmx events, confirm-dialog wiring | `decisions.md`, operator decision (2026-11-21) | pages work with JS off except dialogs and partial swaps |

## Design milestone (M1) inputs

The design milestone produces static HTML design pages under `design/` (shape of migrate-karbonfm `design/`): shared partials (head, top bar with module tabs, nav tabs, footer), one CSS file, a component set (`design/components.html`), and a page shell for every kept screen (UI-01 to UI-35; full list in `03_migration_plan.md` "M1 page list"), with the same synthetic data as the mockups. The table below lists the pages drawn in full. The promo video (M2, episode 1) is made from the design pages and compares each before mockup with its design page. Its input pack and the video live in the private promo kit, vault and share, not in this repository.

| Design page | Screen | Before screen |
|---|---|---|
| `design/convencionista.html` | UI-04 | `mockups/convencionista.png` |
| `design/registro.html` | UI-06 | `mockups/registro.png` |
| `design/convencion.html` (3 tabs) | UI-09 | `mockups/convencion.png`, `mockups/convencion-auspiciadores.png`, `mockups/convencion-facturacion.png` |
| `design/planes.html` | UI-13 | `mockups/planes.png` |
| `design/lista-facturacion.html` | UI-34 | `mockups/lista-facturacion.png` |
| `design/estudiante.html` | UI-15 | none |
| `design/matricula.html` | UI-20 | none |
| `design/correos.html` | UI-26 | none |

## Missing evidence

| Layout/object | Missing evidence | Implementation risk | Required screenshot/decision | Owner/status |
|---|---|---|---|---|
| High-bit object flags (layouts MBA 98, MBS 43, 82, 103, 113, 172, MBS_SendMail 4) | flag meaning | a control hidden in FileMaker shows in the target | D-27 | open; default visible |
| School and e-mail screens | no before screen | design pages start from the DDR only | none needed for the demo; design pages in M1 | accepted |
| Chart layouts (MBS 75-78, 139, 141) | which charts users want | dashboard content guessed | D-17 | open; UI-24 inventory-only |
| Certifications and professors screens (MBS 59, 66) | how users maintain them today | admin screen shape guessed | D-19 | open; default admin screens |
| Settings button on 12 MBA layouts | deleted script | none | D-32 | accepted; no control |
| Fixed letter and policy texts (25 print layouts) | which texts must be editable | template list | D-18 | default: all editable |

## Verification

- [x] Every in-scope layout file has a target disposition (`06_ui_spec_appendix.md` §1: 181 of 181).
- [x] Every in-scope target page has an evidence readiness level (UI-01 to UI-35).
- [x] Every hide rule, conditional format, trigger and button in the layout explorer has a target behavior: 0 hide rules; the 11 format lines on 4 layouts (07 appendix §4) map to UI-13, UI-26, UI-35 (D-39) and the retired `virtual_table`; the 5 object triggers map to UI-06, UI-13 and UI-20; buttons map through their scripts (07 appendix §1) and the object map above.
- [x] Server authorization negative tests exist for hidden or restricted operations and fields (T-05-05, T-05-06, T-05-07; `08` negative tests).
- [ ] Critical workflows walked against the current app: blocked. Only fake-data mockups of the convention module exist; the design milestone walks UI-04, UI-06, UI-09, UI-13 and UI-34 against them (D-73 not answered).

## Open decisions

| ID | Decision | Default for the demo | Affects |
|---|---|---|---|
| OD-06-01 | UI language | Spanish labels, as the source | all screens |
| OD-06-02 | One offering page for three kinds | yes (UI-18, UI-19, UI-20) | school screens |
| OD-06-03 | Dashboard content (D-17) | counts and revenue per year and type | UI-24 |
| OD-06-04 | Door check-in as its own screen | yes, UI-07 | OP-REG-10 |
| OD-06-05 | Object-flag controls (D-27) | visible and active | UI-15, UI-20, UI-26 |
