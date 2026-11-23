// MBA design pages: small vanilla JS, no framework, no build step.
// Active nav, role view, dialogs, row menus, tabs, toasts, Ctrl+S.
// The Rust templates keep the same data-* hooks (I1 onward); htmx replaces the demo stubs.
(function () {
  "use strict";
  const $ = (s, el = document) => el.querySelector(s);
  const $$ = (s, el = document) => Array.from(el.querySelectorAll(s));
  const params = new URLSearchParams(location.search);
  // Served by the app (<body data-app="server">): the server sets role, user name and page state, and forms post
  // for real. The demo-only stubs below stay off.
  const live = document.body.dataset.app === "server";

  // ---- Active module and nav tab (a template variable in the real app) ----
  function markCurrent() {
    const b = document.body.dataset;
    $$(".modules [data-module]").forEach((a) => a.toggleAttribute("aria-current", a.dataset.module === b.module));
    $$(".nav [data-nav]").forEach((a) => {
      if (a.dataset.nav === b.nav) a.setAttribute("aria-current", "page"); else a.removeAttribute("aria-current");
    });
  }

  // ---- Role view: ROLE-ADMIN or ROLE-STAFF. Visibility only; the server guards the routes. ----
  function setRole(role) {
    document.body.dataset.role = role;
    $$("[data-set-role]").forEach((btn) => btn.setAttribute("aria-pressed", String(btn.dataset.setRole === role)));
    $$("[data-role-name]").forEach((el) => (el.textContent = role === "admin" ? "admin.demo" : "staff.demo"));
    $$("[data-role-text]").forEach((el) => (el.textContent = role === "admin" ? "ROLE-ADMIN" : "ROLE-STAFF"));
    try { localStorage.setItem("mba-role", role); } catch (_) {}
  }

  // ---- Dialogs: native <dialog>, focus returns to the opener, Esc closes ----
  let opener = null;
  function openDialog(id, from) {
    const d = document.getElementById(id);
    if (!d || d.open) return;
    closeMenus();
    opener = from || document.activeElement;
    d.showModal();
    const first = $("[autofocus]", d) || $(".dlg-body input, .dlg-body select, .dlg-body textarea, .dlg-body button", d);
    if (first) first.focus();
  }
  document.addEventListener("close", (e) => {
    if (e.target.matches("dialog") && opener) { opener.focus({ preventScroll: true }); opener = null; }
  }, true);

  // ---- Row and user menus: popovers in the top layer, placed under their button ----
  function closeMenus() { $$(".menu:popover-open").forEach((m) => m.hidePopover()); }
  function toggleMenu(btn) {
    const m = document.getElementById(btn.dataset.menu);
    if (!m) return;
    if (m.matches(":popover-open")) { m.hidePopover(); return; }
    closeMenus();
    m.showPopover();
    const r = btn.getBoundingClientRect();
    const w = m.offsetWidth, h = m.offsetHeight;
    let left = r.right - w, top = r.bottom + 2;
    if (top + h > innerHeight - 32) top = r.top - h - 2;   // keep clear of the footer
    m.style.position = "fixed";
    m.style.left = Math.max(8, left) + "px";
    m.style.top = top + "px";
    btn.setAttribute("aria-expanded", "true");
    m.addEventListener("toggle", function off(ev) {
      if (ev.newState === "closed") { btn.setAttribute("aria-expanded", "false"); m.removeEventListener("toggle", off); }
    });
    const firstItem = $("button, a", m);
    if (firstItem) firstItem.focus({ preventScroll: true });
  }

  // ---- Tabs: links that swap a panel (hx-get + hx-push-url in the app) ----
  function selectTab(tab) {
    const list = tab.closest("[role=tablist]");
    $$("[role=tab]", list).forEach((t) => {
      const on = t === tab;
      t.setAttribute("aria-selected", String(on));
      t.tabIndex = on ? 0 : -1;
      const p = document.getElementById(t.getAttribute("aria-controls"));
      if (p) p.hidden = !on;
    });
  }

  // ---- Toasts: a manual popover, so they sit above dialogs and never move the layout ----
  window.mbaToast = function (text, kind) {
    let host = $("#toast-host");
    if (!host) {
      host = document.createElement("div");
      host.id = "toast-host"; host.className = "toast-host"; host.setAttribute("popover", "manual");
      host.setAttribute("role", "status");
      document.body.appendChild(host);
    }
    if (!host.matches(":popover-open")) host.showPopover();
    const t = document.createElement("div");
    t.className = "toast" + (kind === "err" ? " err" : "");
    t.innerHTML = `<svg class="i"><use href="#i-${kind === "err" ? "alert" : "check"}"/></svg><span></span>`;
    t.querySelector("span").textContent = text;
    host.appendChild(t);
    setTimeout(() => { t.remove(); if (!host.children.length) host.hidePopover(); }, 2600);
  };

  // ---- One click handler for every data-* hook ----
  document.addEventListener("click", (e) => {
    const el = e.target.closest("[data-open],[data-close],[data-menu],[data-set-role],[role=tab],[data-toast],[data-busy],[data-pick],[data-soon]");
    if (!el) {
      if (e.target.matches("dialog.dlg")) e.target.close();   // backdrop click
      return;
    }
    if (el.dataset.open) { e.preventDefault(); openDialog(el.dataset.open, el); }
    else if (el.hasAttribute("data-close")) { e.preventDefault(); el.closest("dialog").close(); if (el.dataset.toast) mbaToast(el.dataset.toast); }
    else if (el.dataset.menu) { e.preventDefault(); toggleMenu(el); }
    else if (el.dataset.setRole) { setRole(el.dataset.setRole); }
    else if (el.getAttribute("role") === "tab") { e.preventDefault(); selectTab(el); }
    else if (el.dataset.busy !== undefined) {
      e.preventDefault();
      el.setAttribute("aria-busy", "true");
      setTimeout(() => { el.removeAttribute("aria-busy"); if (el.dataset.toast) mbaToast(el.dataset.toast); }, 1400);
    }
    else if (el.dataset.pick) {
      const d = el.closest("dialog");
      if (d) d.close();
      mbaToast(el.dataset.pick);
    }
    else if (el.dataset.toast) { e.preventDefault(); closeMenus(); mbaToast(el.dataset.toast, el.dataset.kind); }
    else if (el.dataset.soon !== undefined) { e.preventDefault(); mbaToast((el.dataset.soon || "Esta pantalla") + " llega en la siguiente parte de M1"); }
  });

  // Arrow keys move between tabs (WAI-ARIA tabs pattern).
  document.addEventListener("keydown", (e) => {
    const tab = e.target.closest && e.target.closest("[role=tab]");
    if (tab && (e.key === "ArrowRight" || e.key === "ArrowLeft")) {
      const tabs = $$("[role=tab]", tab.closest("[role=tablist]"));
      const next = tabs[(tabs.indexOf(tab) + (e.key === "ArrowRight" ? 1 : -1) + tabs.length) % tabs.length];
      selectTab(next); next.focus();
    }
    // Ctrl+S / Cmd+S: in a create/settle dialog it submits the form; on a record field it saves
    // that field now (same as leaving it). Record pages have no Guardar button.
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
      e.preventDefault();
      const el = document.activeElement;
      if (el && el.matches("[hx-patch]")) el.dispatchEvent(new Event("change", { bubbles: true }));
      else if (el && el.closest("dialog form")) el.closest("form").requestSubmit();
      else mbaToast("Cada campo se guarda al salir de él");
    }
    // Esc reverts a changed field to its saved value before the user leaves it (FileMaker Revert Record).
    if (e.key === "Escape" && e.target.matches && e.target.matches("[hx-patch]")) {
      const el = e.target, saved = savedOf(el);
      if (currentOf(el) !== saved) {
        e.preventDefault(); e.stopPropagation();
        if (el.type === "checkbox" || el.type === "radio") el.checked = saved === "on"; else el.value = saved;
        clearError(el);
      }
    }
  }, true);

  // Demo forms do not post anywhere.
  document.addEventListener("submit", (e) => {
    if (live) return;
    e.preventDefault();
    const d = e.target.closest("dialog");
    if (d) d.close();
    mbaToast(e.target.dataset.done || "Guardado");
  });

  // ---- Per-field autosave: footer save state from htmx events ----
  const savedOf = (el) => (el.dataset.saved !== undefined ? el.dataset.saved : el.defaultValue);
  const currentOf = (el) => (el.type === "checkbox" || el.type === "radio" ? (el.checked ? "on" : "off") : el.value);
  const clock = () => new Date().toLocaleTimeString("es-PR", { hour: "numeric", minute: "2-digit" }).replace(/\s?([ap])\.?\s?m\.?/i, " $1. m.");
  function setSave(kind, n) {
    const slot = $("footer.footer .save-state");
    if (!slot) return;
    slot.dataset.save = kind;
    $(".save-text", slot).textContent =
      kind === "saving" ? "Guardando…" :
      kind === "error" ? `${n} campo${n === 1 ? "" : "s"} sin guardar` : `Guardado ${clock()}`;
  }
  const unsaved = () => $$("[data-unsaved]").length;
  function settle() { const n = unsaved(); if (n) setSave("error", n); else setSave("saved"); }
  function clearError(el) {
    el.removeAttribute("aria-invalid");
    const wrap = el.closest(".field-wrap, tr, .status-slot");
    if (wrap) {
      wrap.removeAttribute("data-unsaved"); $$(".field-error", wrap).forEach((m) => m.remove());
      const next = wrap.nextElementSibling;   // a line item shows its message in the row below
      if (next && next.classList.contains("row-error")) next.remove();
    }
    settle();
  }
  window.mbaSave = setSave;
  document.addEventListener("htmx:beforeRequest", (e) => { if (e.target.matches("[hx-patch]")) setSave("saving"); });
  // The server's 422 field fragment carries data-unsaved on its wrapper (htmx.config.responseHandling
  // swaps 422); a 200 fragment does not. After any swap, count what is left.
  document.addEventListener("htmx:afterSettle", settle);
  document.addEventListener("htmx:responseError", settle);
  // Static design pages have no server and no htmx: show the same cycle locally.
  document.addEventListener("change", (e) => {
    if (window.htmx || !e.target.matches || !e.target.matches("[hx-patch]")) return;
    setSave("saving");
    setTimeout(() => { e.target.dataset.saved = currentOf(e.target); settle(); }, 600);
  });

  // A fixed-position menu would drift from its button, so scrolling its container or resizing closes it.
  document.addEventListener("scroll", (e) => {
    const btn = $("[data-menu][aria-expanded=true]");
    if (btn && e.target.contains && e.target.contains(btn)) closeMenus();
  }, true);
  // Headless screenshots resize the window once; with ?menu= that resize must not close the menu.
  let size = innerWidth + "x" + innerHeight;
  addEventListener("resize", () => {
    const s = innerWidth + "x" + innerHeight;
    if (s !== size) { size = s; if (!params.get("menu")) closeMenus(); }
  });

  // Page states: <body data-state="NAME"> shows only the [data-st] elements that list NAME;
  // with no state, only those that list "base". Elements without data-st always show.
  function setState(name) {
    if (name) document.body.dataset.state = name; else delete document.body.dataset.state;
    $$("[data-st]").forEach((el) => el.classList.toggle("st-off", !!name && !el.dataset.st.split(/\s+/).includes(name)));
  }
  window.mbaState = setState;

  document.addEventListener("DOMContentLoaded", () => {
    markCurrent();
    if (!live) {
      let role = params.get("role");
      if (!role) { try { role = localStorage.getItem("mba-role"); } catch (_) {} }
      setRole(role === "staff" ? "staff" : "admin");
    }
    // Hooks: ?state=<name>, ?tab=<tab id>, ?scroll=<px> or ?at=<element id>, ?open=<dialog id>
    // (else <body data-open="dialog id">), ?menu=<button id>.
    setState(live ? document.body.dataset.state : params.get("state"));
    // Static save states: ?state=guardando shows "Guardando…"; any visible invalid field counts as unsaved.
    if (params.get("state")) $$("[hx-patch][aria-invalid=true]").filter((el) => el.offsetParent).forEach((el) =>
      (el.closest(".field-wrap, tr, .status-slot") || el).setAttribute("data-unsaved", ""));
    if (params.get("state") === "guardando") setSave("saving"); else if (unsaved()) settle();
    const wanted = params.get("tab") && document.getElementById(params.get("tab"));
    $$("[role=tablist]").forEach((l) => {
      const s = (wanted && l.contains(wanted) && wanted) || $("[aria-selected=true]", l) || $("[role=tab]", l);
      if (s) selectTab(s);
    });
    const main = $("main.content");
    if (main && params.get("scroll")) main.scrollTop = parseInt(params.get("scroll"), 10) || 0;
    if (main && params.get("at")) { const t = document.getElementById(params.get("at")); if (t) main.scrollTop = t.offsetTop - 24; }
    const dlg = params.get("open") || document.body.dataset.open;
    if (dlg) openDialog(dlg);
    if (params.get("menu")) { const b = document.getElementById(params.get("menu")); if (b) toggleMenu(b); }
  });
})();
