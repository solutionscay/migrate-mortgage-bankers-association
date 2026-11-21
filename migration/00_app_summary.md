# Application Summary

Cite evidence as object file paths: `objects/<file>/<kind>/<id>-<name>.txt`.

Labels: **fact** = read from an object file or `index.tsv`; **inference** = drawn from names or structure, not yet confirmed by the explorers; **pending** = filled in Step 3.

## Source

| Item | Value |
|---|---|
| DDR export (folder, date, FileMaker version) | `filemaker-source/DDR` (MBA_fmp12.xml, MBS_fmp12.xml, MBS_SendMail_fmp12.xml, Summary.xml); DDR created 11/13/2026 by FileMaker 26.0.3 (fact, `objects/MBA.fmp12/file/0-options.txt` header) |
| Object files (`objects/index.tsv` rows) | 806 object files, 47,314 object lines (fact) |
| Separators skipped (`-` layouts and scripts, no object file) | separators-skipped: 59 (fact; Summary.xml has 200 layouts and 252 scripts, `index.tsv` has 181 and 212) |
| Extraction errors | none. All calculations shown (MBA 1019/1019, MBS 1237/1237, MBS_SendMail 93/93). `objects/unresolved.tsv` has 60 rows: none 39, marker 18, external 1, folder 1, missing 1. 6 markers in commented-out code skipped. |

## Application profile

| Attribute | Value | Evidence |
|---|---|---|
| Name and purpose | Mortgage Bankers Association back office in three files. MBA: conventions and events (people, events, registration, packages, payments, sponsors, billing). MBS: a school (affiliates, students, courses, seminars, short courses, certifications, enrollments, calendars, professors, emails). MBS_SendMail: an e-mail module (templates, emails, batches, profiles). (inference from table names) | `objects/*/tables/` |
| FileMaker files | 3: MBA.fmp12, MBS.fmp12, MBS_SendMail.fmp12 (fact) | `objects/*/file/0-options.txt` |
| Tables / table occurrences / relationships | 43 / 75 / 57 (MBA 14/26/23, MBS 24/41/28, MBS_SendMail 5/8/6) (fact) | `index.tsv` |
| Layouts | 181 (MBA 64, MBS 109, MBS_SendMail 8) (fact) | `index.tsv` |
| Scripts | 212 (MBA 69, MBS 126, MBS_SendMail 17) (fact) | `index.tsv` |
| Custom functions | 24 (MBA 7, MBS 16, MBS_SendMail 1) (fact) | `index.tsv` |
| Value lists | 36 (MBA 15, MBS 19, MBS_SendMail 2) (fact) | `index.tsv` |
| Privilege sets / accounts / extended privileges | 9 / 10 / 18 (3/3/6 per file; MBS_SendMail has 4 accounts) (fact) | `index.tsv` |
| Custom menus / menu sets | 111 / 15 (MBA 41/7, MBS 46/7, MBS_SendMail 24/1) (fact) | `index.tsv` |

### Kind counts (`tail -n +2 objects/index.tsv | cut -f1,2 | sort | uniq -c`)

| Kind | MBA.fmp12 | MBS.fmp12 | MBS_SendMail.fmp12 | Total | Object lines |
|---|---:|---:|---:|---:|---:|
| accounts | 3 | 3 | 4 | 10 | 50 |
| auth-files | 1 | 1 | 1 | 3 | 15 |
| base-directories | 2 | 2 | 2 | 6 | 36 |
| custom-functions | 7 | 16 | 1 | 24 | 899 |
| custom-menu-sets | 7 | 7 | 1 | 15 | 212 |
| custom-menus | 41 | 46 | 24 | 111 | 6,166 |
| data-sources | 0 | 2 | 1 | 3 | 15 |
| extended-privileges | 6 | 6 | 6 | 18 | 110 |
| file | 1 | 1 | 1 | 3 | 57 |
| layouts | 64 | 109 | 8 | 181 | 14,603 |
| privilege-sets | 3 | 3 | 3 | 9 | 81 |
| relationships | 23 | 28 | 6 | 57 | 913 |
| scripts | 69 | 126 | 17 | 212 | 12,810 |
| table-occurrences | 26 | 41 | 8 | 75 | 2,085 |
| tables | 14 | 24 | 5 | 43 | 8,867 |
| value-lists | 15 | 19 | 2 | 36 | 395 |
| **Total** | **283** | **434** | **89** | **806** | **47,314** |

Separators per file (Summary.xml minus `index.tsv`): MBA 8 layouts + 12 scripts, MBS 11 layouts + 25 scripts, MBS_SendMail 0 layouts + 3 scripts.

## Files and references

| File | References these files (data sources) | Unresolved data sources | Evidence |
|---|---|---|---|
| MBA.fmp12 | none. It has no data sources and no object refers to MBS or MBS_SendMail (fact) | none | `index.tsv` (no `MBA.fmp12/data-sources` rows) |
| MBS.fmp12 | `MBS_sendMail` (id=2, `file:MBS_SendMail`): used by `scripts/173-sciSendMai.caller_module.txt`, which calls script `g.createEmails ( contact_ids )` in MBS_SendMail.fmp12. `mbs_sase` (id=1, `file:sci_admin`): used by `scripts/30-sase.log_message.txt` (Perform Script) (fact) | `mbs_sase` → `sci_admin` was not exported. Script 30 calls `<unknown>` from it (`unresolved.tsv`, status `external`) | `objects/MBS.fmp12/data-sources/1-mbs_sase.txt`, `objects/MBS.fmp12/data-sources/2-MBS_sendMail.txt` |
| MBS_SendMail.fmp12 | `mbs` (id=1, `file:MBS`): table occurrence `students` points to MBS base table `students` (id 131) (fact) | none | `objects/MBS_SendMail.fmp12/data-sources/1-mbs.txt`, `objects/MBS_SendMail.fmp12/table-occurrences/1065092-students.txt` |

MBS and MBS_SendMail reference each other. MBA stands alone. `sci_admin` is a fourth file outside the export.

## Explorer coverage

| Explorer | Kinds | Objects | `catalog_contract.py verify` |
|---|---|---:|---|
| Scripts | scripts | 212 | pending (Step 3) |
| Custom functions | custom-functions | 24 | pending (Step 3) |
| Tables | tables | 43 | pending (Step 3) |
| Layouts | layouts | 181 | pending (Step 3) |
| Graph | table-occurrences, relationships | 132 | pending (Step 3) |
| Security | privilege-sets, accounts, extended-privileges | 37 | pending (Step 3) |
| Value lists | value-lists | 36 | pending (Step 3) |
| Menus | custom-menus, custom-menu-sets | 126 | pending (Step 3) |
| File | file, data-sources, auth-files, base-directories | 15 | pending (Step 3) |

Every kind in `objects/index.tsv` appears in one row. The solution has no kinds beyond these 16. Catalogs: `explorers/<name>/catalog.json`. Assignments: `explorers/assignments/agent-0[1-5]-*.tsv`.

## Data model

Field counts and key status come from the table explorer (Step 3).

| File | Base table | Role | Regular | Calculated | Summary | Globals | Containers | Key status | Evidence |
|---|---|---|---:|---:|---:|---:|---:|---|---|
| MBA.fmp12 | MBA, people, events, event_items, registration, registration_guests, registration_items, packages, package_items, payments, globals, sponsor, billing, virtual_table | pending | | | | | | pending | `objects/MBA.fmp12/tables/` |
| MBS.fmp12 | affiliates, students, courses, seminars, progress, short_courses, certifications, certification_requirements, enrollments_seminars, enrollments_course, enrollments_short_course, calendar_course, calendar_course_dates, calendar_short_course, calendar_seminar, globals, student_certifications, emails, email_templates, virtual_table, professors, stat_daily, serverSettings, serverLog | pending | | | | | | pending | `objects/MBS.fmp12/tables/` |
| MBS_SendMail.fmp12 | templates, emails, globals, batches, profiles | pending | | | | | | pending | `objects/MBS_SendMail.fmp12/tables/` |

Known gap (fact): MBS table occurrence `MBS` (id 1065089) points to base table id 129, which does not exist in MBS (`unresolved.tsv`, status `missing`; `objects/MBS.fmp12/table-occurrences/1065089-MBS.txt`).

### Relationship semantics

Pending (graph explorer, Step 3).

### Globals, containers, time, and validation

- Globals by purpose: pending (table explorer). Each file has a `globals` table (fact).
- Container fields and where they store data: count pending. Base directories: MBA `MBA/`, MBS `MBS/`, MBS_SendMail `Files/Send Emails/` (fact, `objects/*/base-directories/`).
- Source timezone for Date/Time/TimeStamp values: unresolved.
- Fields with validation or auto-enter: pending (table explorer).

## Functional and logic map

Pending (Step 3). Each file has an OnFirstWindowOpen script; MBA runs `global_start` (fact, `objects/MBA.fmp12/file/0-options.txt`).

## Security model

| Dimension | Count | Evidence | Notes |
|---|---:|---|---|
| Accounts and provisioning | 10 | `objects/<file>/accounts/...` | The DDR has no passwords or hashes |
| Privilege sets | 9 | `objects/<file>/privilege-sets/...` | |
| Record-level rules | pending | | |
| Field restrictions | pending | | |
| Layout / script / value-list access | pending | | |
| Extended privileges (access channels) | 18 | `objects/<file>/extended-privileges/...` | |
| Hide and conditional-format rules | pending | layout explorer | UI evidence only |

Each file's auth-file catalog has `requireAuthorization=False` (fact, `objects/*/auth-files/`).

## Open decisions

| Decision | Evidence | Impact | Owner |
|---|---|---|---|
| Is `sci_admin` (data source `mbs_sase`) still in use, and can it be exported? | `objects/MBS.fmp12/data-sources/1-mbs_sase.txt`, `objects/MBS.fmp12/scripts/30-sase.log_message.txt` | Logging behavior of the `sase` script folder is unknown | client |
| Are MBA and MBS one product or two? They share no data source. | `index.tsv` | Scope and order of increments | client |
| What did MBS table occurrence `MBS` (missing base table 129) point to? | `objects/MBS.fmp12/table-occurrences/1065089-MBS.txt` | Possible dead graph branch | client |
