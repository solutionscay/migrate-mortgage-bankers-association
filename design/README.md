# MBA demo app: design pages (M1)

Static HTML design pages for the rebuilt MBA app (convention, school and e-mail modules): a component gallery and one page for each of the 35 kept screens (UI-01 to UI-35). Open `design/index.html` or `design/components.html` in a browser straight from disk. The pages need no network access: the font, CSS and JS are all in `assets/`. All names and figures are fake.

## Pages

Open any page from disk. States are reached with the hooks in "State hooks" below. "full" pages show every field, collection, action and state from 06; "shell" pages show the screen in the app shell with its main parts and a few synthetic rows.

| File | UI | Screen | Depth | States (how to reach) |
|---|---|---|---|---|
| `components.html` | — | Component gallery | all components | every state in place; save states `?state=guardando`, `?state=sin-guardar` (section `#autosave`); `?at=<section>`; dialogs via the buttons |

**Inicio and session**

| File | UI | Screen | Depth | States (how to reach) |
|---|---|---|---|---|
| `login.html` | UI-01 | Iniciar sesión | shell, no app nav | empty; `?state=error` (also the locked case, AP-11) |
| `index.html` | UI-02 | Inicio | shell | module tiles; `?role=staff` hides the Admin tile |
| `cambiar-contrasena.html` | UI-33 | Cambiar contraseña | shell | empty; `?state=error` |

**Convención**

| File | UI | Screen | Depth | States (how to reach) |
|---|---|---|---|---|
| `convencionistas.html` | UI-03 | Convencionistas | shell | list, filters, row menu |
| `convencionista.html` | UI-04 | Convencionista | full | `?state=error`, `?state=vacio`, `?open=dlg-borrar` (409), `?open=dlg-borrar-reg`, `?role=staff` |
| `registros.html` | UI-05 | Registros | shell | list with event, status and type filters |
| `registro.html` | UI-06 | Registro | full (meaty) | `?state=vacio`, `?state=guardando` (qty change, totals refreshing), `?state=sin-guardar` (qty error, footer \"1 campo sin guardar\"); `?open=` `dlg-evento`, `dlg-plan`, `dlg-plan-confirm`, `dlg-atributos-hotel`, `dlg-atributos-mesa`, `dlg-quitar`, `dlg-quitar-guest`, `dlg-pago`, `dlg-pago-error`, `dlg-anular` (admin); `?role=staff&at=pagos` |
| `registro-puerta.html` | UI-07 | Registro en puerta | shell, tablet | three status buttons per row; works at 820 px |
| `convenciones.html` | UI-08 | Convenciones | shell | list |
| `convencion.html` | UI-09 | Convención | full (meaty) | `?tab=tab-act` / `tab-aus` / `tab-fac`; `?open=` `dlg-quitar-act` (409), `dlg-quitar-sp`, `dlg-quitar-bill`, `dlg-borrar-ev` (admin); `?menu=btn-print`, `?menu=btn-ev-more` |
| `actividad.html` | UI-10 | Detalle de actividad | shell | item fields and its registrations |
| `auspiciador.html` | UI-11 | Auspiciador | shell | sponsor fields and prints |
| `planes-lista.html` | UI-12 | Planes | shell | list with event filter |
| `planes.html` | UI-13 | Plan | full (meaty) | `?state=sin-evento`, `?state=sin-detalles`; `?open=` `dlg-reconstruir`, `dlg-quitar-pitem`, `dlg-borrar-plan` (admin) |
| `lista-facturacion.html` | UI-34 | Lista de Facturación (print) | full (meaty, print) | `?state=vacio` ("No hay registros para imprimir"); prints on one Letter page |

**Escuela**

| File | UI | Screen | Depth | States (how to reach) |
|---|---|---|---|---|
| `estudiantes.html` | UI-14 | Estudiantes | shell | list, filters |
| `estudiante.html` | UI-15 | Estudiante | full (meaty) | `?tab=tab-cert` / `tab-cur` / `tab-sem` / `tab-cc` (empty) / `tab-dat`; `?open=dlg-nuevo` (validation), `?open=dlg-borrar-stu` (409); `?role=staff` |
| `afiliados.html` | UI-16 | Afiliados | shell | list, print links |
| `afiliado.html` | UI-17 | Afiliado | shell | `?open=dlg-borrar-afi`; `?role=staff` |
| `ofrecimiento.html` | UI-18 | Ofrecimientos | shell | `?tab=tab-cal` / `tab-det` |
| `fecha-curso.html` | UI-19 | Fecha del curso | shell (dialog) | dialog opens on load (`<body data-open="dlg-fecha">`), autosave with Cerrar; `?state=guardando` |
| `matricula.html` | UI-20 | Matrícula | full (meaty, dialog) | opens `dlg-mat` on load; `?open=` `dlg-mat-ind`, `dlg-mat-err`, `dlg-mat-pago`, `dlg-mat-sem`, `dlg-mat-sinfecha`, `dlg-mat-borrar` |
| `matriculas.html` | UI-21 | Matrículas | shell | list with grades, export and import |
| `importar-notas.html` | UI-22 | Importar notas | shell (3 steps) | preview; `?open=dlg-aplicar` |
| `reportes.html` | UI-23 | Reportes | shell | criteria and result |
| `panel-matriculas.html` | UI-24 | Panel de matrículas | shell, "borrador" | tiles and SVG charts |
| `print-escuela.html` | UI-35 | Impresos de la escuela | shell (print) | one enrollment list on Letter |

**Correos**

| File | UI | Screen | Depth | States (how to reach) |
|---|---|---|---|---|
| `envios.html` | UI-25 | Envíos | shell | batch list |
| `correos.html` | UI-26 | Envío de correos | full (meaty) | `?state=` `borrador`, `enviando`, `errores`, `terminado`, `vacio`, `invalido`; `?open=` `dlg-enviar-todos`, `dlg-borrar-correo`, `dlg-reenviar` |
| `vista-previa.html` | UI-27 | Vista previa | shell | sandboxed preview |
| `bandeja-salida.html` | UI-28 | Bandeja de salida | shell | outbox with detail |

**Admin** (ROLE-ADMIN only; `?role=staff` shows the 403 page in the shell)

| File | UI | Screen | Depth | States (how to reach) |
|---|---|---|---|---|
| `admin-usuarios.html` | UI-29 | Usuarios | shell | `?open=dlg-clave`, `?open=dlg-desactivar`, `?role=staff` |
| `admin-configuracion.html` | UI-30 | Configuración | shell | `?tab=tab-org` / `tab-esc` / `tab-cor` / `tab-ses` / `tab-doc` / `tab-val` |
| `admin-correo.html` | UI-31 | Plantillas y remitentes | shell | template editor, profiles with an error |
| `admin-certificaciones.html` | UI-32 | Certificaciones y profesores | shell, "borrador" | requirements and professors |

## Design rules

- Same app as the FileMaker screens, on a stricter grid. Right-aligned labels beside fields, a big record title with the status at the right, the line-item table across the middle, the totals block at the lower right. Tight rows (28 px).
- Warm stone canvas, white fields, charcoal top bar and nav bar. Teal is the one accent. Coral marks destructive actions and nothing else.
- Radius 0, no shadows, no gradients. 1 px hairlines and stone/white contrast separate areas. Radio buttons are square too, with a square dot.
- The FileMaker solid teal buttons (Evento, Plan) become a white block with a teal left rule and teal text (`.pick`).
- One quiet row action (`.row-act`, "⋯") at the end of a row opens a menu. A child row without money keeps "Quitar" for both roles (`.act-remove`, AP-06). Void and delete of a row with money (`.row-money`) appear only for ROLE-ADMIN (`.act-void`, `.act-delete` with `.admin-only`, AP-05). The column keeps its width in both role views.
- No FileMaker chrome: no record navigator, layout picker or found-set bar.
- No Save button on record pages, as in FileMaker: each field saves when the user leaves it (see "Saving"). Guardar stays only in dialogs that create or settle a record.

## Fixed shell

`.app` is a CSS grid with constant row heights: top bar 40, nav 36, page bar 72, content, footer 28. Only `main.content` scrolls. It always shows a scrollbar track (`overflow-y: scroll` and `scrollbar-gutter: stable`), so its width never changes. `html` does not scroll, so a modal dialog cannot shift the page. Dialogs (`<dialog>`), menus and toasts (`popover`) open in the top layer, outside the grid. The current nav tab keeps its font weight, so its neighbours do not move.

Check: `tools/shoot.sh --compare` on `screenshots/components-top.png` and `screenshots/components-scrolled.png` (and on any page and its scrolled shot) finds identical pixels in the header (rows 0 to 147) and the footer (rows 872 to 899).

## Tokens

All tokens are CSS custom properties at the top of `assets/app.css`.

| Group | Tokens |
|---|---|
| Surfaces | `--stone-50 #f7f6f3`, `--stone-100 #f0eee9` (canvas), `--stone-200 #e5e2db`, `--stone-300 #d6d2c9` (hairline), `--stone-400 #b9b4aa`, `--white` |
| Bars | `--charcoal-2 #2a2b2e` (top bar, footer), `--charcoal #37383b` (nav bar, primary button) |
| Ink | `--ink #1d1c1a`, `--ink-2 #4a4741` (labels), `--muted #6b675f` |
| Accent | `--teal #41b1c6` (rules, marks, focus), `--teal-ink #1a7384` (text), `--teal-tint #eaf5f7` (selected row) |
| Destructive | `--coral #f16350` (fill, ink text on it), `--coral-ink #b73a28` (text on white) |
| Status | `--ok-*`, `--warn-*`, `--err-*`, `--info-*`, `--neu-*` (ink, bg, line); a status always has a text label |
| Type | IBM Plex Sans 400/500/600/700; `--fs-xs 11`, `--fs-sm 12`, `--fs-base 13`, `--fs-md 15`, `--fs-lg 18`, `--fs-xl 24`; tabular figures everywhere |
| Spacing | 4 px grid: `--s-1 4` to `--s-7 48` |
| Sizes | `--h-row 28`, `--h-field 26`, `--h-btn 28`, `--w-label 128`, shell heights above |

## Structure

```
design/
  components.html        component gallery
  *.html                 35 page designs (UI-01 to UI-35), see "Pages"
  partials/              shared shell: head, icons (SVG sprite), topbar, nav, footer
  assets/app.css         tokens, shell, components
  assets/app.js          active nav, role view, dialogs, row menus, tabs, toasts, save state, Esc revert, Ctrl+S (vanilla, no framework)
  assets/fonts/          IBM Plex Sans woff2 (Regular, Medium, SemiBold, Bold) and OFL.txt
  tools/sync_partials.py
  tools/page-template.html  starting point for a new page (sync_partials skips tools/)
  tools/shoot.sh         1440x900 headless screenshot of one page; --compare checks header and footer pixels
  screenshots/           1440x900, headless Chromium
```

Each page holds copies of the partials between `<!-- @partial NAME -->` and `<!-- @endpartial -->`, so pages open from disk. Edit the file in `partials/`, then run `python3 design/tools/sync_partials.py` (`--check` reports stale pages). In the Rust templates each marker pair becomes an include. The active module and nav tab (`<body data-module="…" data-nav="…">`) become template variables.

Navigation: the top bar holds the module tabs (Convención, Escuela, Correos, Admin for ROLE-ADMIN only). `partials/nav.html` holds one tab group per module (`data-group`), plus an `inicio` group for Inicio and Cambiar contraseña. CSS shows the group that matches `<body data-module>`, so it works from disk with no script. `<body data-nav>` marks the current tab; a detail page marks its list (for example `registro.html` uses `data-nav="registros"`). Every tab links to its page file from the 03 "M1 page list".

New page: copy `tools/page-template.html` to `design/<page>.html`, set `data-module`, `data-nav`, the page bar and the content, then run `sync_partials.py`. Page CSS goes only in the matching section at the end of `assets/app.css`: `pages: convention`, `pages: school`, `pages: mail and admin`.

Screenshot: `design/tools/shoot.sh <page.html> "<query>" <name>` writes `design/screenshots/<name>.png`.

The footer has a "Ver como" switch for ROLE-ADMIN and ROLE-STAFF. It only changes what the page draws. The server guards every route.


## Saving (per-field autosave)

Record pages are always editable. There is no Edit, Cancel or Guardar button on them (operator decision, 2026-11-21).

- **When a field saves.** A changed field saves on change (on blur for text). htmx sends only that field: `hx-patch="/registrations/1283" hx-trigger="change" hx-target="closest .field-wrap" hx-swap="outerHTML"`, and `name` is the column. The server validates, saves and returns the field fragment. Routes: `migration/05_api_design.md` "Per-field autosave (PATCH)".
- **Line fields.** Price, qty, no charge and attributes use `hx-target="closest tr"`. The response brings the row plus the totals block out of band (`id="totales"`, `hx-swap-oob`). Totals always come from the server.
- **Error.** The response is 422 with the field fragment. The value stays as typed, with a red border and a message under the field (for a line field, in a row below). It is not saved. The wrapper carries `data-unsaved`.
- **Esc.** Esc reverts a changed field to its saved value (`data-saved`, else the HTML value) before the user leaves it. In a dialog, Esc on an unchanged field still closes the dialog.
- **Footer.** The footer slot (`partials/footer.html`, `.save-state`) shows the save state:
  - "Guardando…" on `htmx:beforeRequest`;
  - "Guardado h:mm p. m." after a successful swap;
  - "N campo(s) sin guardar", the count of `[data-unsaved]`.

  The slot has a fixed width, so the footer never moves.
- **Concurrency.** Last write wins, per field. Every change writes an `audit_log` row.
- **Ctrl+S.** Ctrl+S saves the focused field at once. In a dialog form it submits the form.
- **Static pages.** They have no server and no htmx. app.js shows the same footer cycle locally on change. `?state=guardando` and `?state=sin-guardar` draw the states.

Guardar stays in the dialogs that create or settle a record:
- payment (`registro.html` `dlg-pago`);
- enrollment (`matricula.html`);
- new student (`estudiante.html` `dlg-nuevo`);
- user password (`admin-usuarios.html` `dlg-clave`);
- the submit forms of sign-in and password change (`login.html`, `cambiar-contrasena.html`).

The attribute dialogs (`registro.html` `dlg-atributos-*`) and the calendar-row dialog (`fecha-curso.html`) edit an existing record. They autosave too and close with Cerrar.

## State hooks (`assets/app.js`)

| Hook | Effect |
|---|---|
| `?role=staff` | ROLE-STAFF view (the footer "Ver como" switch does the same). `.admin-only` hides; `.staff-only` shows (the 403 page on admin pages) |
| `?state=guardando`, `?state=sin-guardar` | save states: the footer shows "Guardando…" or "N campo(s) sin guardar" (counts the visible invalid fields); pages draw the matching field and row states |
| `?state=NAME` | sets `<body data-state="NAME">`; only elements whose `data-st` lists NAME show. With no state, elements that list `base` show. Elements without `data-st` always show. Without JS the `base` view shows |
| `?tab=<tab id>` | selects that tab of its tablist |
| `?open=<dialog id>` | opens that `<dialog>` on load; else `<body data-open="<dialog id>">` opens the page's own dialog |
| `?at=<element id>`, `?scroll=<px>` | scrolls the content area |
| `?menu=<button id>` | opens that button's popover menu (stays open in headless screenshots) |

Hooks combine: `registro.html?role=staff&at=pagos`.

## Data

All data is synthetic. Names come from the before mockups or are invented in the same style; e-mail addresses use `ejemplo.com`. Phone numbers use the reserved fictional range 787-555-01xx / 787-555-02xx (operator decision). No page shows an SMTP host, an SMTP password or a licence key. The before mockups, the before/after pairs and the print PDF stay outside this repository.

## Screenshots

All 1440x900, headless Chromium, made with `tools/shoot.sh` unless noted.

| File | Page and hooks |
|---|---|
| `screenshots/components-top.png` | `components.html` |
| `screenshots/components-scrolled.png` | `components.html?at=detalle` |
| `screenshots/components-dialog.png` | `components.html?at=formulario&open=dlg-plan` |
| `screenshots/components-roles.png` | `components.html?at=roles` |
| `screenshots/components-guardado.png` | `components.html?at=autosave` |
| `screenshots/components-sin-guardar.png` | `components.html?at=autosave&state=sin-guardar` |
| `screenshots/login.png` | `login.html` |
| `screenshots/login-error.png` | `login.html?state=error` |
| `screenshots/index.png` | `index.html` |
| `screenshots/cambiar-contrasena.png` | `cambiar-contrasena.html` |
| `screenshots/cambiar-contrasena-error.png` | `cambiar-contrasena.html?state=error` |
| `screenshots/convencionistas.png` | `convencionistas.html` |
| `screenshots/convencionista.png` | `convencionista.html` |
| `screenshots/convencionista-error.png` | `convencionista.html?state=error` |
| `screenshots/convencionista-vacio.png` | `convencionista.html?state=vacio` |
| `screenshots/convencionista-borrar.png` | `convencionista.html?open=dlg-borrar` |
| `screenshots/convencionista-staff.png` | `convencionista.html?role=staff` |
| `screenshots/registros.png` | `registros.html` |
| `screenshots/registro.png` | `registro.html` |
| `screenshots/registro-pagos.png` | `registro.html?at=pagos` |
| `screenshots/registro-pagos-staff.png` | `registro.html?role=staff&at=pagos` |
| `screenshots/registro-vacio.png` | `registro.html?state=vacio` |
| `screenshots/registro-plan-confirm.png` | `registro.html?open=dlg-plan-confirm` |
| `screenshots/registro-pago-error.png` | `registro.html?open=dlg-pago-error` |
| `screenshots/registro-atributos.png` | `registro.html?open=dlg-atributos-hotel` |
| `screenshots/registro-guardando.png` | `registro.html?state=guardando` |
| `screenshots/registro-sin-guardar.png` | `registro.html?state=sin-guardar` |
| `screenshots/registro-atributos-guardando.png` | `registro.html?open=dlg-atributos-hotel&state=guardando` |
| `screenshots/registro-anular.png` | `registro.html?open=dlg-anular&at=pagos` |
| `screenshots/registro-puerta.png` | `registro-puerta.html` |
| `screenshots/registro-puerta-tablet.png` | `registro-puerta.html` at 820x1180 |
| `screenshots/convenciones.png` | `convenciones.html` |
| `screenshots/convencion.png` | `convencion.html` |
| `screenshots/convencion-auspiciadores.png` | `convencion.html?tab=tab-aus` |
| `screenshots/convencion-facturacion.png` | `convencion.html?tab=tab-fac` |
| `screenshots/convencion-quitar-409.png` | `convencion.html?open=dlg-quitar-act` |
| `screenshots/convencion-menu.png` | `convencion.html?menu=btn-ev-more` |
| `screenshots/convencion-menu-staff.png` | `convencion.html?role=staff&menu=btn-ev-more` |
| `screenshots/actividad.png` | `actividad.html` |
| `screenshots/auspiciador.png` | `auspiciador.html` |
| `screenshots/planes-lista.png` | `planes-lista.html` |
| `screenshots/planes.png` | `planes.html` |
| `screenshots/planes-sin-evento.png` | `planes.html?state=sin-evento` |
| `screenshots/planes-sin-detalles.png` | `planes.html?state=sin-detalles` |
| `screenshots/planes-reconstruir.png` | `planes.html?open=dlg-reconstruir` |
| `screenshots/lista-facturacion.png` | `lista-facturacion.html` |
| `screenshots/lista-facturacion-vacio.png` | `lista-facturacion.html?state=vacio` |
| `screenshots/estudiantes.png` | `estudiantes.html` |
| `screenshots/estudiante.png` | `estudiante.html` |
| `screenshots/estudiante-cursos.png` | `estudiante.html?tab=tab-cur` |
| `screenshots/estudiante-seminarios.png` | `estudiante.html?tab=tab-sem` |
| `screenshots/estudiante-vacio.png` | `estudiante.html?tab=tab-cc` |
| `screenshots/estudiante-datos.png` | `estudiante.html?tab=tab-dat` |
| `screenshots/estudiante-nuevo.png` | `estudiante.html?open=dlg-nuevo` |
| `screenshots/estudiante-borrar.png` | `estudiante.html?open=dlg-borrar-stu` |
| `screenshots/estudiante-staff.png` | `estudiante.html?role=staff` |
| `screenshots/afiliados.png` | `afiliados.html` |
| `screenshots/afiliado.png` | `afiliado.html` |
| `screenshots/afiliado-borrar.png` | `afiliado.html?open=dlg-borrar-afi` |
| `screenshots/afiliado-staff.png` | `afiliado.html?role=staff` |
| `screenshots/ofrecimiento.png` | `ofrecimiento.html` |
| `screenshots/ofrecimiento-detalles.png` | `ofrecimiento.html?tab=tab-det` |
| `screenshots/fecha-curso.png` | `fecha-curso.html` |
| `screenshots/fecha-curso-guardando.png` | `fecha-curso.html?state=guardando` |
| `screenshots/matricula.png` | `matricula.html` |
| `screenshots/matricula-individual.png` | `matricula.html?open=dlg-mat-ind` |
| `screenshots/matricula-errores.png` | `matricula.html?open=dlg-mat-err` |
| `screenshots/matricula-con-pago.png` | `matricula.html?open=dlg-mat-pago` |
| `screenshots/matricula-seminario.png` | `matricula.html?open=dlg-mat-sem` |
| `screenshots/matricula-sin-fechas.png` | `matricula.html?open=dlg-mat-sinfecha` |
| `screenshots/matriculas.png` | `matriculas.html` |
| `screenshots/importar-notas.png` | `importar-notas.html` |
| `screenshots/importar-notas-aplicar.png` | `importar-notas.html?open=dlg-aplicar` |
| `screenshots/reportes.png` | `reportes.html` |
| `screenshots/panel-matriculas.png` | `panel-matriculas.html` |
| `screenshots/print-escuela.png` | `print-escuela.html` |
| `screenshots/envios.png` | `envios.html` |
| `screenshots/correos.png` | `correos.html` |
| `screenshots/correos-lista.png` | `correos.html?at=correos-lista` |
| `screenshots/correos-dialog.png` | `correos.html?open=dlg-enviar-todos` |
| `screenshots/correos-enviando.png` | `correos.html?state=enviando` |
| `screenshots/correos-errores.png` | `correos.html?state=errores` |
| `screenshots/correos-terminado.png` | `correos.html?state=terminado` |
| `screenshots/correos-borrador.png` | `correos.html?state=borrador` |
| `screenshots/correos-vacio.png` | `correos.html?state=vacio` |
| `screenshots/correos-invalido.png` | `correos.html?state=invalido` |
| `screenshots/vista-previa.png` | `vista-previa.html` |
| `screenshots/bandeja-salida.png` | `bandeja-salida.html` |
| `screenshots/admin-usuarios.png` | `admin-usuarios.html` |
| `screenshots/admin-usuarios-staff.png` | `admin-usuarios.html?role=staff` |
| `screenshots/admin-usuarios-dialog.png` | `admin-usuarios.html?open=dlg-clave` |
| `screenshots/admin-configuracion.png` | `admin-configuracion.html` |
| `screenshots/admin-correo.png` | `admin-correo.html` |
| `screenshots/admin-certificaciones.png` | `admin-certificaciones.html` |

## Font

IBM Plex Sans, copyright IBM Corp., SIL Open Font License 1.1 (`assets/fonts/OFL.txt`). Source: [github.com/IBM/plex](https://github.com/IBM/plex), release `@ibm/plex-sans@1.1.0`. Its figures have equal widths by default, which the money columns need.
