# Technology and Architecture Recommendation

Scope: a demo rebuild of three FileMaker files (MBA convention, MBS school, MBS_SendMail bulk e-mail) as one web app with one SQLite database (D-01, D-43). It follows the shape of migrate-karbonfm (`app/server`: axum + askama + rusqlite), without Tauri: a web server, not a desktop app. The stack is a product decision of the operator (2026-11-21, `decisions.md`). This file checks it against the constraints and records the rejected alternative.

Labels: **fact** (repository, object file or measured source), **product decision** (operator answer or D-id), **inference**.

## Decision basis

No stakeholder weights were given. The operator fixed the stack, so it is a hard gate; the other rows are checked for fit only.

| Constraint/requirement | Weight / hard gate | Evidence | Primary fit (Rust axum + askama + HTMX + SQLite) | Alternative fit (Ruby on Rails + Hotwire + SQLite) |
|---|---|---|---|---|
| Existing repository compatibility | n/a | No target app repository exists. This repository holds only `mockups/` and `migration/` (git status, 2026-11-21). | n/a | n/a |
| Operator-fixed stack | hard gate | `decisions.md` (2026-11-21) | pass | fail (not the chosen stack) |
| Demo, not production | hard gate | `decisions.md`; D-02, D-44 | pass: one binary, one SQLite file, a seed tool | pass |
| Reuse of the karbonfm base | strong | `decisions.md` ("the same base as migrate-karbonfm's app/server") | pass: same crates, same route and template pattern, same command layer shape | fail: nothing to reuse |
| Simplicity | strong | operator reason for rejecting Rails (task brief, 2026-11-21) | one language for rules, routes and templates; no ORM, no asset pipeline, no JS build; HTMX covers the partial updates | more framework surface: ActiveRecord, Turbo, Stimulus, asset pipeline, generators |
| Authorization coverage | unweighted | `08_auth_roles.md` AP-01 to AP-14 | strong: every write goes through a typed handler with a guard and one domain command; the client gets HTML only | strong: before_action filters and policies; same layering |
| Client/offline/accessibility needs | unweighted | D-72, D-75 not answered (Step 5 defaults: desktop browser, WCAG 2.2 AA on meaty screens) | semantic HTML, native `<dialog>`, server-rendered tables; no offline need | same |
| Data/reporting/integration needs | unweighted | `04` (41 tables, 11 views, money as integer cents), `07` (79 rules), `05` (EXT-01 Resend, 72 print pages) | rusqlite with plain SQL and the views of `04`; print-CSS pages from askama; reqwest for Resend; calamine and rust_xlsxwriter for the grades round trip | ActiveRecord over SQLite; Resend gem; caxlsx; same print approach |
| Deployment/operations/compliance | unweighted | out of scope (demo): no hosting study, no secret rotation (`decisions.md`) | one static binary + one database file + `data/files/` | Ruby runtime + gems + database file |
| Reversal/migration cost | unweighted | demo | low: the domain layer (07) is plain Rust functions over SQL; templates and routes are thin | low for a demo |

## Recommendation

Confirms `04` (SQLite dialect, WAL, foreign keys on, money as integer cents; `04_database_schema.sql` header and conventions) and `05` (server-rendered HTML routes with HTMX partials; `05_api_design.md` "Route conventions"). No choice in `04` or `05` changes.

| Layer | Primary | Alternative | Evidence-based rationale / tradeoff |
|---|---|---|---|
| Architecture/deployment unit | One Rust binary `mba-server` (axum on tokio) serving HTML on one port; one SQLite file; uploaded files under `data/files/` | Rails monolith | product decision; one process, one writer to SQLite |
| Runtime/framework | Rust stable; axum. Crate layout as karbonfm `app/server`: `domain/<domain>.rs` (rules and operations of `07`), `routes/<domain>.rs` (handlers of `05`), `templates/` (askama) | Ruby on Rails | type-checked handlers, templates and SQL row mapping; no runtime reflection |
| UI/rendering/client | askama templates compiled into the binary; HTMX (vendored, pinned) for partial updates and out-of-band swaps; `app.js` (< 200 lines, vanilla) for dialogs, focus and keyboard; one CSS file, no CSS framework build | Hotwire (Turbo + Stimulus) | `decisions.md`: pure SSR, HTMX, small vanilla JS, no SPA and no JS build step. HTMX covers every partial update the screens of `06` need (portals, tabs, pickers, totals) |
| Database/data access | SQLite 3.40+ through rusqlite (bundled); plain SQL, no ORM; `PRAGMA journal_mode = WAL`, `foreign_keys = ON`, `busy_timeout = 5000`; schema = `04_database_schema.sql` as migration 0001 | ActiveRecord | `04`; one file, one writer, live views replace stored snapshots (D-03, D-04) |
| API/server-operation style | HTML routes: full page or fragment by `HX-Request` (05); no JSON API | JSON API + SPA | the only client is the app's own HTML; AP-14 says no external channel |
| Background jobs/events | In-process tokio tasks: JOB-01 (send all) and JOB-02 (session and token cleanup); no queue, no scheduler service | Active Job + Solid Queue | two small jobs; the DDR has no server schedule (05 "Background jobs") |
| File/container storage | Files on disk under `data/files/<sha256>`, metadata in `stored_files` (BR-SYS-04) | BLOBs in SQLite | small database; dedupe by hash |
| Authentication/identity provider | Local: username + argon2id hash (`argon2` crate), server session table, cookie `HttpOnly; Secure; SameSite=Lax`, CSRF token per session (D-50; AP-01, AP-02, AP-11) | Devise | operator answer; no SSO, no MFA |
| Hosting/compliance | out of scope (demo). Runs on localhost or one small VM behind HTTPS | n/a | `decisions.md` |
| Document/PDF generation (print layouts, letters) | Print-CSS HTML pages (UI-34, UI-35); the browser prints or saves as PDF; no server PDF engine | server PDF (krilla, wkhtmltopdf) | `decisions.md`; 72 print layouts are lists, letters and labels that HTML and CSS paged media cover |
| Integrations and secrets | EXT-01: `MailProvider` trait; `ResendProvider` (reqwest + rustls) when `RESEND_API_KEY` is set, else `OutboxMock` writing `mail_outbox` (D-05). EXT-02: plain Google Maps link. Secrets only from the environment | SMTP (lettre) | D-05: Resend replaces the 360Works plug-in and SMTP; no SMTP host, password or licence key in code, config or data |
| Authorization enforcement | axum middleware for AP-01 (session) and AP-02 (CSRF); `require_role` guard per handler (AP-03 to AP-13); domain preconditions in the command (AP-04, AP-05, AP-07, AP-09) | Pundit policies | default deny; route guard and domain check are separate layers (08) |
| Testing/observability/deployment | `cargo test`: domain fixtures on an in-memory SQLite per test (T-07-nn), route tests that call the axum router in process and check HTML with `scraper` (T-05-nn, T-08-nn), a local HTTP mock for the Resend contract (T-05-09). `tracing` logs with request id and secret masking. No CI or release pipeline beyond `cargo test` (demo) | RSpec, system tests | the operator runs broader testing; process work stays small |

### Why HTMX and server rendering fit these screens

- The screens are FileMaker forms with portals: a page shows a record, a portal shows related rows, a button runs a script. That maps to "a page renders rows, a button posts to a route, the route returns the changed fragment".
- The partial updates the source needs are known and few: portal rows (items, guests, payments, plan items, enrollments, e-mails), tab bodies, pickers in a dialog, and dependent totals. Each is one `hx-post` or `hx-get` with an `hx-target`, plus `hx-swap-oob` for totals.
- All rules stay in Rust on the server (07). No client copy of a rule exists to drift from the server.
- Print layouts are already page designs. Print CSS renders them without a second rendering path.

## Existing-stack decision

No target repository exists yet (the app folder is created in I1). The karbonfm `app/server` crate is the base to copy from, without the Tauri shell.

- Established stack: none in this repository; reference stack from migrate-karbonfm `app/server` (axum, askama, rusqlite, htmx).
- Keep/change decision: keep the karbonfm base; drop Tauri, Alpine.js, Tailwind/DaisyUI and Chart.js (decisions.md: small vanilla JS layer, no build step). Add argon2 (local sign-in), reqwest (Resend), calamine and rust_xlsxwriter (grades).
- Evidence requiring change: web server instead of desktop app (`decisions.md`); multi-user sign-in (D-50).
- Migration/reversal cost: low (demo, no user data, D-02).

## Authorization architecture

This table picks the mechanism for each layer. The policies themselves are in `08_auth_roles.md`.

| Source dimension | Target owner/mechanism | Default deny | Negative-test strategy |
|---|---|---|---|
| Roles/capabilities | `users.role` (`admin`, `staff`); ROLE-ADMIN, ROLE-STAFF | unknown role -> no access | T-05-05; `08` tests |
| Record predicates | none in the source (standard privilege sets only); domain preconditions for deletes and money (AP-04, AP-05, AP-07) | refuse when a dependent row exists | T-05-04, T-07-04, T-07-13 |
| Field read/write | query projection per template; `password_hash` and `RESEND_API_KEY` never selected or rendered (AP-13) | columns not in the projection are never sent | T-05-13 |
| Operations/scripts/value lists | one handler per route, `require_role`; value-list edits admin only (AP-08) | handler without a guard fails review | route table in `05` lists a policy per route |
| Routes/layouts | axum router; `/admin/*` nested router with an admin guard | unknown route 404; no session 303/401 | T-05-01, T-05-05 |
| Extended privileges/access channels | browser only (`fmapp` → HTTPS web app); no API, ODBC, XML or PHP channel (AP-14) | no route exists | T-08-19 |
| Service principals (robots, server schedules, API clients) | none. JOB-01 runs as the confirming user; JOB-02 as `system` with no domain writes | n/a | T-05-10 |
| UI visibility | templates hide admin links and void/delete buttons by role | Secondary only | hidden ≠ allowed: every hidden action has a 403 test |

## Interface style

Server-rendered HTML routes with HTMX partials (05). The routes come from the use cases of `07` (62 operations), not from the 41 tables: 150 routes, of which 47 are pages, 87 are writes, and 2 route families serve 72 print pages. A JSON API would add a second contract with no second client (AP-14).

## Migration strategy

Build order by workflow on seed data, no data load (D-02). A design milestone comes first (M1): a component set plus an HTML page shell for every kept screen of `06` (35 screens). Then the promo video milestone (M2), made from the design pages; build work waits until the operator approves it. Then the foundation (I1), then convention, school, e-mail and print increments. Each increment ships its domain invariants and negative authorization tests with its screens. `03_migration_plan.md` owns the increments. Production cutover is out of scope (demo).

## Current external evidence

| Claim affecting choice | Primary source/version | Access date | Fact or inference |
|---|---|---|---|
| axum, askama and rusqlite are current crates | crates.io via `cargo search`: axum 0.8.9, askama 0.16.1, rusqlite 0.43.0 | 2026-11-21 | fact (registry) |
| argon2id hashing is available in pure Rust | crates.io: argon2 0.6.0 | 2026-11-21 | fact (registry) |
| XLSX read and write for the grades round trip | crates.io: calamine 0.38.0 (read), rust_xlsxwriter 0.103.0 (write) | 2026-11-21 | fact (registry) |
| HTTPS client for the Resend API | crates.io: reqwest 0.13.5 | 2026-11-21 | fact (registry) |
| Local toolchain | `rustc 1.96.0` on the build machine | 2026-11-21 | fact |
| The same axum + askama + rusqlite + htmx base runs a FileMaker rebuild | migrate-karbonfm `app/server`, `migration/02_recommendations.md` (axum 0.8.9, askama 0.14.0, rusqlite 0.37.0, 2026-10-30) | 2026-11-21 | fact for that repository; askama and rusqlite moved one or more minor versions since |
| Resend accepts `POST /emails` with bearer key and an `Idempotency-Key` header | Resend API reference | not checked in this task | inference; check in I4 before the contract test (T-05-09) |

Pin crate versions in `Cargo.lock` at I1. askama 0.14 → 0.16 may change template syntax details; port the karbonfm templates with that in mind.

## Risks, spikes, and decisions

| Risk/unknown | Impact | Proof/spike | Owner | Decision deadline |
|---|---|---|---|---|
| R-01 Print CSS quality across browsers (labels in mm, page breaks, repeated table headers) | labels and badges misalign | print the Lista de Facturación design page and one label page in Chrome and Firefox at M1 | design milestone | M1 |
| R-02 askama 0.16 changes from the karbonfm templates (0.14) | template port takes longer | port the base layout and one page first | tech lead | I1 |
| R-03 SQLite write contention during JOB-01 | slow writes while send-all runs | each e-mail update is one short transaction; the provider call is outside it | tech lead | I4 |
| R-04 Resend contract details (attachments, idempotency) | mock and real path differ | contract test against a local HTTP mock; one manual send with a test key | operator | I4 |
| R-05 High-bit object flags undecoded (D-27) | a control hidden in FileMaker shows in the target | none for the demo; visible by default | operator | open |

## Rejected alternative

Ruby on Rails with Hotwire and SQLite would build the same screens: ActiveRecord over the `04` schema, Turbo Frames for portals, Stimulus for small behavior. It loses on simplicity, the operator's stated reason. It brings a larger framework surface (ORM, asset pipeline, generators, Turbo and Stimulus conventions) for a demo whose partial updates HTMX already covers with HTML attributes. It also gives up the karbonfm base, so nothing from the earlier rebuild carries over.

What would reverse the decision: a team that knows Rails and not Rust, or a need for a large admin surface that Rails generators would produce faster. Neither applies to this demo.

The tradeoff the primary accepts: more hand-written code for forms and validation than Rails gives by default, and Rust compile times on each template change.

## Open decisions

| ID | Decision | Rests on | Owner |
|---|---|---|---|
| OD-02-01 | Host for a shared demo (localhost only, or one VM with HTTPS) | out of scope (demo); D-80 | operator |
| OD-02-02 | Port the karbonfm CSS or write a new one in M1 | design milestone | operator |
