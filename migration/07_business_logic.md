# Business-Logic Recovery and Target Contract

Scope: MBA.fmp12 (convention), MBS.fmp12 (school), MBS_SendMail.fmp12 (bulk e-mail), rebuilt as one web app with one SQLite database (D-01). Demo build: no FileMaker data is migrated (D-02). Live SQL replaces the stored snapshots and recalc flags (D-03, D-04).

Labels: **fact** = read from the object file that the row cites; **inference** = drawn from names or structure; **product decision** = operator answer or Step 5 default (D-id given); **changed** = the target fixes or changes as-written behavior (D-31 default: fix defects, list each change).

Forward ids were filled in the reconcile pass (task 3c868fb3): routes, JOB- and EXT- ids are in `05_api_design.md`, screens UI-nn in `06_ui_spec.md`, milestones M1, M2 and increments I1 to I5 in `03_migration_plan.md`. Policies AP-nn are defined in `08_auth_roles.md`. Tables, columns and views are in `04_database_schema.sql`.

Full inventory: `07_business_logic_appendix.md` (212 scripts, 552 logic-bearing fields, 24 custom functions, 8 layouts with conditional formats or triggers, 14 custom menus that run scripts, 3 file triggers).

Domains: REG (registrations), EVT (events, items, packages), SPN (sponsors, billing), PPL (people), STU (students), AFF (affiliates), CRS (courses, seminars, short courses, calendars), ENR (enrollments), CRT (certifications), MAIL (e-mail), RPT (reports and print), AUTH, SYS.

## Evidence and coverage

| Logic | Object kind | Explorer | Count | Disposition coverage | Unresolved |
|---|---|---|---:|---|---|
| Scripts | `scripts` | script | 212 (MBA 69, MBS 126, MBS_SendMail 17) | 212/212: keep 109, replace 36, retire 67 (appendix §1) | 0 |
| Calculated fields | `tables` | table | 111 calculated + 41 summary | 152/152: views, app code or report aggregates (appendix §2; 04 field register) | 0 |
| Field validation/auto-enter | `tables` | table | 394 auto-enter fields (174 auto-enter calculations), 8 validation rules | 552 logic lines, all mapped (appendix §2) | 3 validations name no list (`progress`, `Sesión` x2); both tables or fields dropped |
| Custom functions | `custom-functions` | function | 24 | 24/24 (appendix §3) | 0 |
| Value lists | `value-lists` | value list | 36 | 36/36 (04 appendix §4) | 0 |
| Conditional formatting | `layouts` | layout | 11 format lines, 46 items, on 4 layouts | 4/4 (appendix §4) | 0 |
| Hide-object rules | `layouts` | layout | 0 | n/a | high-bit object flags on buttons and portals are not decoded (D-27) |
| Triggers and buttons | `layouts` | layout | 5 object triggers, 261 buttons, 0 layout triggers | triggers 5/5 (appendix §4); buttons through their scripts (appendix §1) | 12 MBA buttons run a deleted "settings" script (D-32) |
| Custom menu overrides | `custom-menus` | menu | 111 menus, 15 sets; 14 menus run scripts | 14/14 (appendix §5) | standard-menu removals unresolved (D-41, no impact) |
| File triggers | `file` | file | 3 OnFirstWindowOpen | 3/3 (appendix §6) | 0 |
| Security predicates/restrictions | `privilege-sets` | security | 9 standard sets, 0 record or field rules, 0 `runFullAccess` scripts | 9/9 (08) | 0 |

Detection signals used (reference/business-logic-detection.md): durable writes (43 data-change scripts), `ExecuteSQL` calculations (stored snapshots in `events`, `registration`, `packages`, `students`, `student_certifications`, `calendar_course`), multi-entity orchestration (scripts 45, 50, 34, MBS_SendMail 1), external effects (MBS_SendMail 8, 9, 10, 18; MBS 30, 127, 128, 173), overwriting auto-enters (status, totals and flags), developer comments (`registration` #43, `enrollments_course` #35).

## Domain invariants and operations

Each operation row gives its policy (08). Each rule cites its object file and step or field id. Started-by values: button, field trigger, layout trigger, menu, file open, schedule, other script.

### REG: registrations (MBA)

| Domain/rule id | Plain-language invariant | Started by | Preconditions/inputs | Reads | Writes/found-set scope | Error/exception behavior | Object files | Open question |
|---|---|---|---|---|---|---|---|---|
| BR-REG-01 | A registration belongs to one person. The event can only be picked from events with status Activo. The event is picked once: when the registration already has an event, the picker does not open (script 41 steps 3-4). Audit fix: the old row left out the "already set" stop and cited the wrong steps. | button | person exists; `registration.event_uuid` empty | `events.status` | `registration.event_uuid` | Event already set: nothing happens (script 41 steps 3-4). No active event: message "Al momento no hay eventos activos.", nothing changes (script 41 steps 12-15). | `objects/MBA.fmp12/scripts/41-registrations.selectEvent-open.txt`, `objects/MBA.fmp12/scripts/42-registrations.selectEvent-select.txt` | none |
| BR-REG-02 | Picking a plan deletes every item of the registration, then creates one item per active plan item (`package_items.active = 1`), and sets `package_uuid` and `event_uuid` from the plan. When a plan is already set, the user confirms first. | button | registration has an event; plan belongs to that event and is active | `package_items` where active = 1 | delete all `registration_items` of this registration (steps 5-10: find by `$$registration_uuid`, Delete All Records); create items (steps 23-33); set registration (steps 37-38) | Find error skips the delete (step 9). Changed: one transaction; the target never writes the picker record (D-31). | `objects/MBA.fmp12/scripts/44-registrations.selectPackage-open.txt`, `objects/MBA.fmp12/scripts/45-registrations.selectPackage-select.txt` | none |
| BR-REG-03 | Every item of a registration has `ticket_qty` = number of guests + 1. It is recomputed when guest badges change and when a guest is removed. Changed (operator decision D-88, 2026-11-23): a qty set by hand survives a guest change. The recompute writes only items with `qty_manual` = 0; a hand edit of qty sets `qty_manual` = 1. | field trigger (OnObjectExit on `registration_guests::name_badge`, layout 29) | none | guest count | `registration_items.ticket_qty` of the registration's items with `qty_manual` = 0 (source: step 8-14, every portal row) | none | `objects/MBA.fmp12/scripts/38-registration_update_guests.txt`, `objects/MBA.fmp12/scripts/39-registration_delete_guest.txt`, `objects/MBA.fmp12/layouts/29-Registro.txt` | Plan items also get guests + 1, which overrides the plan item quantity. Kept as written. Hand-set qty kept (D-88). No reset action: a plan change (BR-REG-02) rebuilds the items with `qty_manual` = 0. |
| BR-REG-04 | Registration total: with a plan, plan price + the totals of items that do not come from the plan; without a plan, the sum of item totals. | (calculation) | none | `v_package_price`, `v_registration_item_lines` | none (view `v_registration_totals.total`) | none | `objects/MBA.fmp12/tables/133-registration.txt` #14, relationship 28 | none |
| BR-REG-05 | Item price = plan item price when the item comes from the plan, else event item price; stored on creation. Item total = 0 when `no_charge` = 1 or the item comes from the plan, else `ticket_qty` x price. | (auto-enter) | none | `package_items.price`, `event_items.price` | `registration_items.price` on creation | none | `objects/MBA.fmp12/tables/135-registration_items.txt` #15, #16 | Changed: source re-evaluates price on every key change; target stores it once at creation (D-14 pattern). |
| BR-REG-06 | Hotel items take a reservation number and extra nights. Seating items (Cena, Almuerzo, Seminario, Desayuno) take a table number. Other items take nothing. | button (portal row) | item type | `event_items.type` | `hotel_reservation_num`, `adicional` or `mesa` of that one item | The dialog has only an OK button; the script always writes the dialog values back (steps 10-13, 21-23). Audit fix: the old row said a cancel keeps the old values; the source has no cancel. Target: the dialog adds Cancel, which changes nothing (changed, D-31). | `objects/MBA.fmp12/scripts/37-registration_items_edit_attributes.txt` steps 7-24; `objects/MBA.fmp12/tables/135-registration_items.txt` #25, #29 | none |
| BR-REG-07 | A registration is "Convencionista" when it has at least one Hotel item, else "Participante". | (calculation) | none | item types | none (`v_registration_totals.registration_type`) | none | `objects/MBA.fmp12/tables/133-registration.txt` #49, #50, #47 | Changed: source `is_convencionista` reads the context of the first related item (D-31). Target uses has_hotel. |
| BR-REG-08 | Balance = total - sum of payments. | (calculation) | none | payments | none (view) | none | `objects/MBA.fmp12/tables/133-registration.txt` #19, #29 | none |
| BR-REG-09 | A payment is never deleted. ROLE-ADMIN voids it with a reason; voided payments do not count. A registration with a payment that is not voided is cancelled (status Canceló), not deleted. With only voided payments it can be deleted; the voided rows go with it and the audit row keeps them (D-93). | button | reason text | payment | `payments.voided_at/by/void_reason`; `audit_log` | Missing reason: refuse. | product decision (D-09); `objects/MBA.fmp12/relationships/4-registration_payments.txt` (cascade delete in source) | none |
| BR-REG-10 | New registration status = "Nuevo". Allowed values: Nuevo, Registrado, Asistió, Canceló, No Canceló, No Asistió. Door check-in sets Asistió / No Asistió / Canceló, stored as written (D-91, migration 0002). | (auto-enter) | none | none | `registration.status` | none | `objects/MBA.fmp12/tables/133-registration.txt` #16; `objects/MBA.fmp12/value-lists/2-registration_status.txt`; `objects/MBA.fmp12/value-lists/8-registro_puerta_status.txt` | OD-04-03 (Nuevo not in list) |
| BR-REG-11 | Guests are names only; they never link to people. | none | none | none | none | none | `objects/MBA.fmp12/tables/134-registration_guests.txt` #3 (never set) | D-15 default |
| OP-REG-01 | Create a registration from a person record. | button (Convencionista) | person | none | new `registration` (serial `registration_number`) | none | `objects/MBA.fmp12/scripts/21-people_create_registration.txt`, `objects/MBA.fmp12/scripts/33-registration_newRecord.txt` | AP-03 |
| OP-REG-02 | Select the event of a registration (BR-REG-01). | button | none | events | registration | as BR-REG-01 | scripts 41, 42 | AP-03 |
| OP-REG-03 | Select the plan (BR-REG-02). | button | confirm when replacing | as BR-REG-02 | as BR-REG-02 | as BR-REG-02 | scripts 44, 45 | AP-03, AP-09 |
| OP-REG-04 | Add or remove one item by hand (event items of the registration's event only). | portal | item of same event (relationship 8, value list 11) | event items | one `registration_items` row | Removing an item: confirm. | `objects/MBA.fmp12/layouts/29-Registro.txt`, `objects/MBA.fmp12/value-lists/11-registrationSelectEventItems.txt` | AP-06 |
| OP-REG-05 | Set item attributes (BR-REG-06), or price, qty or no charge by hand. A qty set by hand also sets `qty_manual` = 1 (BR-REG-03, D-88). | button, field | none | item | one item | none | script 37 | AP-03 |
| OP-REG-06 | Add, edit or remove a guest, then apply BR-REG-03. | portal, field trigger | none | guests | guests; `ticket_qty` of items with `qty_manual` = 0 (D-88) | none | scripts 38, 39 | AP-06 |
| OP-REG-07 | Record a payment (amount, type, number). | button | amount > 0 (changed: strengthened) | none | new `payments` row | none | `objects/MBA.fmp12/scripts/35-registration_add_payment.txt` | AP-03 |
| OP-REG-08 | Void a payment (BR-REG-09). | button | reason | payment | payment, audit | none | product decision (D-09) | AP-05 |
| OP-REG-09 | Cancel a registration; delete it only when it has no payment that is not voided (items and guests cascade; voided payments are deleted with it, D-93). | button | none | payments | registration | Has a payment not voided: refuse delete, offer cancel. | product decision (D-09); relationships 4, 5, 7 | AP-05, AP-06 |
| OP-REG-10 | Door check-in: set attendance status. | button / field | none | none | `registration.status` | none | `objects/MBA.fmp12/layouts/55-Registro_Puerta.txt` | AP-03 |

### EVT: events, items, packages (MBA)

| Domain/rule id | Plain-language invariant | Started by | Preconditions/inputs | Reads | Writes/found-set scope | Error/exception behavior | Object files | Open question |
|---|---|---|---|---|---|---|---|---|
| BR-EVT-01 | Event: end date defaults to the start date when empty. Event item: end date always equals the start date (auto-enter `date_start`, overwrites on each change). Audit fix: the old row gave the "when empty" rule to event items too. | (auto-enter) | none | date_start | date_end | none | `objects/MBA.fmp12/tables/131-events.txt` #15; `objects/MBA.fmp12/tables/132-event_items.txt` #15 | none |
| BR-EVT-02 | An event is active when status = Activo (new events start Activo). Only active events are offered to plans and registrations. | (auto-enter) | none | status | none | none | `objects/MBA.fmp12/tables/131-events.txt` #17, #18; `objects/MBA.fmp12/relationships/16-packages_package_active_events.txt` | none |
| BR-EVT-03 | A new event item gets `sort_order` = number of items of the event + 1. | (auto-enter) | none | event items | `event_items.sort_order` | none | `objects/MBA.fmp12/tables/132-event_items.txt` #22 | none |
| BR-EVT-04 | Event totals: registrations, guests, registrations + guests, sum of registration totals, sum of payments, sum of balances, sum of item prices, sponsor promised, received and promised + received. Computed live. | (calculation) | none | registrations, sponsors | none (`v_event_totals`) | none | `objects/MBA.fmp12/tables/131-events.txt` #21-#31; `objects/MBA.fmp12/scripts/15-event.save.txt` | AMB-07-03, OD-04-04 |
| BR-EVT-05 | Plan price = `price_override` when set, else the sum of price x quantity of the active plan items. | (calculation) | none | package items | none (`v_package_price`) | none | `objects/MBA.fmp12/tables/136-packages.txt` #13; `objects/MBA.fmp12/tables/137-package_items.txt` #18 | none |
| BR-EVT-06 | Building a plan creates one plan item per event item of the plan's event that the plan does not have yet, in event-item order. It needs an event with at least one item. | layout trigger (OnObjectSave on `packages::event_key`, layout 40) | plan has an event; event has items | event items | new `package_items` rows only | No event: "Favor de seleccionar el evento." No items: "El Evento seleccionado no tiene detalles..." (steps 5-16). | `objects/MBA.fmp12/scripts/50-package_select_event.txt` | none |
| BR-EVT-07 | A plan item copies price and quantity from its event item when it is created. | (auto-enter) | none | event item | `package_items.price`, `ticket_qty` | none | `objects/MBA.fmp12/tables/137-package_items.txt` #14, #17 | none |
| OP-EVT-01 | Create, edit, delete an event; attach or remove its PDF. | menu, button | delete only without registrations (AP-04) | none | `events`, `stored_files` | Has registrations: refuse delete. | `objects/MBA.fmp12/scripts/26-event_newRecord.txt`, `objects/MBA.fmp12/scripts/13-event.deletePDF.txt` | AP-03, AP-04 |
| OP-EVT-02 | Save an event (form submit); totals are live, nothing else is stored. | button | none | none | `events` | none | script 15 | AP-03 |
| OP-EVT-03 | Add, edit, delete event items. | portal button | delete only when no registration item uses it (FK) | none | `event_items` | FK refusal shows a message. | `objects/MBA.fmp12/scripts/25-event_create_event_item.txt` | AP-03, AP-06 |
| OP-EVT-04 | Build plan items (BR-EVT-06). | field trigger | as BR-EVT-06 | | | | script 50 | AP-03 |
| OP-EVT-05 | Create and edit plans; toggle a plan item active; add or remove plan items. | button | none | none | `packages`, `package_items` | none | `objects/MBA.fmp12/scripts/48-package_create_item.txt`, `objects/MBA.fmp12/scripts/49-package_active.txt` | AP-03, AP-06 |

### SPN: sponsors and billing (MBA)

| Domain/rule id | Plain-language invariant | Started by | Preconditions/inputs | Reads | Writes/found-set scope | Error/exception behavior | Object files | Open question |
|---|---|---|---|---|---|---|---|---|
| BR-SPN-01 | A sponsor belongs to one event and has a category (Platino, Oro, Plata, Bronce, Otro) and requested, promised and received amounts. Event sponsor total = promised + received. | (calculation) | none | sponsors | none | none | `objects/MBA.fmp12/tables/140-sponsor.txt`; `objects/MBA.fmp12/tables/131-events.txt` #29-#31; `objects/MBA.fmp12/relationships/21-events_sponsor.txt` | AMB-07-04 |
| BR-SPN-02 | Billing files attach to an event (Facturación tab). No script writes them. | button | file | none | `billing`, `stored_files` | none | `objects/MBA.fmp12/tables/141-billing.txt`; `objects/MBA.fmp12/layouts/35-Convencion.txt` | D-16 |
| OP-SPN-01 | Create, edit, delete sponsors of an event. | button | none | none | `sponsor` | none | `objects/MBA.fmp12/layouts/45-Auspiciadores.txt` | AP-03, AP-06 |
| OP-SPN-02 | Upload or remove a billing file. | button | file | none | `billing` | none | `objects/MBA.fmp12/layouts/35-Convencion.txt` | AP-03 |

### PPL: people (MBA)

| Domain/rule id | Plain-language invariant | Started by | Preconditions/inputs | Reads | Writes/found-set scope | Error/exception behavior | Object files | Open question |
|---|---|---|---|---|---|---|---|---|
| BR-PPL-01 | Full name = first + " " + last; address block = address, then city state zip. `name_badge` is the badge text. | (calculation) | none | person | none (app) | none | `objects/MBA.fmp12/tables/130-people.txt` #24, #25 | none |
| BR-PPL-02 | A person with registrations is not deleted. | button | none | registrations | none | Refuse with a message. | product decision (D-09); relationship 1 | none |
| OP-PPL-01 | Create and edit a person. | menu | none | none | `people` | none | `objects/MBA.fmp12/scripts/22-people_newRecord.txt` | AP-03 |
| OP-PPL-02 | Delete a person (BR-PPL-02). | menu | none | registrations | `people` | as BR-PPL-02 | `objects/MBA.fmp12/custom-menus/36-people.records.txt` | AP-04 |

### AFF: affiliates (MBS, shared)

| Domain/rule id | Plain-language invariant | Started by | Preconditions/inputs | Reads | Writes/found-set scope | Error/exception behavior | Object files | Open question |
|---|---|---|---|---|---|---|---|---|
| BR-AFF-01 | An affiliate is active when `cust_type` = ACTIVO. A student linked to an active affiliate is an affiliate student and gets member prices. | (calculation) | none | affiliate | none (`v_student_academic.is_affiliate`) | none | `objects/MBS.fmp12/tables/131-students.txt` #63; `objects/MBS.fmp12/value-lists/17-activo_inactivo.txt` | none |
| BR-AFF-02 | Deleting an affiliate unlinks its students (`member_uuid` set to empty); their typed company name stays. | button | none | students | students.member_uuid | none | product decision (D-11); relationship 1 | none |
| OP-AFF-01 | Create, edit, delete affiliates. | menu, button | none | none | `affiliates` | none | `objects/MBS.fmp12/layouts/48-Afiliado.txt` | AP-03, AP-04 |

### STU: students (MBS)

| Domain/rule id | Plain-language invariant | Started by | Preconditions/inputs | Reads | Writes/found-set scope | Error/exception behavior | Object files | Open question |
|---|---|---|---|---|---|---|---|---|
| BR-STU-01 | A new or duplicated student gets one `student_certifications` row per certification. One row per student and certification. | menu (New Record, Duplicate Record overrides) | none | certifications | new student, then one row per certification (script 34 steps 4-15) | Changed: the target skips pairs that exist (UNIQUE, D-31); the source duplicates rows on a rerun. | `objects/MBS.fmp12/scripts/33-student_new_record.txt`, `objects/MBS.fmp12/scripts/34-student_create_certification_records_student_uuid.txt`, `objects/MBS.fmp12/scripts/35-student_duplicate_record.txt` | none |
| BR-STU-02 | Source: the user types the company name (`COMPAÑIA`); `member_uuid` auto-enters the uuid of the affiliate whose `Socio` equals that text (relationship 17). The auto-enter formula of `COMPAÑIA` is not in the DDR (unavailable). Target: the user picks an affiliate; the app sets `member_uuid` and copies the affiliate name into `compania`. Audit fix: the old row stated the target direction as source fact. | field | typed company name (source); picked affiliate (target) | affiliate | `member_uuid`, `compania` | No match: `member_uuid` empty (source). | `objects/MBS.fmp12/tables/131-students.txt` #12, #35; `objects/MBS.fmp12/relationships/17-students_get_affiliate_uuid.txt` | Changed: by key, not by name match (D-11). |
| BR-STU-03 | A student is ACTIVO when at least one course enrollment is in a calendar row with status Confirmado, else INACTIVO. Live. | (calculation) | none | enrollments, calendar | none (`v_student_academic`) | none | `objects/MBS.fmp12/scripts/21-sase.updateStudentRecord.txt` steps 1-25 | Changed: live instead of the SASE job (D-04). |
| BR-STU-04 | Display name = name + " (company)" when the company is set. | (auto-enter) | none | student | none (app) | none | `objects/MBS.fmp12/tables/131-students.txt` #34 | none |
| BR-STU-05 | A certification of interest records the date it was set. Up to 6 per student. | field | none | none | `student_certification_interests` | none | `objects/MBS.fmp12/tables/131-students.txt` #23, #24 | none |
| BR-STU-06 | Academic summary: average score of passed course enrollments (rounded), total credits and hours of passed enrollments, last course date. | (calculation) | none | enrollments | none (`v_student_academic`) | none | `objects/MBS.fmp12/tables/131-students.txt` #49-#51, #67, #68 | none |
| BR-STU-07 | Only ROLE-ADMIN deletes a student, and only when the student has no enrollments. Certification rows and interests cascade. | menu | none | enrollments | student | Has enrollments: refuse. | product decision (D-09); relationships 2, 18, 25, 30 (cascade in source) | none |
| OP-STU-01 | Create a student (BR-STU-01). | menu | none | | | | scripts 33, 34 | AP-03 |
| OP-STU-02 | Duplicate a student (BR-STU-01). | menu | none | | | | script 35 | AP-03 |
| OP-STU-03 | Edit a student, affiliate and interests (BR-STU-02, BR-STU-05). | form | none | | | | `objects/MBS.fmp12/layouts/43-Estudiante.txt` | AP-03 |
| OP-STU-04 | Delete a student (BR-STU-07). | menu | | | | | product decision | AP-04 |
| OP-STU-05 | Find students: 0 hits shows a message, 1 hit opens the form, more opens the list. | menu (Quick Find override) | search text | students | none | none | `objects/MBS.fmp12/scripts/45-student_quick_find.txt` | AP-03 |

### CRS: courses, seminars, short courses, calendars (MBS)

| Domain/rule id | Plain-language invariant | Started by | Preconditions/inputs | Reads | Writes/found-set scope | Error/exception behavior | Object files | Open question |
|---|---|---|---|---|---|---|---|---|
| BR-CRS-01 | A calendar row is active when status is Pendiente or Confirmado. Only active rows of the chosen offering are offered when an enrollment picks its date. | (calculation) | none | status | none | none | `objects/MBS.fmp12/custom-functions/4-isCourse_valid.txt`; `objects/MBS.fmp12/tables/143-calendar_short_course.txt` #24; `objects/MBS.fmp12/tables/144-calendar_seminar.txt` #24; relationships MBS 5, 20, 32 | none |
| BR-CRS-02 | A new calendar row starts Pendiente and copies member and individual prices from its offering (once, on creation). | button | offering | offering prices | calendar row | none | `objects/MBS.fmp12/tables/141-calendar_course.txt` #12, #19, #20; `objects/MBS.fmp12/scripts/52-course_add_date.txt` | none |
| BR-CRS-03 | Needed = quorum - enrollments of the calendar row. | (calculation) | none | enrollments | none (`v_calendar_*_summary`) | none | `objects/MBS.fmp12/tables/141-calendar_course.txt` #16-#18 | AMB-07-05 (seminar counts "?") |
| BR-CRS-04 | NMLS total = hours x NMLS rate (setting, 1.50) when `nmls_fee` = 1. | (auto-enter) | none | short course | none (app) | none | `objects/MBS.fmp12/tables/135-short_courses.txt` #35-#37 | D-14 |
| BR-CRS-05 | Calendar rows are not deleted by anyone; staff cancel them with status Cancelado. | button | none | none | none | Source: every Borrar button passes access 0, so the script shows "No tiene acceso para borrar." | `objects/MBS.fmp12/scripts/164-g.deletePortalRow_access.txt`; `objects/MBS.fmp12/layouts/57-Cursos_Especializados.txt` | D-09 default |
| BR-CRS-06 | A class day is Día de Clase or Día de Fiesta. Holidays print as "No habrá clases el ..." on letters. | (calculation) | none | dates | none | none | `objects/MBS.fmp12/tables/142-calendar_course_dates.txt` #14; `objects/MBS.fmp12/tables/139-enrollments_course.txt` #90, #92 | none |
| OP-CRS-01 | Create and edit courses, seminars, short courses. | form | none | | offering tables | none | layouts 57, 60, 64 | AP-03, AP-04 (delete) |
| OP-CRS-02 | Add or edit a calendar row (BR-CRS-02). | button | | | calendar tables | | scripts 52, 58, 64 | AP-03, AP-07 |
| OP-CRS-03 | Manage the class dates of a calendar row (course: child rows; short course: up to 15 dates). | dialog | | | `calendar_course_dates`, `calendar_short_course_dates` | | `objects/MBS.fmp12/layouts/58-Fechas_de_Curso.txt`, `objects/MBS.fmp12/layouts/65-Fechas_del_Curso_Corto.txt` | AP-03, AP-06 |
| OP-CRS-04 | Maintain professors. | admin screen | | | `professors` | | `objects/MBS.fmp12/layouts/66-Tabla_de_Profesores.txt` | AP-08 |
| OP-CRS-05 | Open the enrollment list of a calendar row; no enrollments shows a message. | button | | enrollments | none | Empty: message. | scripts 54, 60, 65 | AP-03 |

### ENR: enrollments (MBS)

| Domain/rule id | Plain-language invariant | Started by | Preconditions/inputs | Reads | Writes/found-set scope | Error/exception behavior | Object files | Open question |
|---|---|---|---|---|---|---|---|---|
| BR-ENR-01 | The enrollment price is the member price of the chosen calendar row when the student is an affiliate student (BR-AFF-01), else the individual price. It is stored when the calendar row is picked. | field (calendar pick) | active calendar row | student, calendar row | `costo` / `cargo` | none | `objects/MBS.fmp12/tables/139-enrollments_course.txt` #10; `objects/MBS.fmp12/tables/138-enrollments_seminars.txt` #86; `objects/MBS.fmp12/tables/140-enrollments_short_course.txt` #86 | Changed: source re-evaluates on every key change; target stores once (D-14). |
| BR-ENR-02 | Balance = price - payment. | (calculation) | none | | none (view) | none | same files, #11 / #96 | none |
| BR-ENR-03 | Score is 0 to 100. When the final average is set, the score takes its value. | field | numeric | promedio_final | `puntuacion` | Out of range: validation error (fact: Range 0-100, StrictDataType). | `objects/MBS.fmp12/tables/139-enrollments_course.txt` #57, #124 | none |
| BR-ENR-04 | Letter grade: >= 90 A, >= 80 B, >= 70 C, >= 60 D, else F. Same scale for student and certification averages. | (auto-enter) | none | score | none (view) | none | `objects/MBS.fmp12/tables/139-enrollments_course.txt` #28; `objects/MBS.fmp12/tables/131-students.txt` #65; `objects/MBS.fmp12/tables/146-student_certifications.txt` #29 | none |
| BR-ENR-05 | An enrollment is passed when score > 69. | (auto-enter) | none | score | none (view) | none | `objects/MBS.fmp12/tables/139-enrollments_course.txt` #82 | none |
| BR-ENR-06 | Grades import updates course enrollments matched on `id` (#61, target `serial`; not `ID #:` #38, target `id_num`): exam, eight assignments, assignment average, attendance and punctuality, final average. Auto-enter runs during the import, so `Puntuacion`, `NOTA` and `is_passed` follow the imported final average (BR-ENR-03..05). Rows with no match change nothing. The user picks the file in the import dialog; the DDR gives no file type (character set Mac Roman). The user sees a preview first. Audit fix: the old row did not name the match field, left out the score recompute, and stated XLSX as fact. | menu | file from the export (BR-ENR-07, XLSX; product decision) | file rows | matched `enrollments_course` rows only; recompute score, letter, passed | Unmatched rows are listed, not created. Changed: preview step (D-23). | `objects/MBS.fmp12/scripts/128-enroll_course_import_registro_de_notas.txt` step 1 (Method: Update, match on id) | none |
| BR-ENR-07 | Grades export writes the current list of course enrollments with id, name and the grade columns. | menu | filter | enrollments | file | none | `objects/MBS.fmp12/scripts/127-enroll_course_export_registro_de_notas.txt` | Source also creates an e-mail; not rebuilt (inference: convenience step). |
| BR-ENR-08 | Deleting an enrollment asks for confirmation. An enrollment with a payment is cancelled (status BAJA or Canceló), not deleted. | menu | confirm | payment | enrollment | Commit error: "No se pudo completar la acción." | `objects/MBS.fmp12/scripts/165-gEnrollment.deleteRecord.txt` steps 1-10 | Changed: payment guard (D-09). |
| BR-ENR-09 | A new course enrollment has status ACTIVO; reports count ACTIVO and BAJA. | (auto-enter) | none | | `status` | none | `objects/MBS.fmp12/tables/139-enrollments_course.txt` #35 (comment "rename to is_baja") | OD-04-02 |
| OP-ENR-01 | Create a course enrollment from the student form; pick an active calendar row (BR-CRS-01, BR-ENR-01). | button | student | | `enrollments_course` | | `objects/MBS.fmp12/scripts/37-student_new_course_enrollment.txt`; `objects/MBS.fmp12/layouts/82-Matricula_de_Curso.txt` | AP-03 |
| OP-ENR-02 | Create a seminar enrollment. | button | | | `enrollments_seminars` | | script 38; layout 103 | AP-03 |
| OP-ENR-03 | Create a short-course enrollment. | button | | | `enrollments_short_course` | | script 39; layout 113 | AP-03 |
| OP-ENR-04 | Save an enrollment (payment, payer, notes). | button | | | enrollment | | `objects/MBS.fmp12/scripts/166-gEnrollment.guardar.txt` | AP-03 |
| OP-ENR-05 | Delete or cancel an enrollment (BR-ENR-08). | menu | | | | | script 165 | AP-05 |
| OP-ENR-06 | Export grades (BR-ENR-07). | menu | | | | | script 127 | AP-10 |
| OP-ENR-07 | Import grades (BR-ENR-06). | menu | | | | | script 128 | AP-09 |
| OP-ENR-08 | Enter grades by hand on the enrollment form. | form | | | grade columns | | layout 82 | AP-03 |

### CRT: certifications (MBS)

| Domain/rule id | Plain-language invariant | Started by | Preconditions/inputs | Reads | Writes/found-set scope | Error/exception behavior | Object files | Open question |
|---|---|---|---|---|---|---|---|---|
| BR-CRT-01 | A requirement is required when its type is Requisito and elective when Electiva. | (auto-enter) | none | type | none | none | `objects/MBS.fmp12/tables/137-certification_requirements.txt` #16, #18 | none |
| BR-CRT-02 | Progress: total = required courses + required electives; completed = passed required courses of the certification; left = max(total - completed, 0); courses left = required courses with no passed enrollment. | (calculation) | none | requirements, enrollments | none (`v_student_certification_progress`) | none | `objects/MBS.fmp12/tables/146-student_certifications.txt` #21, #22, #24, #37, #38; `objects/MBS.fmp12/relationships/27-student_certifications_student_certification_enrollments.txt` | Changed: completed counts distinct courses; source counts enrollments (D-31). |
| BR-CRT-03 | Certification average = rounded average score of those passed enrollments; credits and hours = their sums. | (calculation) | none | | none (view) | none | same file #28, #30, #31 | none |
| BR-CRT-04 | A certification is graduated when its graduation date is set. The certificate prints only for a graduated certification. | button | graduated | | none | Not graduated: no certificate. | same file #25, #26; `objects/MBS.fmp12/scripts/48-students_print_certification.txt` | none |
| OP-CRT-01 | Maintain certifications and their requirements. | admin screen | | | `certifications`, `certification_requirements` | | `objects/MBS.fmp12/layouts/59-Certificaciones.txt` | AP-08 |
| OP-CRT-02 | Set graduation date and note. | form | | | `student_certifications` | | `objects/MBS.fmp12/layouts/43-Estudiante.txt` | AP-03 |
| OP-CRT-03 | Print the graduation certificate (BR-CRT-04). | button | | | none | | script 48 | AP-10 |

### MAIL: bulk e-mail (MBS_SendMail)

| Domain/rule id | Plain-language invariant | Started by | Preconditions/inputs | Reads | Writes/found-set scope | Error/exception behavior | Object files | Open question |
|---|---|---|---|---|---|---|---|---|
| BR-MAIL-01 | A new batch takes the default sender profile, the default template (setting), title "Sin título", and copies subject and HTML body from the template. | other script | none | profiles, template | `mail_batches` | none | `objects/MBS_SendMail.fmp12/tables/132-batches.txt` #5, #10, #11, #23, #24 | Changed: template from a setting, not one fixed uuid (D-20). |
| BR-MAIL-02 | A batch has one e-mail per contact. Recipient = the student's work e-mail and e-mail, empty ones dropped, joined with ", ". The name is copied. | other script | contact list | students | `mail_emails` | none | `objects/MBS_SendMail.fmp12/scripts/1-g.createEmails_contact_ids.txt` steps 5-20; `objects/MBS_SendMail.fmp12/tables/130-emails.txt` #5, #13 | none |
| BR-MAIL-03 | The body replaces `<SCI_NAME>` (recipient name), `<SCI_BODY>` (batch message as HTML), `<SCI_USER>` and `<SCI_TITLE>` (sender profile name and title) in the batch HTML. | other script | | | rendered body | none | `objects/MBS_SendMail.fmp12/tables/130-emails.txt` #4; `objects/MBS_SendMail.fmp12/scripts/17-emails.setBody.txt` | Script 17 writes `<SCI_TITLE` without `>` (D-20). The target matches the token with `>`. |
| BR-MAIL-04 | Success sets `sent` = 1 and `sent_on`. Failure sets `error` = 1 and puts "Create: ..." or "Send: ..." with the provider message on top of `error_log`. | button | | | one e-mail | as stated | `objects/MBS_SendMail.fmp12/scripts/8-email.send.txt` steps 1-24 | none |
| BR-MAIL-05 | Status = Sent when sent, Error when error, else Draft. Batch counts: e-mails, sent, errors. Batch text: when at least one e-mail is sent, "n enviado(s)", plus " m error(es)" when errors > 0; when none is sent, "Nuevo", also when errors exist. Audit fix: the old row gave "n enviados m errores" or "Nuevo" and hid that a batch with only errors shows "Nuevo". Target: a batch with errors and no sent e-mail shows "0 enviados m errores" (changed, D-31). | (auto-enter) | | | none (view, app) | | `objects/MBS_SendMail.fmp12/tables/130-emails.txt` #19, #20; `objects/MBS_SendMail.fmp12/tables/132-batches.txt` #14-#17, #22 | none |
| BR-MAIL-06 | Send all sends every e-mail of the batch that is not sent yet, one by one; a failure does not stop the others. | button | | | each e-mail | Per-e-mail error (BR-MAIL-04). | `objects/MBS_SendMail.fmp12/scripts/18-email.buttonAll.txt` | Changed: source resends sent e-mails (D-20). |
| BR-MAIL-07 | From a calendar row, the recipients are the students enrolled in that row (module ce, cc or s). No enrollments: nothing happens. | button | calendar row | enrollments | new batch | Empty list: exit (step 11-12). | `objects/MBS.fmp12/scripts/173-sciSendMai.caller_module.txt` | none |
| BR-MAIL-08 | Delivery goes through Resend when `RESEND_API_KEY` is set; else to `mail_outbox` with provider mock, shown in the app. No SMTP host, user, password or licence key exists in code, config or data. | (adapter) | | | `mail_outbox` row per send | Provider error -> BR-MAIL-04. | product decision (D-05); scripts 8, 9, 10, 11, 12 | none |
| BR-MAIL-09 | A sender profile e-mail is unique. | (validation) | | | | Duplicate: refuse. | `objects/MBS_SendMail.fmp12/tables/133-profiles.txt` #1 (Unique) | none |
| OP-MAIL-01 | Create a batch from a calendar row (BR-MAIL-07, BR-MAIL-01, BR-MAIL-02). | button | | | | | MBS 173; MBS_SendMail 1 | AP-03 |
| OP-MAIL-02 | Edit a batch: title, message, sender, attachment. | form | draft batch | | `mail_batches` | | `objects/MBS_SendMail.fmp12/scripts/21-email.attachment.txt`; layout 4 | AP-03 |
| OP-MAIL-03 | Send one e-mail (BR-MAIL-03, -04, -08). | button | | | | | MBS_SendMail 8, 11, 17 | AP-03 |
| OP-MAIL-04 | Send all (BR-MAIL-06). | button | confirm | | | | MBS_SendMail 18 | AP-09 |
| OP-MAIL-05 | Preview the rendered e-mail. | button | | | none | | MBS_SendMail 7; layout 9 | AP-03 |
| OP-MAIL-06 | Maintain templates. | admin screen | | | `mail_templates` | | `objects/MBS_SendMail.fmp12/tables/129-templates.txt` | AP-08 |
| OP-MAIL-07 | Maintain sender profiles (BR-MAIL-09). | admin screen | | | `mail_profiles` | | `objects/MBS_SendMail.fmp12/layouts/12-profiles.txt` | AP-08 |
| OP-MAIL-08 | View the outbox. | screen | | `mail_outbox` | none | | product decision (D-05) | AP-13 |

### RPT: reports and print

| Domain/rule id | Plain-language invariant | Started by | Preconditions/inputs | Reads | Writes/found-set scope | Error/exception behavior | Object files | Open question |
|---|---|---|---|---|---|---|---|---|
| BR-RPT-01 | Invoices: registrations paid by Individuo print the individual invoice; paid by Afiliado print the company invoice. Scope: the current event. | button | event | registrations | none | No records: the print window closes, no message (steps 12-15). Target shows "No hay registros para imprimir" (changed). Audit fix: the old row said the source shows a message. | `objects/MBA.fmp12/scripts/60-events.print_Registro_Individuos.txt`, `objects/MBA.fmp12/scripts/61-events.print_Registro_Afiliados.txt` | none |
| BR-RPT-02 | Enrollment statistics count enrollments with status ACTIVO plus BAJA, per course and year. | menu, button | year range | enrollments | none | none | `objects/MBS.fmp12/scripts/121-reports.CE.ingresosPorCursoYAfiliado.txt`, scripts 123, 124; `objects/MBS.fmp12/custom-functions/13-sql.enrollmentsCount.txt` | D-13 default: show each status as its own count too. |
| BR-RPT-03 | Revenue = sum of payments by payer (COMPAÑIA, INDIVIDUAL) per calendar row, course and affiliate. | menu | year range | enrollments | none | none | `objects/MBS.fmp12/custom-functions/14-sql.enrollmentsRevenue.txt`; scripts 121, 140 | none |
| BR-RPT-04 | Badges, confirmation letters, package labels and the convencionista list include only registrations with a hotel item. Companion badges include guests of those registrations. | button | event | registrations | none | none | MBA scripts 63, 66, 71, 72, 79 | D-15 default: a filter the user can turn off. |
| BR-RPT-05 | Credit transcript per student: courses with latest grade, credits, hours, semester, student average and completed certifications, dated today in Spanish. | menu | student | enrollments | none | none | `objects/MBS.fmp12/scripts/91-students_credit_transcription.txt`; `objects/MBS.fmp12/tables/132-courses.txt` #24-#39 | none |
| BR-RPT-06 | CSBS CE export per short-course enrollment: state, provider id, course id, offering dates, begin time, postal code, NMLS id. Constants are settings. | menu | filter | enrollments | CSV | none | `objects/MBS.fmp12/tables/140-enrollments_short_course.txt` #106-#116 | D-25 |
| BR-RPT-07 | Graduates: graduation date set. Graduation candidates: not graduated and left < 1. Certification candidates: not graduated, `left` = n (the count from BR-CRT-02, not the course list), certification in a chosen set, course date >= a date, certification GMB omitted (script 92 steps 11-18). Audit fix: the old row said "courses left", which is the course-name list. | menu | criteria | `v_student_certification_progress` | none | none | MBS scripts 92, 93, 94, 95, 97, 146 | none |
| BR-RPT-08 | Letters show dates as Spanish words ("lunes, 3 de marzo de 2026") and lists as Spanish sentences ("a, b y c"). | (function) | | | | | `objects/MBS.fmp12/custom-functions/11-solFM_fechaPalabras.txt`, `objects/MBS.fmp12/custom-functions/3-fromListToSentence.txt` | none |
| BR-RPT-09 | Affiliate member lists include only active affiliates (ACTIVO). | menu | | affiliates | none | | MBS scripts 81, 84 | none |
| BR-RPT-10 | Labels and raffle tickets print the quantity the user enters. Source: script 81 finds number rows "1..." qty (one label per number). Scripts 82 and 85 find number = qty, so they show the one row with that number (inference: defect). Target prints 1 to qty for all three (changed, D-31). Audit fix: the old row said all three print the quantity. | menu | quantity | none | none | Cancel: exit. | MBA scripts 81 step 14, 82 step 14, 85 step 14 | Changed: no numbers table (D-34). |
| BR-RPT-11 | Report years come from the data, not a fixed list. | | | | | | `objects/MBS.fmp12/value-lists/20-report.years.txt`; `objects/MBS.fmp12/scripts/170-globals.loadGlobals.txt` | D-33, D-44 |
| BR-RPT-12 | Accounting export: one row per registration (sorted by registration number) with "tickets - total" per event item, plus a total row. Only the event items the user picked (`globals::g_events`) are columns. A flag (`globals::g_boolean`) includes or leaves out plan items (steps 9, 53-61). The total row sums at most 10 item columns (steps 124-139). Audit fix: the old row left out the item choice and the plan-item flag. Target: all columns are summed (changed). | menu | event, event items, include plan items | registration items | CSV | | `objects/MBA.fmp12/scripts/95-registrations.createAccountingReport_lastTimestamp.txt` | Changed: CSV, no pivot table (D-16). |
| OP-RPT-01 | Convention print pages (29 scripts: invoices, lists, letters, badges, labels, tickets, attendance, door list, seating, sponsor letters ES/EN, sponsor labels). | menu, button | event or event item | | none | No records: message. | appendix §1 (OP-RPT-01) | AP-10 |
| OP-RPT-02 | School print pages (39 scripts: enrollment lists, policies, letters, certificates, attendance, labels, transcripts, affiliate lists). | menu | filter | | none | | appendix §1 (OP-RPT-02) | AP-10 |
| OP-RPT-03 | School admin reports with a filter form (18 scripts; drafts retired). | menu | criteria | | none | | appendix §1 (OP-RPT-03) | AP-10 |
| OP-RPT-04 | CSV and XLSX exports: accounting (BR-RPT-12), CSBS (BR-RPT-06), grades (OP-ENR-06). | menu | | | file | | MBA 95; MBS 127 | AP-10 |
| OP-RPT-05 | Enrollment dashboard: one page with live counts and revenue; replaces the chart layouts. | menu | | views | none | | layouts MBS 75, 78, 139, 141 (D-17) | AP-10 |

### AUTH and SYS

| Domain/rule id | Plain-language invariant | Started by | Preconditions/inputs | Reads | Writes/found-set scope | Error/exception behavior | Object files | Open question |
|---|---|---|---|---|---|---|---|---|
| BR-AUTH-01 | Sign-in checks username (case-insensitive) and an argon2id password hash. 5 failures lock the user for 15 minutes. | form | | `users` | `sessions`, `audit_log` | Generic error message; no hint which part failed. | product decision (D-50) | none |
| BR-AUTH-02 | A session ends after 10 minutes without a request, and at the latest 12 hours after sign-in (`sessions.expires_at`; product decision, D-90). | request | | `sessions` | `sessions.last_seen_at` | Expired: redirect to sign-in. | `objects/MBA.fmp12/extended-privileges/4-fmreauthenticate10.txt` (D-40) | none |
| BR-AUTH-03 | Every state-changing request carries the session CSRF token. | request | | | | Missing or wrong: 403. | product decision (D-50) | none |
| BR-AUTH-04 | Only ROLE-ADMIN manages users and roles. | screen | | | `users` | | `objects/MBA.fmp12/privilege-sets/1-Full_Access.txt` (manageAccounts=True only here) | none |
| BR-SYS-01 | Each module opens on its home screen: convention on events, school on the student list. No plug-in install. | file open | | | | | file triggers (appendix §6) | none |
| BR-SYS-02 | Every entity row records created and modified time and user. | (auto-enter) | | | audit columns | | `log_*` fields on every MBS and MBA table | none |
| BR-SYS-03 | Signers, logo, NMLS rate, CSBS constants and default mail template are settings that ROLE-ADMIN edits. | screen | | | `app_settings` | | D-18, D-25, D-14, D-20 | none |
| BR-SYS-04 | An uploaded file is stored once with its SHA-256, MIME type and size. | upload | | | `stored_files` | Too large or empty: refuse. | containers (04 appendix) | none |
| OP-AUTH-01 | Sign in (BR-AUTH-01). | form | | | | | product decision | AP-01, AP-11 |
| OP-AUTH-02 | Sign out. | button | | | delete session | | MBA 2, MBS 2, MBS_SendMail 20 (Re-Login) | AP-01 |
| OP-AUTH-03 | Change own password. WeakPassword: the new password has fewer than 10 characters (product decision, D-89); the same rule applies when an admin sets a password (OP-AUTH-04). | form | current password | | `users.password_hash` | Wrong current password or WeakPassword: refuse, nothing written. | `allowModifyPassword=True` in every privilege set | AP-12 |
| OP-AUTH-04 | Manage users: create, set password, disable, change role. | admin screen | | | `users`, revoke `sessions` | | BR-AUTH-04 | AP-08, AP-11 |
| OP-SYS-01 | Edit settings (BR-SYS-03). | admin screen | | | `app_settings` | | product decision | AP-08 |
| OP-SYS-02 | Edit document templates (letters, certificates, policies) (D-18). | admin screen | | | `document_templates` | | 25 print layouts with fixed text (D-18) | AP-08 |
| OP-SYS-03 | Seed synthetic demo data (CLI, not web). | CLI | | | all tables | | product decision (D-02) | AP-14 |
| OP-SYS-04 | Upload and download files (BR-SYS-04). | button | | | `stored_files` | | containers | AP-03 |

## Target ownership

| Rule/operation | Server-domain/query/job/adapter/client | Interface | Transaction/concurrency | Authorization | Audit | Semantic tests |
|---|---|---|---|---|---|---|
| OP-REG-03 (BR-REG-02) | server-domain | `POST /registrations/{id}/plan` (UI-06) | One transaction: delete items, insert items, update registration | AP-03, AP-09 | audit_log `registration.plan_changed` | T-07-01 |
| OP-REG-06 (BR-REG-03) | server-domain | `POST /registrations/{id}/guests`, `PATCH /registrations/{id}/guests/{guest_id}`, `DELETE /registrations/{id}/guests/{guest_id}` (UI-06) | One transaction: guest change + qty update of items with `qty_manual` = 0 (D-88) | AP-03, AP-06 | none | T-07-02 |
| BR-REG-04, -05, -08 | server-query (views) | read in registration page | read-only | AP-03 | none | T-07-03 |
| OP-REG-08 (BR-REG-09) | server-domain | `POST /payments/{id}/void` (UI-06) | single update | AP-05 | `payment.void` with reason | T-07-04 |
| OP-EVT-04 (BR-EVT-06) | server-domain | `POST /plans/{id}/build`; `PATCH /plans/{id}` on event change (UI-13) | one transaction; INSERT only missing pairs (UNIQUE) | AP-03 | none | T-07-05 |
| BR-EVT-04, -05 | server-query (views) | event page | read-only | AP-03 | none | T-07-06 |
| OP-STU-01/02 (BR-STU-01) | server-domain | `POST /students`, `POST /students/{id}/duplicate` (UI-15) | one transaction: student + certification rows | AP-03 | none | T-07-07 |
| BR-STU-03, BR-STU-06, BR-CRT-02..04 | server-query (views) | student page, reports | read-only | AP-03 | none | T-07-08, T-07-09 |
| OP-ENR-01..03 (BR-ENR-01, BR-CRS-01) | server-domain | `POST /students/{id}/enrollments/{kind}`; options `GET /calendar-options/{kind}` (UI-20) | single insert; price read in the same transaction | AP-03 | none | T-07-10 |
| BR-ENR-03..05 | server-domain (validation) + view | enrollment form | single update | AP-03 | none | T-07-11 |
| OP-ENR-07 (BR-ENR-06) | server-domain | `POST /grades/import` (upload, preview), `POST /grades/import/{token}/apply` (UI-22) | apply in one transaction; only matched serials | AP-09 | `grades.import` with row counts | T-07-12 |
| OP-ENR-05 (BR-ENR-08) | server-domain | `DELETE /enrollments/{kind}/{id}`, `POST /enrollments/{kind}/{id}/cancel` (UI-20) | single delete or update | AP-05 | `enrollment.delete` | T-07-13 |
| OP-MAIL-01 (BR-MAIL-07, -01, -02) | server-domain | `POST /calendar/{kind}/{id}/mail` (UI-18) | one transaction: batch + e-mails | AP-03 | none | T-07-14 |
| OP-MAIL-03/04 (BR-MAIL-03, -04, -06, -08) | external-adapter EXT-01 (Resend + outbox mock); JOB-01 for send all | `POST /mail/emails/{id}/send`, `POST /mail/batches/{id}/send-all` (UI-26) | per e-mail: send, then update the row; no transaction across the provider call | AP-03, AP-09 | `mail_outbox` row per send; `mail.send_batch` | T-07-15, T-07-16 |
| OP-RPT-01..05 | server-query + print-CSS template | `GET /print/convention/{doc}`, `GET /print/school/{doc}`, `GET /reports/{code}`, `GET /dashboard/enrollments`, `GET /exports/*` (UI-23, UI-24, UI-34, UI-35) | read-only | AP-10 | none | T-07-17 |
| OP-AUTH-01..04 | server-domain | `POST /login`, `POST /logout`, `POST /account/password`, `/admin/users` (UI-01, UI-29, UI-33) | single statements | AP-01, AP-08, AP-11, AP-12 | `auth.*` events | 08 negative tests |
| UI navigation, view switch, window sizing (appendix §1 client-ui rows) | client-ui | links, HTMX | none | route guard (AP-01) | none | none |

Persistent invariants stay server-owned. HTMX and the small JS layer only call these operations.

## Script dispositions

Full table: appendix §1 (212 rows). The rows below are the tier1 scripts with durable effects.

| Object file | Folder/name | Effects and evidence | Target owner/operation | Keep/replace/retire/unresolved | Dependencies/tests |
|---|---|---|---|---|---|
| `objects/MBA.fmp12/scripts/45-registrations.selectPackage-select.txt` | registrations / selectPackage-select | Delete All Records of the registration's items (step 10), create items (27-30), set plan and event (37-38) | OP-REG-03 | keep | script 44; T-07-01 |
| `objects/MBA.fmp12/scripts/38-registration_update_guests.txt` | registrations / update_guests | ticket_qty = guests + 1 on every item (step 9) | OP-REG-06 | keep | trigger on layout 29; T-07-02 |
| `objects/MBA.fmp12/scripts/50-package_select_event.txt` | packages / package_select_event | creates missing plan items (31-33) | OP-EVT-04 | keep | trigger on layout 40; T-07-05 |
| `objects/MBA.fmp12/scripts/15-event.save.txt` | events / save | Set Field on stored calculation fields (steps 8-12) | OP-EVT-02 + `v_event_totals` | replace | AMB-07-03; T-07-06 |
| `objects/MBA.fmp12/scripts/17-global_registerPackage_registration_id_package_id.txt`, `18-...` | global | create registration items by parameter; no caller | none | retire (D-22) | none |
| `objects/MBA.fmp12/scripts/95-registrations.createAccountingReport_lastTimestamp.txt` | registrations | Delete All Records in `virtual_table`, pivot build | OP-RPT-04 | replace (D-16) | T-07-17 |
| `objects/MBS.fmp12/scripts/33-student_new_record.txt`, `34-...`, `35-...` | students | create student + certification rows | OP-STU-01, OP-STU-02 | keep (changed: unique pair) | T-07-07 |
| `objects/MBS.fmp12/scripts/21-sase.updateStudentRecord.txt`, `167-...`, `18-sase.storeCalcs.txt` | sase | set flags, Replace Field Contents over the found set of flagged students (18 steps 8-9), log to `sci_admin` | views | retire (D-04) | T-07-08 |
| `objects/MBS.fmp12/scripts/128-enroll_course_import_registro_de_notas.txt` | enrollments | Import Records, Method Update, match on id | OP-ENR-07 | keep (changed: preview) | T-07-12 |
| `objects/MBS.fmp12/scripts/173-sciSendMai.caller_module.txt` | integration | student uuids of the calendar row -> MBS_SendMail script 1 | OP-MAIL-01 | keep | T-07-14 |
| `objects/MBS_SendMail.fmp12/scripts/1-g.createEmails_contact_ids.txt` | g | batch + e-mails | OP-MAIL-01 | keep | T-07-14 |
| `objects/MBS_SendMail.fmp12/scripts/8-email.send.txt`, `11-...`, `18-email.buttonAll.txt` | email | send through the plug-in, set sent / error | OP-MAIL-03, OP-MAIL-04 | replace (D-05) | T-07-15, T-07-16 |
| `objects/MBS_SendMail.fmp12/scripts/9-g.pluginRegister.txt`, `10-g.smtpConnect.txt` | g | plug-in licence and SMTP login; hold secrets (not copied) | none | retire (D-05) | none |

Retired groups (appendix §1): 15 SASE and log scripts (D-04), 16 scripts with no caller (D-22), 13 report drafts (D-17), 6 plug-in and SMTP scripts (D-05), 2 credential-passing login scripts (D-07, D-50), window and view-switch utilities.

## Calculations and custom functions

Full lists: appendix §2 (fields) and §3 (functions). The important ones:

| Object file and field id | Context/dependencies | Meaning | Target implementation/signature | Coercion/null/timezone rules | Fixtures |
|---|---|---|---|---|---|
| `objects/MBA.fmp12/tables/133-registration.txt` #14, #19, #29 | registration -> packages, registration_items, payments | total, payments, balance | `v_registration_totals` | missing values sum as 0; voided payments excluded | T-07-03 |
| `objects/MBA.fmp12/tables/135-registration_items.txt` #16 | own fields | line total | `v_registration_item_lines.total` | no_charge or plan item -> 0 | T-07-03 |
| `objects/MBA.fmp12/tables/136-packages.txt` #13 | package_items (active) | plan price | `v_package_price` | override wins even when 0 | T-07-06 |
| `objects/MBA.fmp12/tables/131-events.txt` #21-#31 | registration, sponsor | event totals | `v_event_totals` | as above | T-07-06 |
| `objects/MBS.fmp12/tables/139-enrollments_course.txt` #10, #11, #28, #82 | students, calendar | price, balance, letter, passed | stored `costo`; `v_enrollments_course_lines` | empty score -> no letter, not passed | T-07-10, T-07-11 |
| `objects/MBS.fmp12/tables/146-student_certifications.txt` #21-#38 | requirements, passed enrollments (r27, r28) | certification progress | `v_student_certification_progress` | ROUND half away from zero (FileMaker Round and SQLite ROUND agree) | T-07-09 |
| `objects/MBS.fmp12/tables/141-calendar_course.txt` #13-#18, sase_* | dates, enrollments | first date, quorum, needed, counts, revenue | `v_calendar_course_summary` | OD-04-01 (first date = MIN) | T-07-18 |
| `objects/MBS.fmp12/custom-functions/4-isCourse_valid.txt` | status | active calendar row | `is_active` in the calendar views; Rust `fn is_course_active(status: &str) -> bool` | exact match, case-sensitive | T-07-10 |
| `objects/MBS.fmp12/custom-functions/7-object.calendar.txt`, `13-sql.enrollmentsCount.txt`, `14-sql.enrollmentsRevenue.txt` | ExecuteSQL by table name | calendar statistics | views | none | T-07-18 |
| `objects/MBS.fmp12/custom-functions/11-solFM_fechaPalabras.txt`, `3-fromListToSentence.txt` | pure | Spanish date and list text | Rust `format_date_es_long`, `list_to_sentence_es` | empty -> empty | T-07-19 |
| `objects/MBS_SendMail.fmp12/tables/130-emails.txt` #4, #5 | batch, profile, student | rendered body, recipients | Rust `render_mail(batch, profile, email)`, `recipients(student)` | empty addresses dropped | T-07-15 |

Pseudo-JSON helpers (`json.code`, `JSON.this`, `sc.this`, `object.getGetPair`, `sc.getPair`, `sq`, `squo`) and the functions with no caller are retired (appendix §3). Twin functions in two files are written once.

## Validation and reference values

| Source rule/value list | Timing/override/stored-display semantics | Target DB/API/UI enforcement | Deliberate strengthening/change | Tests |
|---|---|---|---|---|
| `enrollments_course.Puntuacion` range 0-100, strict numeric (#57) | OnlyDuringDataEntry | CHECK (0-100) + form validation | DB check also covers imports | T-07-11 |
| `enrollments_course` `AUSENCIAS`, `TARDANZAS` strict numeric (#22, #27) | data entry | INTEGER column + form validation | none | T-07-11 |
| `enrollments_course."ID #:"` unique, not empty, serial (#38) | data entry | UNIQUE; app assigns next serial | NOT NULL left to the app | none |
| `mail_profiles.email` unique (#1) | data entry | PRIMARY KEY | none | T-07-20 |
| Value lists `course_statuses`, `course_types`, `event_status`, `event_item_types`, `registration_status`, `costeado_por`, `course_enrollment_paid_by`, `tipo_de_pago`, `categoria_auspiciador`, `irregular_course_statuses`, `course_calendar_day_type` | display only in source (no validation) | CHECK on the column + select options from `value_list_items` | strengthened (no legacy data, D-02) | T-07-21 |
| `progress.Certificación`, `Sesión` (seminar and short-course enrollments): validate against a list the DDR does not name | data entry | none: `progress` and `Sesión` are dropped | none | none |
| Field-based value lists (affiliates, courses, calendar dates, professors, profiles, events, sponsors, people.company) | related-only lists filter by relationship | queries that apply the same filter (BR-CRS-01 for calendar dates; same event for items) | none | T-07-10 |

## Presentation and workflow signals

| Layout file and object key | Condition | Effect (style or hide) | Domain/security cross-reference | Target behavior |
|---|---|---|---|---|
| `objects/MBA.fmp12/layouts/40-Planes.txt` key 5 | plan item active = 1 | style | BR-EVT-05 | active plan items styled; toggle (OP-EVT-05) |
| `objects/MBS_SendMail.fmp12/layouts/4-Emails.txt` key 65 (x3) | status Draft / Sent / Error | style | BR-MAIL-05 | status badge |
| `objects/MBS.fmp12/layouts/172-Print_Estudiante.txt` keys 188-256 (18 items) | student has no course / seminar / short-course enrollment | white text (hides labels or notes) | none | render the right note; no white text (D-39) |
| `objects/MBA.fmp12/layouts/103-virtual_table.txt` keys 12-49 (21 items) | header or sum row | style | BR-RPT-12 | retired with the pivot (D-16) |
| Object flags with high bits on buttons and portals (layouts MBA 98, MBS 43, 82, 103, 113, 172, MBS_SendMail 4) | unknown | unknown (hide or lock?) | none | treated as visible and active (D-27) |

No hide condition and no conditional format carries a security decision.

## Bulk and integration safety

| Source operation | Reconstructed scope/direction | Secret handling | Retry/idempotency | Target contract | Failure tests |
|---|---|---|---|---|---|
| Delete All Records, MBA script 45 step 10 | found set = `registration_items` where `registration_uuid = $$registration_uuid` (step 7 find) | none | rerun gives the same result (delete + rebuild) | `DELETE FROM registration_items WHERE registration_uuid = ?` inside the OP-REG-03 transaction | T-07-01 (other registrations untouched) |
| Delete All Records, MBA script 95 step 22 | whole `virtual_table` | none | n/a | retired (D-16) | none |
| Replace Field Contents, MBS script 18 steps 8-9 | found set = students with `need_update` = 1 | none | n/a | retired (D-04); views replace the flags | none |
| Import Records (update), MBS script 128 | target = `enrollments_course` rows matched on id | none | rerun with the same file gives the same values | parse XLSX, preview, update matched serials only, report unmatched | T-07-12 |
| Export Records, MBS script 127 | current found set of course enrollments | none | n/a | XLSX of the filtered list | T-07-17 |
| Send mail through the 360Works plug-in, MBS_SendMail scripts 8, 11, 18 | outbound SMTP | SMTP password and licence key sit in scripts 9 and 10 as clear text. They are not copied into any artifact and get no column and no config (D-05). Target: `RESEND_API_KEY` from the environment only. | send-all skips sent e-mails (BR-MAIL-06); a resend of one e-mail is a user action | EXT-01 adapter: Resend API, or `mail_outbox` mock without a key | T-07-15, T-07-16 |
| Perform Script in `sci_admin`, MBS script 30 | outbound to a file not exported | none | n/a | retired (D-04) | none |
| Cross-file call MBS 173 -> MBS_SendMail 1 | internal | none | n/a | internal module call (D-01) | T-07-14 |

No `Insert from URL` step exists (script explorer).

## Ambiguities and conflicts

| ID | Source behavior | Stakeholder description | Conflict/missing evidence | Risk | Required decision/proof |
|---|---|---|---|---|---|
| AMB-07-01 | MBA script 45 writes `$$registration_uuid` items; script 42 writes the event to the current record of the picker layout (D-31). | none | as-written defect | wrong record updated | Target writes only the registration (changed, D-31). Accepted for the demo. |
| AMB-07-02 | `registration_items.ticket_qty` auto-enter formula is inactive (constant 1); script 38 sets guests + 1. | none | which rule is intended | plan quantity lost | Build as script 38 does (BR-REG-03). |
| AMB-07-03 | MBA script 15 uses Set Field on `events` fields that are stored calculations (#22, #25, #27). The step text for `balance` sums `payment_total`; the field formula sums `balance`. | none | which formula the users see | event balance figure | Target follows the field formulas (BR-EVT-04). Inference: Set Field on a calculation field fails in FileMaker, so the formulas are what users see. |
| AMB-07-04 | `events.sponsor_total` = promised + received (#31). | none | a total of promised and received counts money twice | sponsor totals | Build as written; show promised and received too. Owner may change. |
| AMB-07-05 | `object.calendar` passes `seminar_enrollments` where the count function expects another name, so seminar counts show "?"; the short-course branch writes names the fields do not read. | none | counts broken in source | report figures | Target counts all three types the same way (D-13 default). |
| AMB-07-06 | `student_certifications.completed` counts passed enrollments, so a retaken course counts twice. | none | as-written defect | progress overstated | Target counts distinct courses (changed, D-31). Credits, hours and average keep the source sums. |
| AMB-07-07 | `email.buttonAll` sends every e-mail in the found set, sent or not. | none | D-20 | duplicate mail | Target skips sent e-mails (changed, D-20). |
| AMB-07-08 | Script 17 writes the token `<SCI_TITLE` without `>`. | none | D-20 | token not replaced | Target matches `<SCI_TITLE>`. |
| AMB-07-09 | Delete rules: FileMaker cascades from students and registrations to money rows; the MBS Borrar buttons always deny. | none | D-09 not answered | lost payments | Step 5 default (D-09): cancel or void money rows, admin-only parent deletes, calendar rows never deleted. |

## Completion gate

- [x] `catalog_contract.py verify` passes for every explorer (explorer reports, Step 3).
- [x] Every logic-bearing catalog row has a disposition (appendix §1-§6; 0 unmatched).
- [x] Every tier1 rule cites its object file and step number or field id.
- [x] Every tier1 rule names its semantic tests. Test list:

| Test id | Rule/operation | Case |
|---|---|---|
| T-07-01 | BR-REG-02 | plan change replaces only this registration's items; inactive plan items skipped; cancel keeps old items |
| T-07-02 | BR-REG-03 | add, edit, remove a guest -> every item with `qty_manual` = 0 has qty = guests + 1; zero guests -> 1; an item with qty set by hand (`qty_manual` = 1) keeps its qty (D-88); plan change -> all items back to guests + 1, `qty_manual` = 0 |
| T-07-03 | BR-REG-04, -05, -08 | with plan, without plan, no_charge item, voided payment, no items (total 0) |
| T-07-04 | BR-REG-09 | staff cannot void; admin void without reason refused; voided payment drops from balance |
| T-07-05 | BR-EVT-06 | no event -> message; event without items -> message; second run adds nothing |
| T-07-06 | BR-EVT-04, -05 | event totals with 0, 1, n registrations; override 0 vs empty |
| T-07-07 | BR-STU-01 | new and duplicated student get one row per certification; no duplicate pairs |
| T-07-08 | BR-STU-03 | Confirmado calendar -> ACTIVO; only Pendiente -> INACTIVO; no enrollments -> INACTIVO |
| T-07-09 | BR-CRT-02..04 | retaken course counts once; left never negative; graduation date sets graduated |
| T-07-10 | BR-ENR-01, BR-CRS-01 | affiliate ACTIVO -> member price; INACTIVO or none -> individual; Completado/Cancelado rows not offered |
| T-07-11 | BR-ENR-03..05 | score 101 refused; 69 not passed; 70 C passed; empty -> no letter |
| T-07-12 | BR-ENR-06 | unmatched serial not created; matched rows updated; preview changes nothing |
| T-07-13 | BR-ENR-08 | enrollment with payment cannot be deleted; without payment deleted after confirm |
| T-07-14 | BR-MAIL-07, -01, -02 | calendar with 0 enrollments -> no batch; n -> one batch, n e-mails; empty addresses dropped |
| T-07-15 | BR-MAIL-03, -04, -08 | tokens replaced; no key -> outbox mock row; provider error -> error flag and log on top |
| T-07-16 | BR-MAIL-06 | send-all skips sent; one failure does not stop the rest |
| T-07-17 | OP-RPT-01..04 | hotel filter on/off; Individuo vs Afiliado invoice sets; CSV columns |
| T-07-18 | BR-CRS-03, BR-RPT-02, -03 | counts and revenue per calendar row for all three offering types |
| T-07-19 | BR-RPT-08 | Spanish date and list text |
| T-07-20 | BR-MAIL-09 | duplicate profile e-mail refused |
| T-07-21 | CHECK enums | value outside the list refused by the database |

## Open decisions

| ID | Decision | Default for the demo | Affects |
|---|---|---|---|
| OD-07-01 | Keep the hotel-only filter on badges and letters as a default-on filter? (D-15) | yes, default on, user can turn it off | OP-RPT-01 |
| OD-07-02 | Sponsor total = promised + received (AMB-07-04) | as written | BR-SPN-01 |
| OD-07-03 | Event totals include cancelled registrations (OD-04-04) | as written | BR-EVT-04 |
| OD-07-04 | Delete rules (D-09, AMB-07-09) | Step 5 default | AP-04..AP-07 |
| OD-07-05 | Report and print scripts with no caller or `-0.1` names (D-17, D-22) | retired | OP-RPT-01..03 |
