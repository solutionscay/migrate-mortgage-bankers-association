# MBA demo app: design pages (M1)

Static HTML design pages for the rebuilt MBA app (convention, school and e-mail modules). Open `design/components.html` in a browser straight from disk. The pages need no network access: the font, CSS and JS are all in `assets/`. All names and figures are fake.

## Pages

| File | Content | Spec |
|---|---|---|
| `components.html` | Every component in every state, inside the app shell: tokens, buttons, fields, the registration form, tags and statuses, tabs, search and lists, line items and totals, role views, dialogs, notices, empty and error states, mail preview, import preview, chart, print sheet | `migration/03_migration_plan.md` "M1 component set", `migration/06_ui_spec.md` "Design system" |

The page shells for UI-01 to UI-35 come in the next part of M1.

## Design rules

- Same app as the FileMaker screens, on a stricter grid. Right-aligned labels beside fields, a big record title with the status at the right, the line-item table across the middle, the totals block at the lower right. Tight rows (28 px).
- Warm stone canvas, white fields, charcoal top bar and nav bar. Teal is the one accent. Coral marks destructive actions and nothing else.
- Radius 0, no shadows, no gradients. 1 px hairlines and stone/white contrast separate areas. The one round shape is the radio button.
- The FileMaker solid teal buttons (Evento, Plan) become a white block with a teal left rule and teal text (`.pick`).
- One quiet row action (`.row-act`, "⋯") at the end of a row opens a menu. Void and delete appear only for ROLE-ADMIN (`.admin-only`). The column keeps its width in both role views.
- No FileMaker chrome: no record navigator, layout picker or found-set bar.

## Fixed shell

`.app` is a CSS grid with constant row heights: top bar 40, nav 36, page bar 72, content, footer 28. Only `main.content` scrolls. It always shows a scrollbar track (`overflow-y: scroll` and `scrollbar-gutter: stable`), so its width never changes. `html` does not scroll, so a modal dialog cannot shift the page. Dialogs (`<dialog>`), menus and toasts (`popover`) open in the top layer, outside the grid. The current nav tab keeps its font weight, so its neighbours do not move.

Check: `screenshots/components-top.png` and `screenshots/components-scrolled.png` have identical pixels in the header (rows 0 to 147) and the footer (rows 872 to 899).

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
  partials/              shared shell: head, icons (SVG sprite), topbar, nav, footer
  assets/app.css         tokens, shell, components
  assets/app.js          active nav, role view, dialogs, row menus, tabs, toasts, Ctrl+S (vanilla, no framework)
  assets/fonts/          IBM Plex Sans woff2 (Regular, Medium, SemiBold, Bold) and OFL.txt
  tools/sync_partials.py
  screenshots/           1440x900, headless Chromium
```

Each page holds copies of the partials between `<!-- @partial NAME -->` and `<!-- @endpartial -->`, so pages open from disk. Edit the file in `partials/`, then run `python3 design/tools/sync_partials.py` (`--check` reports stale pages). In the Rust templates each marker pair becomes an include. The active module and nav tab (`<body data-module="…" data-nav="…">`) become template variables.

The footer has a "Ver como" switch for ROLE-ADMIN and ROLE-STAFF. It only changes what the page draws. The server guards every route.

Screenshot hooks in `app.js`: `?role=staff`, `?scroll=<px>`, `?at=<section id>`, `?open=<dialog id>`, `?menu=<button id>`.

## Screenshots

| File | Shows |
|---|---|
| `screenshots/components-top.png` | top of `components.html` |
| `screenshots/components-scrolled.png` | scrolled to "Detalle y totales"; header and footer unchanged |
| `screenshots/components-dialog.png` | the plan-change confirm dialog open |
| `screenshots/components-roles.png` | ROLE-ADMIN and ROLE-STAFF views side by side |

## Font

IBM Plex Sans, copyright IBM Corp., SIL Open Font License 1.1 (`assets/fonts/OFL.txt`). Source: [github.com/IBM/plex](https://github.com/IBM/plex), release `@ibm/plex-sans@1.1.0`. Its figures have equal widths by default, which the money columns need.
