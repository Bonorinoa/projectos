/* ProjectOS — Synodos Field app shell.
 *
 * Canvas engine + a11y list-equivalent, driven by the native bridge
 * (window.projectos from bridge.js). No network, no CDN. Fonts load via
 * relative paths in index.html.
 *
 * Canvas (Synodos Field): typed artifact nodes rendered with the 12×12 pixel
 * sprites from design/sketches/007-canvas-synodos-field, dashed typed edges,
 * native saved relationships, lens filters
 * (All/Lineage/Evidence/Workflow), zoom, drag, and a typed inspector.
 *
 * A11y list-equivalent: the SAME records as a semantic <table>, toggled by a
 * button and by the keyboard shortcut "A". Full keyboard path:
 *   Tab   - move focus among nodes / into the table
 *   Arrows- move focus to the previous/next record
 *   Enter - open the inspector for the focused artifact
 *   Esc   - close the inspector / leave the list
 * Reduced motion is respected via CSS (prefers-reduced-motion).
 */
"use strict";

/* ---- 12×12 pixel sprites (integer-grid rects, crispEdges) ----
 * Rect data reused verbatim from design/sketches/007-canvas-synodos-field.
 * Each entry: [fill, x, y, w, h] on a 12×12 grid. */
const SPRITES = {
  Draft:[[ "#c79a52",3,1,6,9],["#2a3a38",3,1,1,9],["#2a3a38",8,1,1,9],["#2a3a38",3,1,6,1],["#2a3a38",3,9,6,1],["#2a3a38",5,3,3,1],["#2a3a38",5,5,2,1],["#2a3a38",5,7,3,1]],
  Receipt:[[ "#7f8b86",3,1,6,8],["#2a3a38",3,1,6,1],["#2a3a38",3,1,1,10],["#2a3a38",8,1,1,10],["#2a3a38",4,10,1,1],["#2a3a38",6,10,1,1],["#2a3a38",5,9,1,2],["#2a3a38",7,9,1,2],["#2a3a38",5,4,2,1],["#2a3a38",5,6,2,1]],
  Evidence:[[ "#2a3a38",2,11,8,1],["#5f7d6a",3,6,2,5],["#5f7d6a",6,3,2,8],["#5f7d6a",9,8,2,3],["#2a3a38",3,6,2,1],["#2a3a38",6,3,2,1],["#2a3a38",9,8,2,1]],
  Scope:[[ "#2a3a38",2,2,8,1],["#2a3a38",2,9,8,1],["#2a3a38",2,2,1,8],["#2a3a38",9,2,1,8],["#5f6f92",5,5,2,2],["#2a3a38",5,1,2,1],["#2a3a38",5,10,2,1],["#2a3a38",1,5,1,2],["#2a3a38",10,5,1,2]],
  Recipe:[[ "#b45c3c",3,3,2,2],["#b45c3c",7,3,2,2],["#b45c3c",3,7,2,2],["#b45c3c",7,7,2,2],["#2a3a38",6,1,1,10],["#2a3a38",1,6,10,1],["#2a3a38",5,5,3,3]],
  Revision:[[ "#2a3a38",2,4,2,2],["#2a3a38",5,4,2,2],["#2a3a38",8,4,1,2],["#2a3a38",3,8,2,2],["#2a3a38",6,8,2,2],["#8a6a3a",4,6,4,1],["#8a6a3a",7,5,1,3]]
};
function sprite(type, size) {
  const parts = (Array.isArray(SPRITES[type]) ? SPRITES[type] : []).map(([c, x, y, w, h]) =>
    `<rect x="${x}" y="${y}" width="${w}" height="${h}" fill="${c}"/>`).join("");
  return `<svg width="${size}" height="${size}" viewBox="0 0 12 12" shape-rendering="crispEdges" aria-hidden="true">${parts}</svg>`;
}

/* ---- data comes from the bridge; canvas and list share the SAME records ---- */
const bridge = window.projectos;
let OBJECTS = [];
let EDGES = [];
const byId = Object.create(null);
let NODES = [];            // {obj, el}
let NODE_ORDER = [];       // ids in stable order (drives keyboard nav)

const world = document.getElementById("world");
const svg = document.getElementById("svg");
const insp = document.getElementById("insp");
let z = 1, sel = null, drag = null, lens = "all";
let focusIdx = 0;

/* ---- build the canvas -------------------------------------------------- */
function build() {
  OBJECTS = bridge.objects();
  document.getElementById("empty-field").hidden = OBJECTS.length > 0;
  EDGES = bridge.edges();
  Object.keys(byId).forEach(k => delete byId[k]);
  OBJECTS.forEach((n, i) => { n.fields ||= {}; n.x = Number.isFinite(n.x) ? n.x : 170 + (i % 3) * 290; n.y = Number.isFinite(n.y) ? n.y : 150 + Math.floor(i / 3) * 170; byId[n.id] = n; });

  world.innerHTML = "";
  NODES = [];
  NODE_ORDER = OBJECTS.map((n) => n.id);

  OBJECTS.forEach((n) => {
    const el = document.createElement("div");
    el.className = "node";
    el.dataset.id = n.id;
    el.tabIndex = 0;
    el.setAttribute("role", "button");
    el.setAttribute("aria-label", `${n.kind} artifact: ${n.label}`);
    el.style.left = n.x + "px";
    el.style.top = n.y + "px";
    const firstField = Object.entries(n.fields)[0];
    const meta = firstField ? `${firstField[0]} = ${firstField[1]}` : "";
    el.innerHTML =
      `<div class="hd"><span class="sp">${sprite(n.kind, 16)}</span>` +
      `<span class="badge">${escapeHtml(String(n.kind).toUpperCase())}</span>` +
      `<span class="sub">${escapeHtml(n.sub || "")}</span></div>` +
      `<div class="tt">${escapeHtml(n.label)}</div>` +
      `<div class="meta">${escapeHtml(meta)}</div>` +
      `<div class="ports"><span class="port p-in" title="in-port" aria-hidden="true"></span>` +
      `<span class="port p-out" title="out-port" aria-hidden="true"></span></div>`;
    world.appendChild(el);
    n._el = el;
    NODES.push({ obj: n, el });

    // drag to move
    el.addEventListener("mousedown", (ev) => {
      if (ev.target.classList.contains("port")) return;
      sel = n.id; focusIdx = NODE_ORDER.indexOf(n.id);
      const r = el.getBoundingClientRect();
      drag = { el, obj: n, dx: ev.clientX - r.left, dy: ev.clientY - r.top, w: r.width / 2, h: r.height / 2 };
    });
    // Relationships come from saved native records; ports are decorative.
    el.addEventListener("dblclick", () => { sel = n.id; showInsp(); });
    el.addEventListener("focus", () => { focusIdx = NODE_ORDER.indexOf(n.id); });
    el.addEventListener("keydown", nodeKey);
  });

  buildList();
  buildTrace();
  paint();
}

function nodeKey(ev) {
  const cur = focusIdx;
  switch (ev.key) {
    case "ArrowRight": case "ArrowDown":
      ev.preventDefault(); setFocus((cur + 1) % NODE_ORDER.length); break;
    case "ArrowLeft": case "ArrowUp":
      ev.preventDefault(); setFocus((cur - 1 + NODE_ORDER.length) % NODE_ORDER.length); break;
    case "Enter":
      ev.preventDefault(); sel = NODE_ORDER[cur]; showInsp(); break;
    case "Escape":
      ev.preventDefault(); closeInsp(); break;
  }
}
function setFocus(i) {
  focusIdx = i;
  const id = NODE_ORDER[i];
  if (id && byId[id] && byId[id]._el) {
    sel = id;
    byId[id]._el.focus();
    showInspFor(id);
  }
}

/* ---- edge rendering ---------------------------------------------------- */
function relFor(id) { return EDGES.filter((r) => r.from === id || r.to === id); }

function visible(id) {
  if (lens === "all") return true;
  const n = byId[id];
  if (!n) return false;
  if (lens === "evidence") return n.kind === "Evidence";
  if (lens === "workflow") return ["Recipe", "Receipt", "Revision"].includes(n.kind);
  // lineage: connected artifacts, excluding pure receipts unless they are a Draft
  const connected = relFor(id).length > 0;
  return connected && n.kind !== "Receipt" || n.kind === "Draft";
}

function paint() {
  NODES.forEach(({ obj, el }) => {
    const v = visible(obj.id);
    el.style.display = v ? "" : "none";
  });
  paintLines();
}

function paintLines() {
  svg.innerHTML = "";
  const r = world.getBoundingClientRect();
  EDGES.forEach((rel) => {
    const a = byId[rel.from], b = byId[rel.to];
    const ae = a && a._el, be = b && b._el;
    if (!ae || !be || ae.style.display === "none" || be.style.display === "none") return;
    const ar = ae.getBoundingClientRect(), br = be.getBoundingClientRect();
    const ax = ar.right - r.left, ay = ar.top - r.top + 16;
    const bx = br.left - r.left, by = br.top - r.top + 16;
    svg.innerHTML +=
      `<line x1="${ax}" y1="${ay}" x2="${bx}" y2="${by}"></line>` +
      `<text class="rel" x="${(ax + bx) / 2}" y="${(ay + by) / 2}">${escapeHtml(rel.rel)}</text>`;
  });
}

/* ---- inspector --------------------------------------------------------- */
function showInspFor(id) {
  sel = id;
  showInsp();
}
function showInsp() {
  const n = byId[sel];
  if (!n) return;
  insp.classList.add("open");
  insp.setAttribute("aria-hidden", "false");
  document.getElementById("i-sprite").innerHTML = sprite(n.kind, 34);
  document.getElementById("i-title").textContent = n.label;
  document.getElementById("i-type").textContent = `${n.kind} · saved artifact`;
  const f = document.getElementById("i-fields");
  f.innerHTML = "";
  Object.entries(n.fields).forEach(([k, v]) => {
    f.insertAdjacentHTML("beforeend", `<tr><td class="k">${escapeHtml(k)}</td><td>${escapeHtml(v)}</td></tr>`);
  });
  insp.querySelector(".artifact-open")?.remove();
  if (n.fields.annotation_id || (n.kind === "Draft" && n.fields.path)) {
    const button = document.createElement("button"); button.className = "artifact-open";
    button.textContent = n.fields.annotation_id ? "Read saved annotation" : "Open in draft editor";
    button.addEventListener("click", () => window.dispatchEvent(new CustomEvent(n.fields.annotation_id ? "open-annotation-artifact" : "open-draft-artifact", {
      detail: n.fields.annotation_id ? { id: n.fields.annotation_id, label: n.label } : n.fields.path
    })));
    insp.appendChild(button);
  }
  const rels = relFor(sel);
  document.getElementById("i-rels").innerHTML =
    rels.length
      ? rels.map((r) => r.from === sel
          ? `<span class="arrow">→</span> ${escapeHtml(r.rel)} · ${escapeHtml(byId[r.to] ? byId[r.to].label : r.to)}`
          : `<span class="arrow">←</span> ${escapeHtml(r.rel)} · ${escapeHtml(byId[r.from] ? byId[r.from].label : r.from)}`).join("<br>")
      : "(no relations)";
}
function closeInsp() {
  insp.classList.remove("open");
  insp.setAttribute("aria-hidden", "true");
}

/* ---- lens + zoom ------------------------------------------------------- */
function setLens(l) {
  lens = l;
  document.querySelectorAll(".lens button").forEach((b) => {
    const on = b.dataset.l === l;
    b.classList.toggle("on", on);
    b.setAttribute("aria-pressed", String(on));
  });
  paint();
}
document.querySelectorAll(".lens button").forEach((b) => {
  b.addEventListener("click", () => setLens(b.dataset.l));
});
function zoom(delta) {
  z = Math.min(2, Math.max(0.5, z * delta));
  world.style.transform = `scale(${z})`;
  paintLines();
}
document.getElementById("zin").addEventListener("click", () => zoom(1.15));
document.getElementById("zout").addEventListener("click", () => zoom(1 / 1.15));

/* Temporary canvas arrangement; saved graph relationships are read-only here. */
document.addEventListener("mouseup", () => { drag = null; });
document.getElementById("cv").addEventListener("mousemove", (ev) => {
  if (drag) {
    const r = document.getElementById("cv").getBoundingClientRect();
    drag.obj.x = (ev.clientX - r.left - drag.dx + drag.w) / z;
    drag.obj.y = (ev.clientY - r.top - drag.dy + drag.h) / z;
    drag.el.style.left = drag.obj.x + "px"; drag.el.style.top = drag.obj.y + "px";
  }
  paintLines();
});

/* ---- A11Y LIST-EQUIVALENT (semantic table, same records) ---------------- */
const listpanel = document.getElementById("listpanel");
const listbody = document.getElementById("listbody");
let listMode = false;
let listRows = [];

function buildList() {
  listbody.innerHTML = "";
  listRows = [];
  OBJECTS.forEach((n) => {
    const tr = document.createElement("tr");
    tr.tabIndex = 0;
    tr.dataset.id = n.id;
    tr.setAttribute("aria-label", `${n.kind} artifact: ${n.label}`);
    const rels = relFor(n.id).map((r) =>
      r.from === n.id ? `<b>→</b> ${escapeHtml(r.rel)} → ${escapeHtml(byId[r.to] ? byId[r.to].label : r.to)}`
                      : `<b>←</b> ${escapeHtml(r.rel)} ← ${escapeHtml(byId[r.from] ? byId[r.from].label : r.from)}`)
      .join(" · ");
    const fieldsTxt = Object.entries(n.fields).map(([k, v]) => `${k}: ${v}`).join(" · ");
    tr.innerHTML =
      `<td class="td-type">${escapeHtml(n.kind)}</td>` +
      `<td class="td-kind"><span class="dot" style="background:${typeColor(n.kind)}"></span>${escapeHtml(n.label)}</td>` +
      `<td class="lrowkey">${escapeHtml(fieldsTxt)}</td>` +
      `<td class="td-rel">${rels || "<span class=\"lrowkey\">(no relations)</span>"}</td>`;
    tr.addEventListener("focus", () => { listFocus = listRows.indexOf(tr); });
    tr.addEventListener("keydown", listKey);
    tr.addEventListener("dblclick", () => openFromList(n.id));
    listbody.appendChild(tr);
    listRows.push(tr);
  });
}
function typeColor(kind) {
  return { Draft: "#c79a52", Receipt: "#7f8b86", Evidence: "#5f7d6a",
           Scope: "#5f6f92", Recipe: "#b45c3c", Revision: "#8a6a3a" }[kind] || "#888";
}
let listFocus = 0;
function listKey(ev) {
  switch (ev.key) {
    case "ArrowDown": case "ArrowRight":
      ev.preventDefault(); focusListRow((listFocus + 1) % listRows.length); break;
    case "ArrowUp": case "ArrowLeft":
      ev.preventDefault(); focusListRow((listFocus - 1 + listRows.length) % listRows.length); break;
    case "Enter":
      ev.preventDefault(); openFromList(listRows[listFocus].dataset.id); break;
    case "Escape":
      ev.preventDefault(); closeInsp(); break;
  }
}
function focusListRow(i) {
  if (!listRows.length) return;
  listFocus = i;
  listRows[i].focus();
}
function openFromList(id) { sel = id; showInsp(); }

/* toggle views: exactly one of canvas | list | trace is visible at a time */
let view = "editor";
function setView(v) {
  view = v;
  document.getElementById("editor-panel").hidden = v !== "editor";
  document.getElementById("editor-toggle").classList.toggle("on", v === "editor");
  document.getElementById("editor-toggle").setAttribute("aria-pressed", String(v === "editor"));
  document.getElementById("canvas-toggle").classList.toggle("on", v === "canvas");
  document.getElementById("canvas-toggle").setAttribute("aria-pressed", String(v === "canvas"));
  document.querySelector(".zoom").hidden = v !== "canvas";
  document.querySelector(".hint").hidden = v !== "canvas";
  if (v === "editor") { closeInsp(); closeTraceDetail(); }
  document.getElementById("cv").hidden = v !== "canvas";
  document.getElementById("listpanel").hidden = v !== "list";
  document.getElementById("tracepanel").hidden = v !== "trace";
  const listOn = v === "list";
  document.getElementById("listtoggle").classList.toggle("on", listOn);
  document.getElementById("listtoggle").setAttribute("aria-pressed", String(listOn));
  const traceOn = v === "trace";
  document.getElementById("tracetoggle").classList.toggle("on", traceOn);
  document.getElementById("tracetoggle").setAttribute("aria-pressed", String(traceOn));
  if (v === "list") focusListRow(0);
  else if (v === "trace") { closeTraceDetail(); focusTraceRow(0); }
  else if (v === "canvas") { closeInsp(); closeTraceDetail(); world.querySelectorAll(".node")[0] && world.querySelectorAll(".node")[0].focus(); }
}
document.getElementById("listtoggle").addEventListener("click", () => setView(view === "list" ? "canvas" : "list"));
document.getElementById("tracetoggle").addEventListener("click", () => setView(view === "trace" ? "canvas" : "trace"));
document.getElementById("inspclose").addEventListener("click", closeInsp);

/* ---- TRACE VIEWER (redacted receipt facts) ------------------------------ */
const tracebody = document.getElementById("tracebody");
const tracecount = document.getElementById("tracecount");
const tracedetail = document.getElementById("tracedetail");
let RECEIPTS = [];
let traceFilter = "all";
let traceRows = [];
let traceFocus = 0;

function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
}
function fmtAt(secs) {
  const d = new Date(secs * 1000);
  const p = (n) => String(n).padStart(2, "0");
  return `${d.getUTCFullYear()}-${p(d.getUTCMonth() + 1)}-${p(d.getUTCDate())} ${p(d.getUTCHours())}:${p(d.getUTCMinutes())}:${p(d.getUTCSeconds())}`;
}
// Denial outcomes (kernel words them 'read not granted' / 'write not granted').
function isDenied(r) { return /not granted|refused/.test(r.outcome); }
function traceShown(r) {
  if (traceFilter === "all") return true;
  if (traceFilter === "unknown") return window.ProjectOSEditorState.needsReconciliation(r) || r.outcome === "outcome-unknown";
  return isDenied(r); // "denied"
}
function outcomeClass(r) {
  if (isDenied(r)) return "denied";
  if (window.ProjectOSEditorState.needsReconciliation(r) || r.outcome === "outcome-unknown") return "unknown";
  return "ok";
}
function buildTrace() {
  // Same facts + exact field names as the kernel's RedactedReceipt, via bridge.
  RECEIPTS = bridge.receipts();
  renderTrace();
}
function renderTrace() {
  tracebody.innerHTML = "";
  traceRows = [];
  const shown = RECEIPTS.filter(traceShown);
  shown.forEach((r) => {
    const op = r.op || r.operation || "Activity";
    const relPath = r.rel_path || (Array.isArray(r.changed_paths) && r.changed_paths.length ? r.changed_paths.join(", ") : "Project");
    const tr = document.createElement("tr");
    tr.tabIndex = 0;
    tr.dataset.id = r.id;
    tr.setAttribute("aria-label", `${op} on ${relPath}: ${r.outcome}`);
    const cr = [r.requested_model || r.route, r.resolved_model ? "resolved: " + r.resolved_model : null, r.usage_tokens != null ? r.usage_tokens + " tokens" : null, r.provider_cost_usd != null ? "$" + r.provider_cost_usd + " provider cost" : null, r.billed_usd != null ? "$" + r.billed_usd + " billed" : null, r.cost_state || r.cost, Array.isArray(r.capability_scope) ? r.capability_scope.join(", ") : null].filter(Boolean).join(" · ");
    const receiptHash = r.content_hash || r.review_hash;
    const path = `<span class="td-path">${escapeHtml(relPath)}</span>` +
      (receiptHash ? `<span class="td-hash"> · #${escapeHtml(receiptHash)}</span>` : "");
    tr.innerHTML =
      `<td>${fmtAt(r.at)}</td>` +
      `<td class="td-op">${escapeHtml(op)}</td>` +
      `<td>${path}</td>` +
      `<td><span class="td-outcome ${outcomeClass(r)}">${escapeHtml(r.outcome)}</span></td>` +
      `<td class="td-cr">${cr ? escapeHtml(cr) : "—"}</td>`;
    tr.addEventListener("focus", () => { traceFocus = traceRows.indexOf(tr); });
    tr.addEventListener("keydown", traceKey);
    tr.addEventListener("dblclick", () => openTraceDetail(r, tr));
    tracebody.appendChild(tr);
    traceRows.push(tr);
  });
  tracecount.textContent = `${shown.length} of ${RECEIPTS.length} receipts shown`;
}
function traceKey(ev) {
  switch (ev.key) {
    case "ArrowDown": case "ArrowRight":
      ev.preventDefault(); focusTraceRow((traceFocus + 1) % traceRows.length); break;
    case "ArrowUp": case "ArrowLeft":
      ev.preventDefault(); focusTraceRow((traceFocus - 1 + traceRows.length) % traceRows.length); break;
    case "Enter":
      ev.preventDefault(); openTraceDetail(RECEIPTS.find((r) => r.id === traceRows[traceFocus].dataset.id), traceRows[traceFocus]); break;
    case "Escape":
      ev.preventDefault(); closeTraceDetail(); break;
  }
}
function focusTraceRow(i) {
  if (!traceRows.length) return;
  traceFocus = i;
  traceRows[i].focus();
}
function openTraceDetail(r, tr) {
  if (!r) return;
  document.getElementById("td-op").textContent = `${r.op || r.operation || "Activity"} · ${r.rel_path || (r.changed_paths || []).join(", ") || "Project"}`;
  const fields = Object.entries(r).map(([k, v]) => [k.replaceAll("_", " "), v == null ? "Unknown / not recorded" : typeof v === "object" ? JSON.stringify(v) : v]);
  const t = document.querySelector(".td-fields");
  t.innerHTML = fields.map(([k, v]) => `<tr><td class="k">${escapeHtml(k)}</td><td>${escapeHtml(v)}</td></tr>`).join("");
  tracedetail.querySelector(".reconcile-button")?.remove();
  if (window.ProjectOSEditorState.needsReconciliation(r) && r.attempt_id) {
    const reconcile = document.createElement("button"); reconcile.className = "reconcile-button"; reconcile.textContent = "Acknowledge unresolved request / unknown billing";
    reconcile.addEventListener("click", () => window.dispatchEvent(new CustomEvent("reconcile-attempt", { detail: r.attempt_id })));
    tracedetail.appendChild(reconcile);
  }
  tracedetail.classList.add("open");
  tracedetail.setAttribute("aria-hidden", "false");
  document.getElementById("tracedetailclose").focus();
}
function closeTraceDetail() {
  tracedetail.classList.remove("open");
  tracedetail.setAttribute("aria-hidden", "true");
}
document.querySelectorAll(".tradefilter button").forEach((b) => {
  b.addEventListener("click", () => {
    traceFilter = b.dataset.traceFilter;
    document.querySelectorAll(".tradefilter button").forEach((x) => {
      const on = x === b;
      x.classList.toggle("on", on);
      x.setAttribute("aria-pressed", String(on));
    });
    renderTrace();
    closeTraceDetail();
  });
});
document.getElementById("tracedetailclose").addEventListener("click", closeTraceDetail);

/* global keyboard shortcuts (no text inputs run, so bare keys are safe) */
document.addEventListener("keydown", (ev) => {
  if (ev.target.closest("input, textarea, select, dialog") || ev.ctrlKey || ev.metaKey || ev.altKey) return;
  if (ev.key === "a" || ev.key === "A") { ev.preventDefault(); setView(view === "list" ? "canvas" : "list"); }
  if (ev.key === "t" || ev.key === "T") { ev.preventDefault(); setView(view === "trace" ? "canvas" : "trace"); }
  if (ev.key === "Escape") { closeInsp(); closeTraceDetail(); }
});

function fitView() {
  if (!OBJECTS.length) return;
  const panel = document.getElementById("cv");
  const maxX = Math.max(...OBJECTS.map(n => n.x + 160));
  const maxY = Math.max(...OBJECTS.map(n => n.y + 100));
  z = Math.max(.2, Math.min(1.5, (panel.clientWidth - 20) / maxX, (panel.clientHeight - 20) / maxY));
  world.style.transform = `scale(${z})`; paintLines();
}
document.getElementById("fit-view").addEventListener("click", fitView);
document.getElementById("editor-toggle").addEventListener("click", () => setView("editor"));
document.getElementById("canvas-toggle").addEventListener("click", () => { setView("canvas"); fitView(); });
window.addEventListener("workspace-changed", () => { closeInsp(); closeTraceDetail(); build(); if (view === "canvas") fitView(); });
window.projectosViews = { setView };
build(); setView("editor");
