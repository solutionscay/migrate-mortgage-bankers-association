# Plan Audit (Step 6)

Date: 2026-11-21. Reader: a second agent that did not write `07` (task 77752b02). Method: for each rule in `07_business_logic.md`, open the object files that the rule cites and compare the rule text with the steps, field formulas, value lists and relationships. The catalogs and `07` were not used as evidence.

Scope: demo build (`decisions.md`). Prod-only gaps are marked "out of scope (demo)" and stay listed.

## Summary

| Measure | Value |
|---|---|
| BR rules in `07` | 79 |
| Rules checked against object files (tier1: rules with object evidence) | 75 |
| Rules with product-decision evidence only (citation checked, no object to re-read) | 4: BR-AUTH-01, BR-AUTH-03, BR-SYS-03, BR-SYS-04 |
| Rules fixed | 10 |
| Error rate | 10 / 75 = 13.3% |
| Restatements fixed in other artifacts | 10 (04: 2, 05: 3, 06: 4, 07 appendix: 1) |
| Gaps listed | 95 entries (60 rows of `objects/unresolved.tsv` in 11 groups) |

Tier1 here means every BR rule whose evidence is an object file. `07` has no tier column on its rules (see "Skill problems").

## Rules checked

All rules below were re-read against their object files. "ok" means the rule matches the object.

| Domain | Rules | Result |
|---|---|---|
| REG | BR-REG-01 .. BR-REG-11 | fixed: 01, 06. ok: 02, 03, 04, 05, 07, 08, 09, 10, 11 |
| EVT | BR-EVT-01 .. BR-EVT-07 | fixed: 01. ok: 02 .. 07 |
| SPN | BR-SPN-01, BR-SPN-02 | ok |
| PPL | BR-PPL-01, BR-PPL-02 | ok |
| AFF | BR-AFF-01, BR-AFF-02 | ok |
| STU | BR-STU-01 .. BR-STU-07 | fixed: 02. ok: 01, 03 .. 07 |
| CRS | BR-CRS-01 .. BR-CRS-06 | ok |
| ENR | BR-ENR-01 .. BR-ENR-09 | fixed: 06. ok: 01 .. 05, 07, 08, 09 |
| CRT | BR-CRT-01 .. BR-CRT-04 | ok |
| MAIL | BR-MAIL-01 .. BR-MAIL-09 | fixed: 05. ok: 01 .. 04, 06 .. 09 |
| RPT | BR-RPT-01 .. BR-RPT-12 | fixed: 01, 07, 10, 12. ok: 02 .. 06, 08, 09, 11 |
| AUTH, SYS | BR-AUTH-02, BR-AUTH-04, BR-SYS-01, BR-SYS-02 | ok (BR-AUTH-02 is a product decision on top of `fmreauthenticate10`; see OD-08-04) |

Checks that confirmed a rule, for the record: cascade deletes in MBA relationships 4, 5, 7 and MBS 2, 18, 25, 30 (BR-REG-09, BR-STU-07); `applicable_certifications` holds only required-course certifications, so BR-CRT-02 "passed required courses" is right; every MBS `Borrar` button passes access 0 (BR-CRS-05); the statistics scripts 121, 123, 124 find STATUS "activo" or "baja" (BR-RPT-02).

## Rules fixed

Each fix is in `07_business_logic.md` with the same id and the text "Audit fix: ...".

| Rule | Old text (07) | New text (07) | Evidence |
|---|---|---|---|
| BR-REG-01 | Event picked only from Activo events. No active event: message "no active events" (step 4-6). | Adds: the event is picked once; when the registration has an event, the picker does not open. Message text and steps corrected. | `objects/MBA.fmp12/scripts/41-registrations.selectEvent-open.txt` steps 3-4 (`If not IsEmpty(registration::event_uuid)` → Halt), steps 12-15 (message "Al momento no hay eventos activos.") |
| BR-REG-06 | "Dialog cancel keeps the old values." | The dialog has only OK; the script always writes the values back. Target adds Cancel (changed). | `objects/MBA.fmp12/scripts/37-registration_items_edit_attributes.txt` steps 10 and 21 (one button, "OK"), steps 12-13 and 23 (unconditional Set Field) |
| BR-EVT-01 | End date defaults to start date when empty (events and event items). | Events: when empty. Event items: always equals start date. | `objects/MBA.fmp12/tables/131-events.txt` #15 (`If(IsEmpty(date_end); date_start; date_end)`); `objects/MBA.fmp12/tables/132-event_items.txt` #15 (`date_start`, overwrite) |
| BR-STU-02 | Picking an affiliate sets the affiliate link and company name. | Source: typed `COMPAÑIA` finds the affiliate by `Socio`; `member_uuid` auto-enters its uuid. `COMPAÑIA` formula unavailable. Target: pick by key (changed, D-11). | `objects/MBS.fmp12/tables/131-students.txt` #35 (`get_affiliate_uuid::uuid`), #12 ("no formula in DDR"); `objects/MBS.fmp12/relationships/17-students_get_affiliate_uuid.txt` (`COMPAÑIA = Socio`) |
| BR-ENR-06 | Import matched on "the enrollment serial"; input "XLSX file". | Match on `id` (#61, target `serial`), not `ID #:` (#38). Auto-enter runs, so `Puntuacion`, `NOTA`, `is_passed` follow the imported final average. File type not in the DDR. | `objects/MBS.fmp12/scripts/128-enroll_course_import_registro_de_notas.txt` step 1 (`match with course_enrollments::id`, `AutoEnter=True`, `CharacterSet=Macintosh`, no file type); `objects/MBS.fmp12/tables/139-enrollments_course.txt` #61, #38, #57 |
| BR-MAIL-05 | Batch text "n enviados m errores" or "Nuevo". | "n enviado(s)" plus " m error(es)" when errors > 0; "Nuevo" when none is sent, also with errors. Target shows "0 enviados m errores" (changed). | `objects/MBS_SendMail.fmp12/tables/132-batches.txt` #22 (`If(b > 0; ...; "Nuevo")`) |
| BR-RPT-01 | No records: message. | No records: the print window closes with no message. Target shows a message page (changed). | `objects/MBA.fmp12/scripts/60-events.print_Registro_Individuos.txt` and `61-events.print_Registro_Afiliados.txt` steps 12-15 (Close Window, Exit Script) |
| BR-RPT-07 | Certification candidates: "courses left = n". | `left` = n (the count), not the course-name list. | `objects/MBS.fmp12/scripts/92-reports.STU.certificationCandidates.txt` step 12 (`student_certifications::left`); `objects/MBS.fmp12/tables/146-student_certifications.txt` #24 `left`, #38 `courses_left` (text list) |
| BR-RPT-10 | Labels and raffle tickets print the quantity the user enters. | Script 81 finds 1..qty. Scripts 82 and 85 find number = qty, so they show one row (inference: defect). Target prints 1..qty (changed). | `objects/MBA.fmp12/scripts/81-events.print_labels_botellas.txt` step 14 (`"1..." & qty`); `82-events.print_label_cajas.txt` and `85-print_boletos_rifa_exhibidores.txt` step 14 (`qty`) |
| BR-RPT-12 | One CSV row per registration with tickets and total per event item, plus a total row. | Adds: only user-picked event items are columns (`g_events`); a flag includes or leaves out plan items (`g_boolean`); the source total row sums at most 10 columns. | `objects/MBA.fmp12/scripts/95-registrations.createAccountingReport_lastTimestamp.txt` steps 9, 53-61, 72, 124-139 |

Restatements fixed in other artifacts (`03_migration_plan.md` was not edited):

| File | Place | Fix |
|---|---|---|
| `04_database_schema.sql` | `event_items.date_end` comment | "always equals date_start" (BR-EVT-01) |
| `04_database_schema_appendix.md` | batches #22 `sent_display` row | new BR-MAIL-05 text |
| `07_business_logic_appendix.md` | batches #22 `sent_display` row | new BR-MAIL-05 text |
| `05_api_design.md` | `GET /registrations/{id}/event-picker` | not offered when an event is set; message text (BR-REG-01) |
| `05_api_design.md` | `GET /exports/accounting.csv` | `items` and `plan_items` parameters (BR-RPT-12) |
| `05_api_design.md` | OD-05-03 | the source import gives no file type (BR-ENR-06) |
| `06_ui_spec.md` | UI-06 field "Evento" | button hidden once an event is set (BR-REG-01) |
| `06_ui_spec.md` | UI-26 batch list summary text | new BR-MAIL-05 text |
| `06_ui_spec.md` | report criteria form | "number of courses left `left`" (BR-RPT-07) |
| `06_ui_spec.md` | print "no records" state | source closes the window with no message in some scripts (BR-RPT-01) |

Not counted as rule errors, noted for the owner:

- The catalog rows in `07_business_logic_appendix.md` for scripts 60, 61, 82 and 128 were right. The errors came in when `07` summarized them.
- 03 was not checked for restatements of the fixed rules, because another pane edits it. Check I-rows that name BR-REG-01, BR-RPT-10, BR-RPT-12 or BR-ENR-06 after that pane finishes.

## Gaps: what the plan still does not understand

Area = the part of the plan the gap affects. The operator decides which gaps matter. A gap with a default stays listed and does not block the plan.

### Open discovery questions (label `unavailable`, default assumed)

| Gap | Question (short) | Default in force | Area |
|---|---|---|---|
| D-08 | Two roles enough, no read-only role | ROLE-ADMIN, ROLE-STAFF | 08 roles (OD-08-01) |
| D-09 | Who may delete what | cancel money rows; admin-only parent delete; calendar rows never deleted | 08 AP-04..AP-07; 04 FK rules; 07 BR-REG-09, BR-STU-07, BR-ENR-08 |
| D-11 | Name-match links or keys | keys. The default text also says "migrate stored uuids" and "match by name during migration": stale under D-02 | 04 `students.member_uuid`; 07 BR-STU-02 |
| D-13 | Meaning of enrollment counts and revenue | each status its own count; revenue = sum of PAGO by payer | 07 BR-RPT-02, BR-RPT-03; 06 UI-24 |
| D-14 | Price rules | stored at creation; member status at that moment | 07 BR-ENR-01, BR-CRS-04 |
| D-15 | Registration rules (is_convencionista, guests, hotel filter) | build as written; filter can be turned off | 07 BR-REG-07, BR-REG-11, BR-RPT-04 |
| D-16 | Accounting report still used | CSV export. Default also says "migrate billing files if the container export holds any": stale under D-02 | 05 `/exports/accounting.csv`; 07 BR-RPT-12 |
| D-17 | Which reports and charts are in use | rebuild reachable reports; one dashboard | 06 UI-23, UI-24; 07 OP-RPT-01..05 |
| D-18 | Which letter texts are editable | templates with merge fields | 06 OP-SYS-02 screens; 04 `document_templates` |
| D-19 | Admin screens for reference data | one admin screen each | 06 UI-32 and admin screens |
| D-20 | E-mail behavior | send-all skips sent; template per batch | 07 BR-MAIL-01, BR-MAIL-06 |
| D-21 | 34 layouts with no caller | drop | 06 appendix |
| D-22 | 32 scripts with no caller | drop | 07 appendix §1 |
| D-23 | Grades round trip | XLSX export and import with preview | 05 grades routes; 07 BR-ENR-06, BR-ENR-07 |
| D-24 | Accounting system that feeds `affiliates` | no import | 05 (no EXT); 04 `affiliates` |
| D-25 | CSBS CE export still filed | CSV; constants are settings | 05 export; 07 BR-RPT-06 |
| D-26 | Repeating fields to child tables | child tables. Default also says "drop `other_requirements` if the profile shows it empty": profiling is out of scope (demo) | 04 child tables |
| D-27 | High-bit object flags | visible and active | 06 UI-15, UI-20, UI-26; 08 (OD-06-05, OD-08-03) |
| D-28 | Enum domains | one paid-by enum per module labels | 04 CHECK enums |
| D-29 | Drop MBS `emails`, `email_templates` | drop | 04 scope |
| D-31 | Fix defects or build as written | fix, list each change in 07 | 07 every "Changed" note, incl. 4 new ones from this audit |
| D-32 | Deleted MBA "settings" script | drop the button | 06 MBA screens (see U1) |
| D-33 | Where report criteria live | filter form per request | 06 report forms |
| D-34 | Label quantity | user enters a quantity | 07 BR-RPT-10 |
| D-35 | Menu sets `sponsors`, `courses` | drop | 06 sponsor tab |
| D-36 | Orphan default objects | drop | 04 scope (see U6) |
| D-37 | Dead references | drop | 07 notes (see U4) |
| D-38 | Unused graph objects and functions | drop | 04, 07 appendix §3 |
| D-39 | Display quirks | show intended data | 06 screen specs |
| D-40 | External access channel | none; 10-minute idle timeout | 08 BR-AUTH-02 (OD-08-04) |
| D-41 | Standard menu commands removed | ignore | 06 (no impact) |
| D-42 | Explorer notes "as written" | build as written | 07 |

### Other unanswered or default-only discovery items

| Gap | Topic | Default in force | Area |
|---|---|---|---|
| D-46 | Retire / simplify / preserve list | default list | 03 scope |
| D-49 | Users and concurrency | fewer than 10 staff, fewer than 5 at once | 08; 02 sizing |
| D-54 | Record, field, operation exceptions | delete (D-09) only | 08 |
| D-72 | Device targets | desktop browser, Spanish UI | 06 |
| D-73 | Most-used screens | inferred from start layouts; 13 screenshots not mapped to layouts | 06 |
| D-74 | Density and navigation | list / form / table per area | 06 |
| D-75 | Accessibility | WCAG 2.2 AA | 06 |
| D-77 | Pain points | none known | 06 |

### Open decisions in the artifacts

Several OD rows repeat a D-question; they stay listed so each artifact owner sees them.

| Gap | Decision (short) | Default | Area |
|---|---|---|---|
| OD-02-01 | Host for a shared demo | out of scope (demo) (D-80) | 02 |
| OD-02-02 | Port karbonfm CSS or new CSS | decided in M1 | 02 |
| OD-03-01 | Seed sizes match mockup counts | yes | 03 |
| OD-03-02 | Design pages in plain CSS | yes | 03 |
| OD-03-03 | Order of I3 and I4 | e-mail after enrollments | 03 |
| OD-03-04 | UI-24, UI-32 built while D-17, D-19 open | build on defaults | 03 |
| OD-03-05 | Increment order M1, M2, then I1..I5 | as stated | 03 |
| OD-04-01 | Calendar first date = MIN(date) | MIN | 04 `v_calendar_*_summary` |
| OD-04-02 | Enrollment status has no value list | ACTIVO, BAJA | 04 enum; 07 BR-ENR-09 |
| OD-04-03 | "Nuevo" not in `registration_status` | kept | 04; 07 BR-REG-10 |
| OD-04-04 | Event totals count cancelled registrations | as written | 04 `v_event_totals`; 07 BR-EVT-04 |
| OD-04-05 | Fields with no use dropped | dropped | 04 appendix |
| OD-05-01 | One `{kind}` route family | yes | 05 CRS, ENR routes |
| OD-05-02 | Send-all as job with polling | JOB-01 | 05; 06 UI-26 |
| OD-05-03 | Grades file format | XLSX in and out | 05; 07 BR-ENR-06 |
| OD-05-04 | Server-side PDF | no | 05 |
| OD-05-05 | Registration header edit | reuse OP-REG-01 form | 05 |
| OD-06-01 | UI language | Spanish | 06 |
| OD-06-02 | One offering page for three kinds | yes | 06 UI-18..UI-20 |
| OD-06-03 | Dashboard content | counts and revenue | 06 UI-24 |
| OD-06-04 | Door check-in screen | UI-07 | 06 |
| OD-06-05 | Object-flag controls | visible and active | 06 |
| OD-07-01 | Hotel-only filter default on | yes | 07 OP-RPT-01 |
| OD-07-02 | Sponsor total = promised + received | as written | 07 BR-SPN-01 |
| OD-07-03 | Event totals include cancelled | as written | 07 BR-EVT-04 |
| OD-07-04 | Delete rules | Step 5 default | 07; 08 AP-04..AP-07 |
| OD-07-05 | Drafts and no-caller reports | retired | 07 OP-RPT-01..03 |
| OD-08-01 | D-08 roles | two roles | 08 |
| OD-08-02 | D-09 delete rules | default | 08 |
| OD-08-03 | Object flags | visible | 08 |
| OD-08-04 | 10-minute idle timeout from `fmreauthenticate10` (a background re-login in FileMaker, not an idle timeout) | 10 minutes | 08 BR-AUTH-02 |
| OD-08-05 | Lockout 5 failures / 15 minutes | as stated | 08 BR-AUTH-01 |

### Unresolved targets

No artifact row carries the disposition `unresolved` (07 appendix §1-§6, 06 appendix). The table occurrence `MBS` with a missing base table is retired (U6).

### Rows of `objects/unresolved.tsv` (60 rows, 11 groups)

| Gap | Rows | What | Object files | Area |
|---|---:|---|---|---|
| U1 | 12 | Buttons run script id 0 (the deleted "settings" script) | MBA layouts 26, 28, 30, 31, 36, 37, 38, 40, 41, 44, 98, 99 | 06 MBA screens; D-32 (button dropped) |
| U2 | 13 | `<Function Missing>`: 360Works Email plug-in calls not installed at DDR time | MBS_SendMail scripts 8 (10 rows), 9, 10, 12 | 05 EXT-01; 07 BR-MAIL-04, BR-MAIL-08 (D-05) |
| U3a | 9 | Field id 0: empty input slots of Show Custom Dialog | MBA scripts 37, 81, 82, 85 | 07 BR-REG-06, BR-RPT-10 (no impact) |
| U3b | 2 | Field id 0 in a dialog | MBS script 95 | 07 BR-RPT-07 (no impact) |
| U3c | 12 | Field id 0 in chart and draft scripts | MBS scripts 77, 78, 125, 144, 145, 169 | 06 UI-24 dashboard replaces them (D-17) |
| U4a | 2 | Go to Layout `<unknown>` (deleted layout) | MBS script 95 `students_graduate_history` | 06 graduate-history report: target layout unknown; 07 BR-RPT-07 |
| U4b | 2 | Go to Layout `<unknown>` | MBS script 120 `enroll_course_print_matricula_x_curso` | 06 OP-RPT-02 print page: layout content unknown |
| U4c | 4 | Go to Layout `<unknown>` in drafts | MBA script 77 (-0.1), MBS script 148 (-0.1) | 07 retired (D-17, D-22, D-37) |
| U5 | 2 | Perform Script in external file `sci_admin` (not exported) | MBS script 30 `sase.log` | 07 retired (D-04) |
| U6 | 1 | Table occurrence points to missing base table 129 | `MBS.fmp12/table-occurrences/1065089-MBS.txt` | 04 scope; retired (D-36) |
| U7 | 1 | Layout folder reference "Registro" | MBA layout 32 `Planes Activos` | 06 (no impact) |

### Out of scope (demo)

These gaps exist for production. They stay listed.

| Gap | Topic | Area |
|---|---|---|
| D-47 | Deadline, budget, parallel operation | 03 |
| D-48 | Rollback, outage, data loss | 03 |
| D-53 | Approval workflow for role grants | 08 |
| D-64 | Source record and container volumes | 03 data migration |
| D-65 | Keys, duplicates, orphans in source data | 03 data migration |
| D-66 | Retention policy | 04 |
| D-70 | Rejected-row correction | 03 data migration |
| D-71 | Parallel-write reconciliation | 03 cutover |
| D-80 | Hosting, network, data residency (also OD-02-01) | 02 |
| D-81 | CI/CD, observability, backup | 02 |
| D-82 | Security and compliance review | 08 |
| D-84 | Map FileMaker accounts (`inieves` role differs per file) | 08 |

### Gap count by plan area

| Area | Open D | Other D | OD | unresolved.tsv groups (rows) | Out of scope (demo) | Total entries |
|---|---:|---:|---:|---:|---:|---:|
| 02 stack | 0 | 0 | 1 | 0 | 3 (D-80, D-81, OD-02-01) | 4 |
| 03 plan | 0 | 1 | 5 | 0 | 6 (D-47, D-48, D-64, D-65, D-70, D-71) | 12 |
| 04 schema | 6 | 0 | 5 | 1 (1) | 1 (D-66) | 13 |
| 05 API | 4 | 0 | 5 | 1 (13) | 0 | 10 |
| 06 UI | 10 | 5 | 5 | 5 (29) | 0 | 25 |
| 07 logic | 9 | 0 | 5 | 4 (17) | 0 | 18 |
| 08 auth | 3 | 2 | 5 | 0 | 3 (D-53, D-82, D-84) | 13 |
| Total | 32 | 8 | 31 | 11 (60) | 13 | 95 |

Rows by group: U1 12, U2 13, U3a 9, U3b 2, U3c 12, U4a 2, U4b 2, U4c 4, U5 2, U6 1, U7 1 = 60. OD-02-01 is counted once, under out of scope (demo).

## Open item from part 2: SMTP host

`explorers/scripts/csv/scripts/MBS_SendMail.fmp12--1fef2279/_root-001.csv`, row of script 10 (`g.smtpConnect`): the host text in `purpose` and `external` is now "SMTP host in script text (redacted)". Nothing else changed. The row spans two physical lines in the CSV (a multi-line cell); both copies were replaced. A search of every file under `migration/` outside `objects/` finds no copy of the host or the other dotted string in script 10.

```
$ python3 $SKILL_DIR/scripts/catalog_contract.py verify --catalog explorers/scripts/catalog.json --csv-root explorers/scripts/csv
catalog coverage verified: 212 objects exactly once
```

## Skill problems

- Step 6 says "tier1 rule", but the `07` template has no tier on rules. Tier is a column of the script catalog. This audit treated every BR rule with object evidence as tier1. The template should carry a tier, or Step 6 should say "every BR rule that cites an object file".
- `explode_ddr.py` writes empty dialog input slots (Field id 0) to `objects/unresolved.tsv` with status `none`. 23 of 60 rows here are that noise. Filter them, or give them their own status.
- Explorer CSVs hold multi-line cells. A line-based edit (sed, grep -v) misses the second line of a row. The SMTP host sat in both lines of one row. Step 3 could tell explorers to keep cells on one line.
- `catalog_contract.py verify` checks coverage only. It does not look for secrets in cells. A secret scan against the object files that hold secrets (here MBS_SendMail scripts 9, 10) would have caught the host and the licence fragment (D-87) at Step 3.
