# Server Operation / API Design

Interface style (product decision, operator 2026-11-21, `decisions.md`; `02_recommendations.md` confirms it): one Rust web server (axum + askama + rusqlite) that renders HTML on the server. Each route handler checks the session and policy, calls one operation of the domain layer (`07_business_logic.md`), and returns HTML: a full page for a normal request, or a fragment for an HTMX request (`HX-Request` header). A small vanilla JS file handles focus, keyboard and `<dialog>` only. No SPA, no JSON API, no CRUD route per base table.

Labels: **fact** = read from the object file cited; **product decision** = operator answer or Step 5 default (D-id given); **inference** = drawn from names or structure. Operations OP- and rules BR- are defined in `07_business_logic.md`. Policies AP- and roles ROLE- are defined in `08_auth_roles.md`. Tables, columns and views are in `04_database_schema.sql`. Screens UI- are in `06_ui_spec.md`. Increments I- are in `03_migration_plan.md`.

Full list of print pages (one route per print layout): `05_api_design_appendix.md`.

## Evidence and boundary

| Item | Value |
|---|---|
| Explorer CSVs used | scripts, layouts, tables, graph, value-lists, menus, security (`explorers/*/csv`) |
| Selected interface style and reason | Server-rendered HTML routes with HTMX partials (product decision, `decisions.md`). All rules run in Rust on the server. One process, one SQLite file (D-01). No JS build step. |
| Consumers/clients/integrations | Browsers of signed-in staff and admins. Outbound: Resend API (EXT-01) or the `mail_outbox` mock. Browser link to Google Maps (EXT-02). No inbound integration. |
| Authentication mechanism | Local username + argon2id password, server session cookie, CSRF token on every write (D-50; AP-01, AP-02, AP-11). |
| Authorization design reference | `08_auth_roles.md` (ROLE-ADMIN, ROLE-STAFF; AP-01 to AP-14) |
| Unavailable/open contracts | The writer of `billing` rows is unknown (D-16): the target adds an upload. High-bit object flags undecoded (D-27): all controls visible. The deleted MBA "settings" script (D-32): no route. FileMaker data import: out of scope (demo, D-02). |

## Route conventions

- **Pages and fragments.** Every screen of `06` has a `GET` page route. With `HX-Request`, the handler returns only the main fragment (or the named target, for example one tab). Without it, the same handler wraps the fragment in the base layout. Every URL works on reload and with the back button.
- **Methods.** Reads are `GET`. Creates and commands are `POST`. A change to one field of an existing record is `PATCH` (per-field autosave, below). Deletes are `DELETE` from HTMX (`hx-delete`). A form without HTMX sends `POST` with `_method=DELETE`; a small middleware rewrites it.
- **Per-field autosave.** Record pages have no Save button (operator decision 2026-11-21: the FileMaker model). Each editable field sends `hx-patch` with `hx-trigger="change"` when the user leaves it, with that one field only. Contract: section "Per-field autosave (PATCH)".
- **Responses to writes.** A write from a full page answers `303 See Other` to the page. A write from HTMX answers the changed fragment, plus out-of-band swaps (`hx-swap-oob`) for figures that depend on it (for example registration totals after an item change). A create answers `HX-Redirect` to the new record.
- **Input.** axum `Form` into a typed struct (serde, `deny_unknown_fields`). Money arrives as dollar text and is parsed to integer cents (04 convention). Dates arrive as ISO `YYYY-MM-DD`. A bad value re-renders the form with a field error (422).
- **Offering kind.** School routes take `{kind}` = `course` | `seminar` | `short-course`. The handler maps the kind to the three table families of `04` (`courses`/`calendar_course`/`enrollments_course`, `seminars`/`calendar_seminar`/`enrollments_seminars`, `short_courses`/`calendar_short_course`/`enrollments_short_course`). An unknown kind is 404.
- **Lists.** `?q=` search text, `?page=` (50 rows per page), `?view=list|table` (the FileMaker list and table layouts become one list page with two views; 06).
- **Static files.** `GET /assets/{path}` serves the CSS, the vendored `htmx.min.js` and `app.js`, embedded in the binary.
- **Print.** Print pages are HTML with a print stylesheet. The browser prints or saves as PDF (product decision, `decisions.md`). No server PDF engine.

## Route catalog

Kind: `page` (full page, or fragment with `HX-Request`), `partial` (HTMX fragment only), `action` (write), `file` (download), `print` (print-CSS page), `static`. Every route except `GET /login`, `POST /login` and `/assets/*` needs a valid session (AP-01). Every `POST`, `PATCH` and `DELETE` needs the CSRF token (AP-02).

### AUTH and SYS

| Method | Path | Kind | Operation | Screen | Policy | Notes |
|---|---|---|---|---|---|---|
| GET | `/login` | page | none | UI-01 | none (public) | Sign-in form. |
| POST | `/login` | action | OP-AUTH-01 | UI-01 | AP-11 | Generic error on any failure (BR-AUTH-01). Sets the session cookie and redirects to `/`. |
| POST | `/logout` | action | OP-AUTH-02 | UI-02 | AP-01, AP-02 | Deletes the `sessions` row. |
| GET | `/` | page | query `home` | UI-02 | AP-01 | Module tiles. Convention opens on events, school on the student list (BR-SYS-01). |
| GET | `/account/password` | page | none | UI-33 | AP-01 | |
| POST | `/account/password` | action | OP-AUTH-03 | UI-33 | AP-12 | Current password required. |
| GET | `/assets/{path}` | static | none | all | none (public) | CSS, htmx, app.js. |
| GET | `/files/{uuid}` | file | OP-SYS-04 | UI-09, UI-26 | AP-03 | Streams a `stored_files` row with its stored MIME type; `Content-Disposition: attachment` except PDF and images. |
| POST | `/files` | action | OP-SYS-04 | UI-09, UI-26, UI-30 | AP-03 | Multipart upload; returns a file-chip partial with the new `stored_files.uuid` (BR-SYS-04). |

### Admin (ROLE-ADMIN)

| Method | Path | Kind | Operation | Screen | Policy | Notes |
|---|---|---|---|---|---|---|
| GET | `/admin/users` | page | query `user_list` | UI-29 | AP-08 | |
| POST | `/admin/users` | action | OP-AUTH-04 | UI-29 | AP-08 | Create user with role and first password. |
| GET | `/admin/users/{id}` | partial | query `user_list` | UI-29 | AP-08 | Edit row. |
| PATCH | `/admin/users/{id}` | action | OP-AUTH-04 | UI-29 | AP-08, AP-11 | Role, active, display name. Disable or role change deletes the user's sessions; last active admin is protected (AP-11). Per-field autosave: one field per request, returns the field fragment (supersedes the whole-record `POST`). |
| POST | `/admin/users/{id}/password` | action | OP-AUTH-04 | UI-29 | AP-08 | Set a new password. |
| GET | `/admin/settings` | page | query `settings_get` | UI-30 | AP-08 | `app_settings` keys; the logo upload. |
| PATCH | `/admin/settings` | action | OP-SYS-01 | UI-30 | AP-08 | BR-SYS-03. Per-field autosave: one field per request, returns the field fragment (supersedes the whole-record `POST`). |
| GET | `/admin/templates/{code}` | page | query `document_template_get` | UI-30 | AP-08 | Letter, certificate and policy texts (D-18). |
| PATCH | `/admin/templates/{code}` | action | OP-SYS-02 | UI-30 | AP-08 | Sanitized HTML (allow-list). Per-field autosave: one field per request, returns the field fragment (supersedes the whole-record `POST`). |
| GET | `/admin/value-lists` | page | query `value_list_get` | UI-30 | AP-08 | `value_list_items` grouped by list. |
| POST | `/admin/value-lists` | action | OP-SYS-01 | UI-30 | AP-08 | Add, rename or reorder an item. A value that a CHECK constraint names cannot be removed (BR list in 07 "Validation and reference values"). |
| GET | `/admin/certifications` | page | query `certification_list` | UI-32 | AP-08 | |
| POST | `/admin/certifications` | action | OP-CRT-01 | UI-32 | AP-08 | |
| GET | `/admin/certifications/{id}` | page | query `certification_get` | UI-32 | AP-08 | Requirements portal (BR-CRT-01). |
| PATCH | `/admin/certifications/{id}` | action | OP-CRT-01 | UI-32 | AP-08 | Per-field autosave: one field per request, returns the field fragment (supersedes the whole-record `POST`). |
| POST | `/admin/certifications/{id}/requirements` | action | OP-CRT-01 | UI-32 | AP-08 | Add a required course or elective. |
| DELETE | `/admin/certifications/{id}/requirements/{req_id}` | action | OP-CRT-01 | UI-32 | AP-08 | |
| GET | `/admin/professors` | page | query `professor_list` | UI-32 | AP-08 | |
| POST | `/admin/professors` | action | OP-CRS-04 | UI-32 | AP-08 | |
| PATCH | `/admin/professors/{id}` | action | OP-CRS-04 | UI-32 | AP-08 | Per-field autosave: one field per request, returns the field fragment (supersedes the whole-record `POST`). |
| GET | `/admin/mail/templates` | page | query `mail_template_list` | UI-31 | AP-08 | |
| POST | `/admin/mail/templates` | action | OP-MAIL-06 | UI-31 | AP-08 | |
| GET | `/admin/mail/templates/{id}` | page | query `mail_template_get` | UI-31 | AP-08 | |
| PATCH | `/admin/mail/templates/{id}` | action | OP-MAIL-06 | UI-31 | AP-08 | Per-field autosave: one field per request, returns the field fragment (supersedes the whole-record `POST`). |
| GET | `/admin/mail/templates/{id}/preview` | page | OP-MAIL-06 | UI-27 | AP-08 | Was layout `Template Preview`. |
| GET | `/admin/mail/profiles` | page | query `mail_profile_list` | UI-31 | AP-08 | |
| POST | `/admin/mail/profiles` | action | OP-MAIL-07 | UI-31 | AP-08 | Duplicate e-mail refused (BR-MAIL-09). |
| PATCH | `/admin/mail/profiles/{email}` | action | OP-MAIL-07 | UI-31 | AP-08 | Name, title, default flag (one default). Per-field autosave: one field per request, returns the field fragment (supersedes the whole-record `POST`). |
### PPL and REG (convention)

| Method | Path | Kind | Operation | Screen | Policy | Notes |
|---|---|---|---|---|---|---|
| GET | `/people` | page | query `people_search` | UI-03 | AP-03 | `?q=`, `?view=list|table`. |
| GET | `/people/new` | page | none | UI-04 | AP-03 | |
| POST | `/people` | action | OP-PPL-01 | UI-04 | AP-03 | |
| GET | `/people/{id}` | page | query `person_get` | UI-04 | AP-03 | Person with registrations portal. |
| PATCH | `/people/{id}` | action | OP-PPL-01 | UI-04 | AP-03 | Per-field autosave: one field per request, returns the field fragment (supersedes the whole-record `POST`). |
| DELETE | `/people/{id}` | action | OP-PPL-02 | UI-04 | AP-04 | Refused when the person has registrations (BR-PPL-02). |
| POST | `/people/{id}/registrations` | action | OP-REG-01 | UI-04 | AP-03 | "Añadir Registro": creates the registration, `HX-Redirect` to UI-06. |
| GET | `/registrations` | page | query `registration_list` | UI-05 | AP-03 | `?event=`, `?status=`, `?q=`, `?view=`. |
| GET | `/registrations/{id}` | page | query `registration_get` | UI-06 | AP-03 | Header, items, guests, payments, totals from `v_registration_totals`. |
| PATCH | `/registrations/{id}` | action | OP-REG-01 | UI-06 | AP-03 | Header fields: status, `costeado_por`, comments, guest badge text. Per-field autosave: one field per request, returns the field fragment (supersedes the whole-record `POST`). |
| GET | `/registrations/{id}/event-picker` | partial | OP-REG-02 | UI-06 | AP-03 | Dialog of events with status Activo (was layout `Eventos Activos`). Not offered when the registration already has an event. Empty: "Al momento no hay eventos activos." (BR-REG-01, audit fix). |
| POST | `/registrations/{id}/event` | action | OP-REG-02 | UI-06 | AP-03 | Writes only this registration (changed, AMB-07-01). |
| GET | `/registrations/{id}/plan-picker` | partial | OP-REG-03 | UI-06 | AP-03 | Dialog of the event's plans (was layout `Planes Activos`). |
| POST | `/registrations/{id}/plan` | action | OP-REG-03 | UI-06 | AP-03, AP-09 | Two-step confirm when a plan is already set (BR-REG-02). One transaction. |
| POST | `/registrations/{id}/items` | action | OP-REG-04 | UI-06 | AP-06 | Add one event item of the same event. Returns items portal + totals (OOB). |
| DELETE | `/registrations/{id}/items/{item_id}` | action | OP-REG-04 | UI-06 | AP-06 | Two-step confirm. |
| GET | `/registrations/{id}/items/{item_id}/attributes` | partial | OP-REG-05 | UI-06 | AP-03 | Dialog by item type: hotel (reservation, extra nights) or seating (table) (BR-REG-06). |
| PATCH | `/registrations/{id}/items/{item_id}` | action | OP-REG-05 | UI-06 | AP-03 | Line fields: price, qty, no charge, attributes (hotel reservation and nights, table). A qty patch also sets `qty_manual` = 1 (BR-REG-03, D-88). Returns the row and the totals block out of band (`hx-swap-oob`, `id="totales"`). Per-field autosave: one field per request, returns the field fragment (supersedes the whole-record `POST`). |
| POST | `/registrations/{id}/guests` | action | OP-REG-06 | UI-06 | AP-06 | Add guest; recompute `ticket_qty` of items with `qty_manual` = 0 (BR-REG-03, D-88) in the same transaction. |
| PATCH | `/registrations/{id}/guests/{guest_id}` | action | OP-REG-06 | UI-06 | AP-03 | Badge edit, `hx-trigger="change"` (replaces OnObjectExit, layout 29 key 56); recompute `ticket_qty` of items with `qty_manual` = 0 (D-88). Per-field autosave: one field per request, returns the field fragment (supersedes the whole-record `POST`). |
| DELETE | `/registrations/{id}/guests/{guest_id}` | action | OP-REG-06 | UI-06 | AP-06 | Recompute `ticket_qty` of items with `qty_manual` = 0 (D-88). |
| POST | `/registrations/{id}/payments` | action | OP-REG-07 | UI-06 | AP-03 | Amount > 0. |
| POST | `/payments/{id}/void` | action | OP-REG-08 | UI-06 | AP-05 | ROLE-ADMIN; reason required (BR-REG-09). |
| POST | `/registrations/{id}/cancel` | action | OP-REG-09 | UI-06 | AP-05 | Status Canceló. |
| DELETE | `/registrations/{id}` | action | OP-REG-09 | UI-04, UI-06 | AP-05, AP-06 | Refused when payments exist; offers cancel. |
| GET | `/events/{id}/door` | page | query `door_list` | UI-07 | AP-03 | Door check-in list of one event. |
| POST | `/registrations/{id}/attendance` | action | OP-REG-10 | UI-07 | AP-03 | Asistió / No Asistió / Canceló; returns the row. |

### EVT and SPN (convention)

| Method | Path | Kind | Operation | Screen | Policy | Notes |
|---|---|---|---|---|---|---|
| GET | `/events` | page | query `event_list` | UI-08 | AP-03 | |
| GET | `/events/new` | page | none | UI-09 | AP-03 | |
| POST | `/events` | action | OP-EVT-01 | UI-09 | AP-03 | Status Activo, end date defaults to start (BR-EVT-01, BR-EVT-02). |
| GET | `/events/{id}` | page | query `event_get` | UI-09 | AP-03 | `?tab=items|sponsors|billing`; totals from `v_event_totals`. |
| PATCH | `/events/{id}` | action | OP-EVT-02 | UI-09 | AP-03 | Header field; event totals return out of band. Per-field autosave: one field per request, returns the field fragment (supersedes the whole-record `POST`). |
| DELETE | `/events/{id}` | action | OP-EVT-01 | UI-09 | AP-04 | Refused when the event has registrations. |
| POST | `/events/{id}/pdf` | action | OP-EVT-01 | UI-09 | AP-03 | Attach the event PDF (`events.pdf_file_uuid`). |
| DELETE | `/events/{id}/pdf` | action | OP-EVT-01 | UI-09 | AP-03 | |
| POST | `/events/{id}/items` | action | OP-EVT-03 | UI-09 | AP-03 | "Añadir Evento"; `sort_order` = count + 1 (BR-EVT-03). |
| GET | `/event-items/{id}` | page | query `event_item_get` | UI-10 | AP-03 | With the registration items that use it. |
| PATCH | `/event-items/{id}` | action | OP-EVT-03 | UI-09, UI-10 | AP-03 | Per-field autosave: one field per request, returns the field fragment (supersedes the whole-record `POST`). |
| DELETE | `/event-items/{id}` | action | OP-EVT-03 | UI-09 | AP-06 | FK refusal shows a message. |
| POST | `/events/{id}/sponsors` | action | OP-SPN-01 | UI-09 | AP-03 | |
| GET | `/sponsors/{id}` | page | query `sponsor_get` | UI-11 | AP-03 | |
| PATCH | `/sponsors/{id}` | action | OP-SPN-01 | UI-11 | AP-03 | Per-field autosave: one field per request, returns the field fragment (supersedes the whole-record `POST`). |
| DELETE | `/sponsors/{id}` | action | OP-SPN-01 | UI-09 | AP-06 | |
| POST | `/events/{id}/billing` | action | OP-SPN-02 | UI-09 | AP-03 | Upload a billing file (D-16). |
| DELETE | `/billing/{id}` | action | OP-SPN-02 | UI-09 | AP-06 | |
| GET | `/plans` | page | query `plan_list` | UI-12 | AP-03 | `?event=`. |
| POST | `/plans` | action | OP-EVT-05 | UI-12 | AP-03 | |
| GET | `/plans/{id}` | page | query `plan_get` | UI-13 | AP-03 | Price from `v_package_price` (BR-EVT-05). |
| PATCH | `/plans/{id}` | action | OP-EVT-05 | UI-13 | AP-03 | Name, price override, event. When the event changes, the handler runs OP-EVT-04 in the same transaction (was the OnObjectSave trigger, layout 40 key 21). Per-field autosave: one field per request, returns the field fragment (supersedes the whole-record `POST`). |
| POST | `/plans/{id}/build` | action | OP-EVT-04 | UI-13 | AP-03 | Adds missing plan items only (BR-EVT-06). |
| PATCH | `/plan-items/{id}` | action | OP-EVT-05 | UI-13 | AP-03 | Active flag; returns the row and the plan price (OOB). Active flag and qty; returns the row and the plan price out of band. Per-field autosave: one field per request, returns the field fragment (supersedes the whole-record `POST`). |
| DELETE | `/plan-items/{id}` | action | OP-EVT-05 | UI-13 | AP-06 | |
| DELETE | `/plans/{id}` | action | OP-EVT-05 | UI-12, UI-13 | AP-04 | ROLE-ADMIN; refused when a registration uses the plan. |

### STU, AFF and CRT (school)

| Method | Path | Kind | Operation | Screen | Policy | Notes |
|---|---|---|---|---|---|---|
| GET | `/students` | page | OP-STU-05 | UI-14 | AP-03 | Quick find: 0 hits shows a message, 1 hit redirects to the student, more shows the list. |
| GET | `/students/new` | page | none | UI-15 | AP-03 | |
| POST | `/students` | action | OP-STU-01 | UI-15 | AP-03 | Student + one `student_certifications` row per certification (BR-STU-01). |
| GET | `/students/{id}` | page | query `student_get` | UI-15 | AP-03 | `?tab=certifications|courses|seminars|short-courses|details`. |
| PATCH | `/students/{id}` | action | OP-STU-03 | UI-15 | AP-03 | Affiliate by key (BR-STU-02). Per-field autosave: one field per request, returns the field fragment (supersedes the whole-record `POST`). |
| POST | `/students/{id}/duplicate` | action | OP-STU-02 | UI-15 | AP-03 | |
| DELETE | `/students/{id}` | action | OP-STU-04 | UI-15 | AP-04 | Refused when enrollments exist (BR-STU-07). |
| POST | `/students/{id}/interests` | action | OP-STU-03 | UI-15 | AP-06 | Add or remove a certification of interest, up to 6 (BR-STU-05). |
| PATCH | `/student-certifications/{id}` | action | OP-CRT-02 | UI-15 | AP-03 | Graduation date and note (BR-CRT-04). Per-field autosave: one field per request, returns the field fragment (supersedes the whole-record `POST`). |
| GET | `/affiliates/options` | partial | query `affiliate_options` | UI-15 | AP-03 | Typeahead for the student form (`?q=`). |
| GET | `/affiliates` | page | query `affiliate_list` | UI-16 | AP-03 | |
| GET | `/affiliates/new` | page | none | UI-17 | AP-03 | |
| POST | `/affiliates` | action | OP-AFF-01 | UI-17 | AP-03 | |
| GET | `/affiliates/{id}` | page | query `affiliate_get` | UI-17 | AP-03 | With its students. |
| PATCH | `/affiliates/{id}` | action | OP-AFF-01 | UI-17 | AP-03 | Per-field autosave: one field per request, returns the field fragment (supersedes the whole-record `POST`). |
| DELETE | `/affiliates/{id}` | action | OP-AFF-01 | UI-17 | AP-04 | Unlinks students (BR-AFF-02). |

### CRS and ENR (school)

| Method | Path | Kind | Operation | Screen | Policy | Notes |
|---|---|---|---|---|---|---|
| GET | `/offerings/{kind}` | page | query `offering_list` | UI-18 | AP-03 | |
| POST | `/offerings/{kind}` | action | OP-CRS-01 | UI-18 | AP-03 | |
| GET | `/offerings/{kind}/{id}` | page | query `offering_get` | UI-18 | AP-03 | Calendar tab with summary counts from `v_calendar_*_summary` (BR-CRS-03). |
| PATCH | `/offerings/{kind}/{id}` | action | OP-CRS-01 | UI-18 | AP-03 | Per-field autosave: one field per request, returns the field fragment (supersedes the whole-record `POST`). |
| DELETE | `/offerings/{kind}/{id}` | action | OP-CRS-01 | UI-18 | AP-04 | Refused when calendar rows or enrollments exist. |
| POST | `/offerings/{kind}/{id}/calendar` | action | OP-CRS-02 | UI-18 | AP-03 | "Abrir Curso": new row Pendiente with copied prices (BR-CRS-02). |
| GET | `/calendar/{kind}/{id}` | partial | query `calendar_get` | UI-19 | AP-03 | Dialog: status, prices, professor, class dates. |
| PATCH | `/calendar/{kind}/{id}` | action | OP-CRS-02 | UI-19 | AP-03, AP-07 | No delete route exists; status Cancelado instead (BR-CRS-05). Per-field autosave: one field per request, returns the field fragment (supersedes the whole-record `POST`). |
| POST | `/calendar/{kind}/{id}/dates` | action | OP-CRS-03 | UI-19 | AP-03 | Course: `calendar_course_dates`; short course: `calendar_short_course_dates` (max 15). |
| DELETE | `/calendar/{kind}/{id}/dates/{date_id}` | action | OP-CRS-03 | UI-19 | AP-06 | |
| GET | `/calendar/{kind}/{id}/enrollments` | page | OP-CRS-05 | UI-21 | AP-03 | Empty: message. |
| POST | `/calendar/{kind}/{id}/mail` | action | OP-MAIL-01 | UI-18 | AP-03 | Creates a batch for the enrolled students (BR-MAIL-07); `HX-Redirect` to UI-26. No enrollments: message, no batch. |
| GET | `/students/{id}/enrollments/{kind}/new` | partial | none | UI-20 | AP-03 | Enrollment dialog. |
| GET | `/calendar-options/{kind}` | partial | query `calendar_options` | UI-20 | AP-03 | `?offering=&student=`: active calendar rows only (BR-CRS-01) with the price this student pays (BR-ENR-01). |
| POST | `/students/{id}/enrollments/{kind}` | action | OP-ENR-01, OP-ENR-02, OP-ENR-03 | UI-20 | AP-03 | One op per kind; price stored at pick time. |
| GET | `/enrollments/{kind}/{id}` | page | query `enrollment_get` | UI-20 | AP-03 | Opens as a dialog from UI-15, as a page from a link. |
| POST | `/enrollments/{kind}/{id}` | action | OP-ENR-04, OP-ENR-08 | UI-20 | AP-03 | Payment, payer, notes; grades on course enrollments (BR-ENR-03). | The UI-20 dialog keeps this `POST` with Guardar: an enrollment is a settle form (payment, grades) validated as a whole.
| PATCH | `/enrollments/{kind}/{id}` | action | OP-ENR-04, OP-ENR-08 | UI-15, UI-21 | AP-03 | Line fields edited in a list (grades, payment date, notes). Per-field autosave: one field per request, returns the field fragment. |
| POST | `/enrollments/{kind}/{id}/cancel` | action | OP-ENR-05 | UI-20 | AP-05 | Status BAJA or Canceló. |
| DELETE | `/enrollments/{kind}/{id}` | action | OP-ENR-05 | UI-15, UI-20 | AP-05 | Two-step confirm; refused when a payment exists (BR-ENR-08). |
| GET | `/enrollments/{kind}` | page | query `enrollment_list` | UI-21 | AP-03 | `?calendar=`, `?status=`, `?q=`. |
| GET | `/exports/grades.xlsx` | file | OP-ENR-06 | UI-21 | AP-10 | Same filters as the list (BR-ENR-07). |
| GET | `/grades/import` | page | none | UI-22 | AP-09 | |
| POST | `/grades/import` | action | OP-ENR-07 | UI-22 | AP-09 | Upload; parse; preview stored under a token; nothing written. |
| POST | `/grades/import/{token}/apply` | action | OP-ENR-07 | UI-22 | AP-09 | Applies matched serials in one transaction; lists unmatched rows (BR-ENR-06). |

### RPT (reports, print, export)

| Method | Path | Kind | Operation | Screen | Policy | Notes |
|---|---|---|---|---|---|---|
| GET | `/reports` | page | none | UI-23 | AP-10 | Report launcher with a criteria form (D-33: criteria live in the query string, not in global fields). |
| GET | `/reports/{code}` | page | OP-RPT-03 | UI-23 | AP-10 | 8 school admin reports (18 source scripts; 10 retired, D-17, D-22); codes in `05_api_design_appendix.md` §2. HTML with print CSS. |
| GET | `/dashboard/enrollments` | page | OP-RPT-05 | UI-24 | AP-10 | Live counts and revenue (D-17). |
| GET | `/print/convention/{doc}` | print | OP-RPT-01 | UI-34 | AP-10 | 25 convention print pages (appendix §1). `?event=`, `?registration=`, `?event_item=`, `?sponsor=`, `?qty=`, `?hotel_only=1` (OD-07-01). |
| GET | `/print/school/{doc}` | print | OP-RPT-02 | UI-35 | AP-10 | 47 school print pages (appendix §1). `?student=`, `?calendar=`, `?student_certification=`, `?affiliate=`, `?qty=`. |
| GET | `/exports/accounting.csv` | file | OP-RPT-04 | UI-09 | AP-10 | `?event=&items=&plan_items=`; one row per registration, one column per picked event item, plan items in or out, + total row (BR-RPT-12, audit fix). Replaces layout `virtual_table`. |
| GET | `/exports/csbs.csv` | file | OP-RPT-04 | UI-21 | AP-10 | Short-course CE export (BR-RPT-06); constants from `app_settings`. |

### MAIL

| Method | Path | Kind | Operation | Screen | Policy | Notes |
|---|---|---|---|---|---|---|
| GET | `/mail/batches` | page | query `batch_list` | UI-25 | AP-03 | Counts from `v_mail_batch_counts`. |
| GET | `/mail/batches/{id}` | page | query `batch_get` | UI-26 | AP-03 | Batch header + one row per e-mail with a status badge (BR-MAIL-05). |
| PATCH | `/mail/batches/{id}` | action | OP-MAIL-02 | UI-26 | AP-03 | Title, message, sender profile. Only while no e-mail of the batch is sent (AP-06). Per-field autosave: one field per request, returns the field fragment (supersedes the whole-record `POST`). |
| POST | `/mail/batches/{id}/attachment` | action | OP-MAIL-02 | UI-26 | AP-03 | |
| DELETE | `/mail/batches/{id}/attachment` | action | OP-MAIL-02 | UI-26 | AP-03 | |
| DELETE | `/mail/emails/{id}` | action | OP-MAIL-02 | UI-26 | AP-06 | Only while `sent = 0`. |
| DELETE | `/mail/batches/{id}` | action | OP-MAIL-02 | UI-25 | AP-06 | Only while no e-mail is sent. |
| POST | `/mail/emails/{id}/send` | action | OP-MAIL-03 | UI-26 | AP-03 | Through EXT-01; returns the row with its new status. |
| POST | `/mail/batches/{id}/send-all` | action | OP-MAIL-04 | UI-26 | AP-09 | Two-step confirm, then starts JOB-01; returns the progress partial. |
| GET | `/mail/batches/{id}/progress` | partial | query `batch_get` | UI-26 | AP-03 | `hx-trigger="every 1s"` while JOB-01 runs; stops when done. |
| GET | `/mail/emails/{id}/preview` | page | OP-MAIL-05 | UI-27 | AP-03 | Rendered body (BR-MAIL-03) in a sandboxed `iframe srcdoc`. |
| GET | `/mail/outbox` | page | OP-MAIL-08 | UI-28 | AP-13 | Mock and real sends; provider column. |
| GET | `/mail/outbox/{id}` | page | OP-MAIL-08 | UI-28 | AP-13 | One message: headers, HTML in a sandboxed iframe, attachment link. |

Route count: 150 (9 AUTH/SYS, 28 admin, 27 PPL/REG, 26 EVT/SPN, 16 STU/AFF/CRT, 24 CRS/ENR, 7 RPT, 13 MAIL). By kind: 47 full pages, 9 HTMX-only partials, 87 actions, 4 file downloads, 2 print families, 1 static. Every page route also answers an HTMX request with its fragment. The two `/print/...` routes serve 72 print pages; each is listed in `05_api_design_appendix.md` §1.

## Use-case operation catalog

One row per operation of `07`. The route column names the route that calls it; the transaction column is the contract the handler keeps.

| Domain/use case | Operation | Route | Input | Output (HTML) | Object files | Transaction/effects | Policy | Errors |
|---|---|---|---|---|---|---|---|---|
| REG: create registration from person | OP-REG-01 | `POST /people/{id}/registrations`; header edit `PATCH /registrations/{id}` (per field) | person id; header fields | redirect to UI-06 | `objects/MBA.fmp12/scripts/21-people_create_registration.txt`, `objects/MBA.fmp12/scripts/33-registration_newRecord.txt` | one insert; next `registration_number` in the same transaction | AP-03 | NotFound, Validation |
| REG: select event | OP-REG-02 | `POST /registrations/{id}/event` | event id | header fragment | `objects/MBA.fmp12/scripts/42-registrations.selectEvent-select.txt` | single update of this registration | AP-03 | NoActiveEvent, NotFound |
| REG: select plan | OP-REG-03 | `POST /registrations/{id}/plan` | plan id, `confirm` | items portal + totals (OOB) | `objects/MBA.fmp12/scripts/45-registrations.selectPackage-select.txt` | one transaction: delete this registration's items, insert active plan items, update registration | AP-03, AP-09 | ConfirmRequired, PlanNotOfEvent |
| REG: add/remove item | OP-REG-04 | `POST /registrations/{id}/items`, `DELETE .../items/{item_id}` | event item id | items portal + totals | `objects/MBA.fmp12/layouts/29-Registro.txt` | single insert/delete | AP-06 | ItemNotOfEvent, ConfirmRequired |
| REG: item attributes | OP-REG-05 | `POST .../items/{item_id}/attributes` | reservation, nights or table | item row | `objects/MBA.fmp12/scripts/37-registration_items_edit_attributes.txt` | single update | AP-03 | Validation |
| REG: guests | OP-REG-06 | `/registrations/{id}/guests...` | name badge | guests + items portals | `objects/MBA.fmp12/scripts/38-registration_update_guests.txt`, `objects/MBA.fmp12/scripts/39-registration_delete_guest.txt` | one transaction: guest change + `ticket_qty` of items with `qty_manual` = 0 (D-88) | AP-03, AP-06 | Validation |
| REG: record payment | OP-REG-07 | `POST /registrations/{id}/payments` | amount, type, number | payments portal + totals | `objects/MBA.fmp12/scripts/35-registration_add_payment.txt` | single insert | AP-03 | Validation (amount > 0) |
| REG: void payment | OP-REG-08 | `POST /payments/{id}/void` | reason | payments portal + totals | product decision (D-09) | update + `audit_log` in one transaction | AP-05 | Forbidden, ReasonRequired |
| REG: cancel/delete registration | OP-REG-09 | `POST /registrations/{id}/cancel`, `DELETE /registrations/{id}` | `confirm` | redirect | `objects/MBA.fmp12/relationships/4-registration_payments.txt` | update, or delete with cascade of items and guests | AP-05, AP-06 | HasPayments |
| REG: door check-in | OP-REG-10 | `POST /registrations/{id}/attendance` | status | row | `objects/MBA.fmp12/layouts/55-Registro_Puerta.txt` | single update | AP-03 | Validation |
| EVT: event CRUD and PDF | OP-EVT-01 | `/events`, `/events/{id}`, `/events/{id}/pdf` | event fields, file | page | `objects/MBA.fmp12/scripts/26-event_newRecord.txt`, `objects/MBA.fmp12/scripts/13-event.deletePDF.txt` | single statement | AP-03, AP-04 | HasRegistrations |
| EVT: save event | OP-EVT-02 | `PATCH /events/{id}` | one event field | field + totals (OOB) | `objects/MBA.fmp12/scripts/15-event.save.txt` | single update | AP-03 | Validation |
| EVT: event items | OP-EVT-03 | `POST /events/{id}/items`, `PATCH /event-items/{id}` | item fields | items portal | `objects/MBA.fmp12/scripts/25-event_create_event_item.txt` | single statement | AP-03, AP-06 | InUse (FK) |
| EVT: build plan items | OP-EVT-04 | `POST /plans/{id}/build`; `PATCH /plans/{id}` on event change | plan id | plan items portal | `objects/MBA.fmp12/scripts/50-package_select_event.txt` | one transaction; insert missing pairs only | AP-03 | NoEvent, EventHasNoItems |
| EVT: plans and plan items | OP-EVT-05 | `/plans...`, `/plan-items/{id}...` | plan fields | page / row | `objects/MBA.fmp12/scripts/48-package_create_item.txt`, `objects/MBA.fmp12/scripts/49-package_active.txt` | single statement | AP-03, AP-04, AP-06 | InUse |
| SPN: sponsors | OP-SPN-01 | `/events/{id}/sponsors`, `/sponsors/{id}` | sponsor fields | sponsors tab | `objects/MBA.fmp12/layouts/45-Auspiciadores.txt` | single statement | AP-03, AP-06 | Validation |
| SPN: billing files | OP-SPN-02 | `POST /events/{id}/billing`, `DELETE /billing/{id}` | file | billing tab | `objects/MBA.fmp12/layouts/35-Convencion.txt` | insert `stored_files` + `billing` in one transaction | AP-03, AP-06 | FileTooLarge |
| PPL: create/edit person | OP-PPL-01 | `POST /people`, `PATCH /people/{id}` | person fields | page | `objects/MBA.fmp12/scripts/22-people_newRecord.txt` | single statement | AP-03 | Validation |
| PPL: delete person | OP-PPL-02 | `DELETE /people/{id}` | `confirm` | redirect | `objects/MBA.fmp12/custom-menus/36-people.records.txt` | single delete | AP-04 | HasRegistrations |
| AFF: affiliates | OP-AFF-01 | `/affiliates...` | affiliate fields | page | `objects/MBS.fmp12/layouts/48-Afiliado.txt` | delete: unlink students + delete in one transaction | AP-03, AP-04 | Validation |
| STU: create student | OP-STU-01 | `POST /students` | student fields | redirect | `objects/MBS.fmp12/scripts/33-student_new_record.txt`, `objects/MBS.fmp12/scripts/34-student_create_certification_records_student_uuid.txt` | one transaction: student + certification rows | AP-03 | Validation |
| STU: duplicate student | OP-STU-02 | `POST /students/{id}/duplicate` | none | redirect | `objects/MBS.fmp12/scripts/35-student_duplicate_record.txt` | one transaction | AP-03 | NotFound |
| STU: edit student | OP-STU-03 | `PATCH /students/{id}`, `POST /students/{id}/interests` | fields, interest | page / interests partial | `objects/MBS.fmp12/layouts/43-Estudiante.txt` | single statement | AP-03, AP-06 | MaxInterests (6) |
| STU: delete student | OP-STU-04 | `DELETE /students/{id}` | `confirm` | redirect | product decision (D-09) | delete with cascade of certification rows and interests | AP-04 | HasEnrollments |
| STU: quick find | OP-STU-05 | `GET /students?q=` | text | list, redirect or message | `objects/MBS.fmp12/scripts/45-student_quick_find.txt` | read | AP-03 | none |
| CRS: offerings | OP-CRS-01 | `/offerings/{kind}...` | offering fields | page | `objects/MBS.fmp12/layouts/57-Cursos_Especializados.txt`, `objects/MBS.fmp12/layouts/60-Seminarios.txt`, `objects/MBS.fmp12/layouts/64-Cursos_Cortos.txt` | single statement | AP-03, AP-04 | InUse |
| CRS: calendar rows | OP-CRS-02 | `POST /offerings/{kind}/{id}/calendar`, `PATCH /calendar/{kind}/{id}` | status, prices, professor | calendar portal | `objects/MBS.fmp12/scripts/52-course_add_date.txt` | single statement; prices copied at insert | AP-03, AP-07 | Validation |
| CRS: class dates | OP-CRS-03 | `/calendar/{kind}/{id}/dates...` | date, day type | dates list | `objects/MBS.fmp12/layouts/58-Fechas_de_Curso.txt`, `objects/MBS.fmp12/layouts/65-Fechas_del_Curso_Corto.txt` | single statement | AP-03, AP-06 | MaxDates (15) |
| CRS: professors | OP-CRS-04 | `/admin/professors...` | name | list | `objects/MBS.fmp12/layouts/66-Tabla_de_Profesores.txt` | single statement | AP-08 | Validation |
| CRS: enrollment list of a row | OP-CRS-05 | `GET /calendar/{kind}/{id}/enrollments` | none | page | `objects/MBS.fmp12/scripts/54-course_view_enrollments.txt` | read | AP-03 | Empty (message) |
| ENR: course enrollment | OP-ENR-01 | `POST /students/{id}/enrollments/course` | calendar row, payer | dialog closes; portal refresh | `objects/MBS.fmp12/scripts/37-student_new_course_enrollment.txt` | single insert; price read in the same transaction | AP-03 | CalendarNotActive |
| ENR: seminar enrollment | OP-ENR-02 | `POST /students/{id}/enrollments/seminar` | as above | as above | `objects/MBS.fmp12/scripts/38-student_new_seminar_enrollment.txt` | as above | AP-03 | as above |
| ENR: short-course enrollment | OP-ENR-03 | `POST /students/{id}/enrollments/short-course` | as above | as above | `objects/MBS.fmp12/scripts/39-student_new_short_course_enrollment.txt` | as above | AP-03 | as above |
| ENR: save enrollment | OP-ENR-04 | `POST /enrollments/{kind}/{id}` | payment, payer, notes | dialog | `objects/MBS.fmp12/scripts/166-gEnrollment.guardar.txt` | single update | AP-03 | Validation |
| ENR: delete/cancel | OP-ENR-05 | `DELETE /enrollments/{kind}/{id}`, `POST .../cancel` | `confirm` | portal | `objects/MBS.fmp12/scripts/165-gEnrollment.deleteRecord.txt` | single statement + `audit_log` | AP-05 | HasPayment, ConfirmRequired |
| ENR: export grades | OP-ENR-06 | `GET /exports/grades.xlsx` | list filters | XLSX | `objects/MBS.fmp12/scripts/127-enroll_course_export_registro_de_notas.txt` | read | AP-10 | none |
| ENR: import grades | OP-ENR-07 | `POST /grades/import`, `POST /grades/import/{token}/apply` | XLSX | preview, then result | `objects/MBS.fmp12/scripts/128-enroll_course_import_registro_de_notas.txt` | apply in one transaction | AP-09 | BadFile, TokenExpired |
| ENR: grades by hand | OP-ENR-08 | `POST /enrollments/course/{id}` | grade fields | dialog | `objects/MBS.fmp12/layouts/82-Matricula_de_Curso.txt` | single update | AP-03 | ScoreOutOfRange |
| CRT: certifications | OP-CRT-01 | `/admin/certifications...` | fields | page | `objects/MBS.fmp12/layouts/59-Certificaciones.txt` | single statement | AP-08 | InUse |
| CRT: graduation | OP-CRT-02 | `PATCH /student-certifications/{id}` | date, note | certifications tab | `objects/MBS.fmp12/layouts/43-Estudiante.txt` | single update | AP-03 | Validation |
| CRT: certificate | OP-CRT-03 | `GET /print/school/certificado-graduacion?student_certification=` | id | print page | `objects/MBS.fmp12/scripts/48-students_print_certification.txt` | read | AP-10 | NotGraduated |
| MAIL: batch from calendar row | OP-MAIL-01 | `POST /calendar/{kind}/{id}/mail` | none | redirect UI-26 | `objects/MBS.fmp12/scripts/173-sciSendMai.caller_module.txt`, `objects/MBS_SendMail.fmp12/scripts/1-g.createEmails_contact_ids.txt` | one transaction: batch + e-mails | AP-03 | NoEnrollments |
| MAIL: edit batch | OP-MAIL-02 | `PATCH /mail/batches/{id}`, `POST .../attachment` | title, message, profile, file | header | `objects/MBS_SendMail.fmp12/scripts/21-email.attachment.txt` | single statement | AP-03, AP-06 | AlreadySent |
| MAIL: send one | OP-MAIL-03 | `POST /mail/emails/{id}/send` | none | row | `objects/MBS_SendMail.fmp12/scripts/8-email.send.txt` | EXT-01 call, then update the row; no transaction across the call | AP-03 | ProviderError (stored, BR-MAIL-04) |
| MAIL: send all | OP-MAIL-04 | `POST /mail/batches/{id}/send-all` | `confirm` | progress partial | `objects/MBS_SendMail.fmp12/scripts/18-email.buttonAll.txt` | JOB-01, one e-mail at a time | AP-09 | ConfirmRequired, AlreadyRunning |
| MAIL: preview | OP-MAIL-05 | `GET /mail/emails/{id}/preview` | none | page | `objects/MBS_SendMail.fmp12/scripts/7-emails.previewTemplate.txt` | read | AP-03 | none |
| MAIL: templates | OP-MAIL-06 | `/admin/mail/templates...` | fields | page | `objects/MBS_SendMail.fmp12/tables/129-templates.txt` | single statement | AP-08 | Validation |
| MAIL: sender profiles | OP-MAIL-07 | `/admin/mail/profiles...` | fields | page | `objects/MBS_SendMail.fmp12/layouts/12-profiles.txt` | single statement; one default | AP-08 | DuplicateEmail |
| MAIL: outbox | OP-MAIL-08 | `GET /mail/outbox...` | none | page | product decision (D-05) | read | AP-13 | none |
| RPT: convention print | OP-RPT-01 | `GET /print/convention/{doc}` | scope params | print page | appendix §1 | read | AP-10 | NoRecords (message page) |
| RPT: school print | OP-RPT-02 | `GET /print/school/{doc}` | scope params | print page | appendix §1 | read | AP-10 | NoRecords |
| RPT: admin reports | OP-RPT-03 | `GET /reports/{code}` | criteria | report page | appendix §2 | read | AP-10 | Validation (criteria) |
| RPT: exports | OP-RPT-04 | `GET /exports/accounting.csv`, `GET /exports/csbs.csv` | filters | CSV | `objects/MBA.fmp12/scripts/95-registrations.createAccountingReport_lastTimestamp.txt` | read | AP-10 | none |
| RPT: dashboard | OP-RPT-05 | `GET /dashboard/enrollments` | year range | page | `objects/MBS.fmp12/layouts/139-Resumen_de_Cursos.txt`, `objects/MBS.fmp12/layouts/141-Ingresos_de_Cursos.txt` | read | AP-10 | none |
| AUTH: sign in | OP-AUTH-01 | `POST /login` | username, password | redirect | product decision (D-50) | session insert, counters update | AP-01, AP-11 | InvalidCredentials (generic) |
| AUTH: sign out | OP-AUTH-02 | `POST /logout` | none | redirect | `objects/MBA.fmp12/scripts/2-Re-Login.txt` | session delete | AP-01 | none |
| AUTH: change own password | OP-AUTH-03 | `POST /account/password` | current, new | page | privilege sets (`allowModifyPassword`) | single update; other sessions of the user deleted | AP-12 | WrongPassword, WeakPassword |
| AUTH: manage users | OP-AUTH-04 | `/admin/users...` | fields | page | `objects/MBA.fmp12/privilege-sets/1-Full_Access.txt` | single statement + session revoke | AP-08, AP-11 | LastAdmin |
| SYS: settings | OP-SYS-01 | `PATCH /admin/settings`, `POST /admin/value-lists` | key/values | page | product decision (D-18, D-25, D-14, D-20) | one transaction | AP-08 | Validation |
| SYS: document templates | OP-SYS-02 | `PATCH /admin/templates/{code}` | HTML | page | product decision (D-18) | single update | AP-08 | Validation |
| SYS: seed demo data | OP-SYS-03 | none (CLI `mba-seed`) | seed, size | database file | product decision (D-02) | one transaction per table group | AP-14 | none |
| SYS: files | OP-SYS-04 | `POST /files`, `GET /files/{uuid}` | multipart | file chip / bytes | containers (04 appendix) | insert `stored_files`, write file to `data/files/` | AP-03 | FileTooLarge, EmptyFile |

Coverage: 62 of 62 operations of `07` have a route or, for OP-SYS-03, a CLI entry.

## Query/read contracts

| Query | Fields/projection | Filters/sort/page | Record policy | Field restrictions | Source found-set/report semantics | Limits |
|---|---|---|---|---|---|---|
| `people_search` | name, company, phone, e-mail, registration count | `q` on name and company; sort last name | none (AP-03) | none | Quick Find on Lista de Convencionistas | 50 per page |
| `person_get` | person + registrations (event, dates, status, total from `v_registration_totals`) | by id | none | none | layout 25 portal `registration` | n/a |
| `registration_list` | number, person, company, event, status, type, total, balance | event, status, `q`; sort number desc | none | none | Lista de Registro; "Ver Registros" from an event (script 28) | 50 per page |
| `registration_get` | registration, person, items (`v_registration_item_lines`), guests, payments (voided shown struck through), totals (`v_registration_totals`) | by id | none | none | layout 29 | n/a |
| `door_list` | number, name, company, guests, status | event; sort last name | none | none | layout 55 Registro Puerta | none (one event) |
| `event_list` | name, dates, location, status, counts from `v_event_totals` | status; sort start desc | none | none | Lista de Eventos | 50 per page |
| `event_get` | event, items, sponsors, billing files, `v_event_totals` | by id, tab | none | none | layouts 35, 98, 99 | n/a |
| `event_item_get` | item + registration items that use it | by id | none | none | layout 36 portal | n/a |
| `sponsor_get` | sponsor fields | by id | none | none | layout 45 | n/a |
| `plan_list`, `plan_get` | plan, event, price (`v_package_price`), items with active flag | event | none | none | layouts 40, 41 | 50 per page |
| `student_get` | student, affiliate, certification progress (`v_student_certification_progress`), academic summary (`v_student_academic`), enrollments per kind, interests | by id, tab | none | no SSN column exists (D-06) | layout 43 | n/a |
| `affiliate_list`, `affiliate_get`, `affiliate_options` | name, type, contact; students | `q`, `cust_type` | none | none | layouts 48, 49; value list `affiliates_by_name` | 50 per page; options 20 |
| `offering_list`, `offering_get` | offering + calendar rows with summary (`v_calendar_course_summary`, `v_calendar_seminar_summary`, `v_calendar_short_course_summary`) | kind | none | none | layouts 57, 60, 64 | n/a |
| `calendar_options` | active rows of one offering with date, status and the price for the student | kind, offering, student; active only (BR-CRS-01) | none | none | value list `course_get_calendar_date` (related-only) | n/a |
| `calendar_get` | row, dates, professor | by id | none | none | layouts 58, 61, 65 | n/a |
| `enrollment_list`, `enrollment_get` | student, offering, row date, price, payment, balance, status, grades (course) | kind, calendar, status, `q` | none | none | layouts 81, 82, 102, 103, 111, 113 | 50 per page |
| `report_run` | per report (appendix §2) | criteria in query string | none | none | Reportes Administrativos (137) and its scripts | none |
| `batch_list`, `batch_get` | batch, profile, counts (`v_mail_batch_counts`), e-mails with status | by id | none | `RESEND_API_KEY` never read into a template (AP-13) | layouts 4, 7 | 50 per page |
| `outbox_list` | created, provider, to, subject, status, error | provider, status; sort created desc | none | none | new (D-05) | 50 per page |
| `user_list` | username, display name, role, active, locked until, last sign-in | none | ROLE-ADMIN | `password_hash` never selected | new (D-50) | n/a |
| `settings_get`, `value_list_get`, `document_template_get`, `certification_list`, `certification_get`, `professor_list`, `mail_template_list`, `mail_template_get`, `mail_profile_list` | table columns | none | ROLE-ADMIN for the edit pages | none | new admin screens (D-19) | n/a |
| `home` | module tiles, counts of active events and active students | none | none | none | file triggers (BR-SYS-01) | n/a |

Every query selects only the columns its template shows. No route returns a whole table.

## Mutation/command contracts

| Command | Preconditions | Allowed fields | Persistent invariant owner | Atomic writes | Audit | Forbidden cases/tests |
|---|---|---|---|---|---|---|
| OP-REG-03 select plan | registration has an event; plan belongs to that event; `confirm=1` when a plan is set | `package_uuid` | BR-REG-02 | delete items of this registration only, insert active plan items, update registration | `registration.plan_changed` | other registrations untouched (T-07-01); no confirm -> confirm dialog, nothing written (T-05-03) |
| OP-REG-06 guests | registration exists | guest `name` | BR-REG-03 | guest write + `ticket_qty` of items with `qty_manual` = 0 (D-88) | none | T-07-02, T-05-10 |
| OP-REG-07 payment | amount > 0 | `payment`, `payment_type`, `payment_number` | BR-REG-08 | single insert | none | amount 0 refused |
| OP-REG-08 void payment | ROLE-ADMIN; reason not empty; not voided yet | `void_reason` | BR-REG-09 | update + `audit_log` | `payment.void` | staff -> 403 (T-07-04) |
| OP-REG-09 delete registration | no payments | none | BR-REG-09 | delete with cascade | `registration.delete` | has payment -> refused (T-05-04) |
| OP-EVT-04 build plan items | plan has an event with items | none | BR-EVT-06 | insert missing pairs only (UNIQUE) | none | second run adds nothing (T-07-05) |
| OP-STU-01/02 student | none | student fields | BR-STU-01 | student + certification rows | none | T-07-07 |
| OP-STU-04 delete student | ROLE-ADMIN; no enrollments | none | BR-STU-07 | delete with cascade | `student.delete` | staff -> 403 |
| OP-ENR-01..03 enroll | calendar row active (BR-CRS-01) | calendar row, payer, notes | BR-ENR-01 | single insert; price read in the same transaction | none | inactive row refused (T-07-10) |
| OP-ENR-05 delete enrollment | `confirm=1`; no payment | none | BR-ENR-08 | single delete | `enrollment.delete` | T-07-13 |
| OP-ENR-07 import grades | preview token of this user, not older than 30 minutes | grade columns only | BR-ENR-06 | update matched serials in one transaction | `grades.import` with counts | unmatched serial never inserted (T-07-12) |
| OP-MAIL-01 batch | calendar row has enrollments | none | BR-MAIL-07, BR-MAIL-01, BR-MAIL-02 | batch + e-mails | none | 0 enrollments -> no batch (T-07-14) |
| OP-MAIL-04 send all | `confirm=1`; no JOB-01 running for this batch | none | BR-MAIL-06 | per e-mail (JOB-01) | `mail.send_batch` | sent e-mails skipped (T-07-16) |
| OP-AUTH-04 users | ROLE-ADMIN; not the last active admin | role, active, display name, password | BR-AUTH-04 | update + delete sessions | `auth.user_changed` | T-08 tests in `08` |
| OP-SYS-01 settings | ROLE-ADMIN | keys listed in `app_settings` only | BR-SYS-03 | one transaction | `settings.change` | unknown key refused |

Bulk operations: three exist (OP-REG-03, OP-MAIL-04, OP-ENR-07). Each takes its scope from the server (registration id, batch id, matched serials in the stored preview), never from a client list of ids (AP-09). No `Replace Field Contents` translation is exposed: MBS script 18 is retired (D-04).

## Per-field autosave (PATCH)

Operator decision (2026-11-21): the app keeps the FileMaker model. Record pages are always editable and have no Edit, Cancel or Save button; each field saves when the user leaves it. Guardar stays only in dialogs that create or settle a record (new student, enrollment, payment, new user, sign-in, password change, grade import apply).

| Concern | Contract |
|---|---|
| Request | `PATCH /<resource>/{id}` from `hx-patch`, `hx-trigger="change"`, body = exactly one field (`campo=valor`, form-encoded). A body with zero or several fields, or a field outside the allow list of the command, is 422. CSRF token in the `X-CSRF-Token` header (htmx `hx-headers` on `<body>`). |
| Success | 200 with the field fragment (`hx-target="closest .field-wrap"`, `hx-swap="outerHTML"`) showing the saved, normalised value. A line field (`hx-target="closest tr"`) returns the row plus every figure that depends on it out of band: registration totals (`#totales`), plan price, sponsor sums, event totals. Totals are always computed on the server (views of `04`); the browser never sends or computes them. |
| Validation error | 422 with the same field fragment in its error state: the value as typed, `aria-invalid="true"`, the Spanish message, and `data-unsaved` on the wrapper. Nothing is written. htmx `responseHandling` swaps 422. The footer counts `[data-unsaved]` ("1 campo sin guardar"). |
| Domain refusal | 409 with a message fragment, as for other commands (for example an e-mail of a sent batch, AP-06). |
| Revert | Esc in a changed field restores its saved value (`data-saved`) before the user leaves it; no request is sent (FileMaker Revert Record). |
| Save state | The fixed footer slot shows "Guardando…" (htmx `beforeRequest`), "Guardado h:mm p. m." (after a 200 swap) or "N campo(s) sin guardar". The slot has a fixed width; the page never moves. |
| Concurrency | Last write wins per field: each `PATCH` updates one column in one `BEGIN IMMEDIATE` transaction. Two users editing different fields of one record do not overwrite each other. |
| Audit | One `audit_log` row per saved field: action `field.change`, table, row id, field, old value, new value, actor, request id. Masked fields (passwords) are never patched; they use their own `POST` routes. |
| Policy | Same policy as the page (AP-03, AP-08 for admin records); the handler checks the field against the role (for example `role` and `active` on users are ROLE-ADMIN only). |

Superseded routes (replaced by the `PATCH` rows of the catalog; no whole-record save route remains):

| Superseded | Replaced by |
|---|---|
| `POST /people/{id}` | `PATCH /people/{id}` |
| `POST /registrations/{id}` (header form) | `PATCH /registrations/{id}` |
| `POST /registrations/{id}/items/{item_id}/attributes` | `PATCH /registrations/{id}/items/{item_id}` (attributes, price, qty, no charge) |
| `POST /registrations/{id}/guests/{guest_id}` | `PATCH /registrations/{id}/guests/{guest_id}` |
| `POST /events/{id}`, `POST /event-items/{id}`, `POST /sponsors/{id}` | `PATCH` on the same paths |
| `POST /plans/{id}`, `POST /plan-items/{id}/toggle` | `PATCH /plans/{id}`, `PATCH /plan-items/{id}` |
| `POST /students/{id}`, `POST /student-certifications/{id}`, `POST /affiliates/{id}` | `PATCH` on the same paths |
| `POST /offerings/{kind}/{id}`, `POST /calendar/{kind}/{id}` | `PATCH` on the same paths |
| `POST /mail/batches/{id}` | `PATCH /mail/batches/{id}` |
| `POST /admin/users/{id}`, `/admin/settings`, `/admin/templates/{code}`, `/admin/certifications/{id}`, `/admin/professors/{id}`, `/admin/mail/templates/{id}`, `/admin/mail/profiles/{email}` | `PATCH` on the same paths |

Kept as `POST`: creates, commands and settle forms (`POST /registrations/{id}/payments`, `POST /payments/{id}/void`, `POST /students/{id}/enrollments/{kind}`, `POST /enrollments/{kind}/{id}` from the UI-20 dialog, `POST /admin/users`, `POST /admin/users/{id}/password`, `POST /account/password`, `POST /login`, imports and send-all).

## Identity and account operations

| Operation | Provider/owner | Flow | Authorization | Audit/negative tests |
|---|---|---|---|---|
| Invite/reset/map user | local `users` table (D-50) | ROLE-ADMIN creates the user with a first password and tells the user out of band. No e-mail invite. No mapping of FileMaker accounts (D-07, demo). | AP-08 | `auth.user_created`; staff -> 403 |
| Activate/disable account | local | `PATCH /admin/users/{id}` with `active`. Disable deletes the user's sessions. | AP-08, AP-11 | last active admin protected |
| Assign/revoke roles | local | `PATCH /admin/users/{id}` with `role` (`admin` or `staff`). Sessions deleted on change. | AP-08, AP-11 | `auth.role_changed` |
| Provision/rotate service principal | none | No service principal. The seed tool runs from the server shell (OP-SYS-03). `RESEND_API_KEY` is an environment variable; rotation out of scope (demo). | AP-14 | none |

Local password change exists because discovery selected local accounts (D-50): OP-AUTH-03.

## Authorization matrix

| Operation/query | Policy ids (`08`) | Access channel | Deny behavior |
|---|---|---|---|
| Every route except sign-in and assets | AP-01 | browser session | page: 303 to `/login?next=`; HTMX: 401 with `HX-Redirect: /login` |
| Every `POST`/`DELETE` | AP-02 | browser session | 403, generic page or fragment |
| Domain create/view/edit (REG, EVT, SPN, PPL, AFF, STU, CRS, ENR, CRT-02, MAIL-01..05) | AP-03 | browser session | 403 |
| Parent hard delete (`DELETE` on people, students, events, plans, affiliates, offerings, certifications) | AP-04 | browser session | 403 for staff; 409 with message when dependents exist |
| Money rows (OP-REG-08, OP-REG-09, OP-ENR-05) | AP-05 | browser session | 403 or 409 with the cancel offer |
| Child rows without money | AP-06 | browser session | 409 when referenced |
| Calendar rows | AP-07 | none | no delete route (405) |
| Admin routes `/admin/*` | AP-08 | browser session | 403; links hidden for staff (UI only, secondary) |
| Bulk (OP-REG-03, OP-MAIL-04, OP-ENR-07) | AP-09 | browser session | 409 when the scope is empty or a job runs |
| Print, reports, exports (`/print/*`, `/reports*`, `/dashboard/*`, `/exports/*`) | AP-10 | browser session | 403 |
| Sign-in, user disable, role change | AP-11 | browser session | generic error; locked user sees the same message |
| Own password | AP-12 | browser session | 422 on wrong current password |
| Outbox, mail secrets | AP-13 | browser session | key never rendered |
| External channels | AP-14 | none | no route exists |

Route checks, record predicates, field projection and UI visibility are separate layers. Hiding a link never replaces the handler guard.

## External integrations

| Id | Integration | Direction | Trigger/operation | Contract | Secret reference | Retry/idempotency | Audit/reconciliation |
|---|---|---|---|---|---|---|---|
| EXT-01 | Resend e-mail adapter with outbox mock | outbound | OP-MAIL-03, OP-MAIL-04 (JOB-01) | Trait `MailProvider { fn send(&self, msg: &OutgoingMail) -> Result<ProviderReceipt, ProviderError> }`. Two implementations. `ResendProvider`: `POST https://api.resend.com/emails` with JSON `from`, `to`, `cc`, `bcc`, `subject`, `html`, `attachments` (base64), bearer key, `Idempotency-Key: <mail_emails.uuid>`. `OutboxMock`: no network; status `mocked`. The server picks `ResendProvider` when `RESEND_API_KEY` is set at start, else `OutboxMock` (D-05, BR-MAIL-08). | `RESEND_API_KEY` environment variable only; never stored, logged or rendered (AP-13). No SMTP host, user, password or licence key exists anywhere (D-05). | No automatic retry. A failed send sets `error` = 1 and puts the provider message on top of `error_log` (BR-MAIL-04); the user resends. The idempotency key stops a double send on a retry within 24 hours (inference from the Resend contract). | One `mail_outbox` row per attempt (provider `resend` or `mock`, status, `provider_message_id`, error). The outbox screen (UI-28) is the reconciliation view. |
| EXT-02 | Google Maps address link | outbound (browser) | "Show in Google Maps" on the student form | Plain link `https://www.google.com/maps/search/?api=1&query=<address>` opened in a new tab; the server builds the URL-encoded query from the student or affiliate address. No server call. | none | n/a | none |

- No `Insert from URL` step exists in the source, so no other outbound request is proven (07 "Bulk and integration safety").
- No inbound webhook. Resend delivery webhooks are out of scope (demo).
- Retired: the 360Works Email plug-in and direct SMTP (MBS_SendMail scripts 8 to 12, 19; D-05). MBS script 127 also opens a client e-mail with the grades file; the target does not rebuild that step (BR-ENR-07). The `sci_admin` file call (MBS script 30) is retired (D-04).

Mock behavior (EXT-01, no key): the adapter renders the message exactly as it would send it, inserts the `mail_outbox` row with `provider = 'mock'` and `status = 'mocked'`, and returns success. The e-mail row then shows Sent, and UI-28 shows the message. A banner on UI-26 and UI-28 says "Mock mode: no e-mail leaves the server".

## Background jobs

| Job id | Source (script, schedule, robot queue) | Trigger and frequency | Operation (OP- id) | Idempotency/retry | Run-as principal (08) | Evidence |
|---|---|---|---|---|---|---|
| JOB-01 | Send-all loop of MBS_SendMail script 18 (foreground loop in the source) | On demand: `POST /mail/batches/{id}/send-all` after confirm. One tokio task per batch; one e-mail at a time. | OP-MAIL-04 (calls OP-MAIL-03 per e-mail) | Skips e-mails with `sent = 1` (BR-MAIL-06, changed); one running job per batch (in-memory lock + 409); a server restart stops the job, and a new send-all continues with the unsent rows. | the user who confirmed (ROLE-ADMIN or ROLE-STAFF); audit actor = that user | `objects/MBS_SendMail.fmp12/scripts/18-email.buttonAll.txt` |
| JOB-02 | none (product decision, D-50) | Every 15 minutes, in process | none (housekeeping: delete `sessions` rows past `expires_at` or idle timeout; delete grade-import preview tokens older than 30 minutes) | Safe to rerun | system | product decision |

No job comes from a FileMaker schedule. The SASE scripts (`sase.storeCalcs`, MBS scripts 18, 21, 167) were schedule candidates; they are retired because live views replace the stored snapshots (D-04).

## Confirmations

A FileMaker "show dialog, then continue" step that guards a write becomes a two-step command. The first `POST` (or `DELETE`) without `confirm=1` writes nothing and returns a confirm dialog partial (status 200, `HX-Retarget: #dialog`). The dialog resends the same request with `confirm=1`.

| Command | Source dialog | Rule |
|---|---|---|
| `POST /registrations/{id}/plan` when a plan is set | "a change deletes the current items" (MBA script 44) | BR-REG-02 |
| `DELETE /registrations/{id}/items/{item_id}` | Delete Portal Row confirm (layout 29) | OP-REG-04 |
| `DELETE /enrollments/{kind}/{id}` | MBS script 165 steps 1-4 | BR-ENR-08 |
| `POST /mail/batches/{id}/send-all` | new (D-20: send-all can send many e-mails) | BR-MAIL-06 |
| Every parent `DELETE` (people, students, events, plans, affiliates, offerings) | FileMaker delete-record dialog | AP-04 |

## Errors, concurrency, and observability

| Concern | Contract |
|---|---|
| Validation/domain errors | `enum DomainError` with stable codes (`Validation{field}`, `NotFound`, `ConfirmRequired`, `HasPayments`, `HasRegistrations`, `HasEnrollments`, `InUse`, `CalendarNotActive`, `NoActiveEvent`, `NoEnrollments`, `ProviderError`, `LastAdmin`). Forms re-render with Spanish field messages (422); a field `PATCH` answers 422 with its field fragment. Domain refusals show a message fragment (409). |
| Authentication/authorization denial | No session: redirect to sign-in. Wrong role: 403 with a generic message; no hint which record exists. Sign-in errors never say which part failed (BR-AUTH-01). |
| Conflict/locking/retry | SQLite WAL, one writer; each command is one `BEGIN IMMEDIATE` transaction with `busy_timeout` 5 s. Last write wins per field: each autosave `PATCH` writes one column (operator decision 2026-11-21; was per form, D-49 default). |
| Partial external failure/compensation | Mail: the provider call is outside the transaction; the row update after it records success or error. No compensation needed. |
| Idempotency | Two-step confirms; serial numbers taken inside the insert transaction; EXT-01 idempotency key = e-mail uuid; JOB-01 skips sent rows. |
| Rate/size/pagination limits | Lists 50 rows per page. Uploads max 20 MB (`stored_files`). Grade import max 5,000 rows. Request body max 25 MB. |
| Audit actor/reason/correlation | `audit_log` rows for every saved field (`field.change`), void, delete, plan change, import, send-all, user and settings changes; `created_by`/`modified_by` on every row (BR-SYS-02). Each request gets an `x-request-id` that the log lines carry. |
| Sensitive-field logging/masking | `tracing` never logs form bodies of `/login`, `/account/password`, `/admin/users/*/password`. `RESEND_API_KEY` is read once at start and kept out of `Debug` output. |

## Traceability and tests

Each operation row above names its object files. Tests (the semantic fixtures T-07-nn in `07`, the negative tests T-08-nn in `08`, and these route tests):

| Test id | Route/contract | Case |
|---|---|---|
| T-05-01 | every route | without a session: page -> 303 to `/login`; HTMX -> 401 with `HX-Redirect` |
| T-05-02 | every `POST`/`DELETE` | without CSRF token -> 403, nothing written |
| T-05-03 | `POST /registrations/{id}/plan` | without `confirm` -> dialog, items unchanged; with `confirm` -> items replaced |
| T-05-04 | `DELETE /registrations/{id}` | with a payment -> 409, cancel offered |
| T-05-05 | `/admin/*` | ROLE-STAFF -> 403 on every admin route |
| T-05-06 | `POST /payments/{id}/void` | ROLE-STAFF -> 403; ROLE-ADMIN without reason -> 422 |
| T-05-07 | `DELETE /calendar/{kind}/{id}` | no route -> 405 |
| T-05-08 | `POST /mail/emails/{id}/send` without `RESEND_API_KEY` | `mail_outbox` row with provider `mock`; e-mail Sent; no network call |
| T-05-09 | EXT-01 with a local HTTP mock of the Resend API | request body, bearer header and `Idempotency-Key`; provider 422 -> `error` = 1, message on top of `error_log` |
| T-05-10 | `PATCH /registrations/{id}/items/{item_id}` (qty), then `POST /registrations/{id}/guests` | qty patch sets `qty_manual` = 1; the guest add changes only items with `qty_manual` = 0; the hand-set qty stays (D-88) |
| T-05-10 | `POST /mail/batches/{id}/send-all` twice | second call -> 409 while JOB-01 runs; sent e-mails skipped |
| T-05-11 | `GET /calendar-options/{kind}` | inactive rows absent; affiliate student sees the member price |
| T-05-12 | `POST /grades/import/{token}/apply` with another user's token | 404 |
| T-05-13 | `PATCH /registrations/{id}/items/{item_id}` with `qty=3` | 200: row with the new total and `#totales` out of band; one `audit_log` `field.change` row |
| T-05-14 | `PATCH /registrations/{id}/items/{item_id}` with `qty=0` | 422: field fragment with the typed value and the message; nothing written; totals unchanged |
| T-05-15 | `PATCH /people/{id}` with two fields | 422; nothing written |
| T-05-13 | all templates | no response body contains the value of `RESEND_API_KEY` or `password_hash` |
| T-05-14 | `GET /print/*`, `GET /exports/*` | the export applies the same filter as the list (AP-10) |
| T-05-15 | HTMX vs full page | the same `GET` returns a fragment with `HX-Request` and a full page without it |

## Unresolved operations

| Source behavior | Missing evidence | Risk | Required proof/owner |
|---|---|---|---|
| Rows in the Facturación portal (`billing`) | No script writes `billing`; table has 0 rows (D-16) | the tab stays empty | Target: upload route (OP-SPN-02). Owner can drop the tab. |
| Buttons and portals with high-bit object flags (layouts MBA 98, MBS 43, 82, 103, 113, 172, MBS_SendMail 4) | flag meaning not decoded (D-27) | a control that FileMaker hid shows in the target | Treated as visible and active. Owner confirms. |
| MBA "settings" button (12 layouts) | the script is deleted (D-32) | none | No route; the admin settings page replaces it. |
| Seminar calendar dates | `calendar_seminar` has one date per row; layout 61 has no caller (D-21) | none | The calendar dialog edits the one date. |

## Open decisions

| ID | Decision | Default for the demo | Affects |
|---|---|---|---|
| OD-05-01 | One route family with `{kind}` for the three offering types, or three route families | `{kind}` param (fewer routes, same handlers) | CRS, ENR routes |
| OD-05-02 | Send-all as a background job with polling (JOB-01), or one long request | JOB-01 with a 1 s poll | OP-MAIL-04, UI-26 |
| OD-05-03 | Grades file format | XLSX in and out (D-23). The source export is XLSX; the source import gives no file type (BR-ENR-06, audit fix) | OP-ENR-06, OP-ENR-07 |
| OD-05-04 | Print to PDF on the server | no: browser print to PDF (`decisions.md`) | UI-34, UI-35 |
| OD-05-05 | Header edit of a registration uses OP-REG-01's form | superseded (2026-11-21): per-field `PATCH /registrations/{id}`; no header form | `PATCH /registrations/{id}` |
