/* ProjectOS — Synodos Field app shell.
 *
 * Canvas engine + a11y list-equivalent, driven by the mock bridge
 * (window.projectos from bridge.js). No network, no CDN. Fonts load via
 * relative paths in index.html.
 *
 * Canvas (Synodos Field): typed artifact nodes rendered with the 12×12 pixel
 * sprites from design/sketches/007-canvas-synodos-field, dashed typed edges,
 * port-to-port linking (out-port ■ → in-port ■), lens filters
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
  const parts = (SPRITES[type] || []).map(([c, x, y, w, h]) =>
    `<rect x="${x}" y="${y}" width="${w}" height="${h}" fill="${c}"/>`).join("");
  return `<svg width="${size}" height="${size}" viewBox="0 0 12 12" shape-rendering="crispEdges" aria-hidden="true">${parts}</svg>`;
}

/* ---- data comes from the bridge; canvas and list share the SAME records ---- */
const bridge = window.projectos;
let OBJECTS = [];
let EDGES = [];
const byId = {};
let NODES = [];            // {obj, el}
let NODE_ORDER = [];       // ids in stable order (drives keyboard nav)

const world = document.getElementById("world");
const svg = document.getElementById("svg");
const insp = document.getElementById("insp");
let z = 1, sel = null, drag = null, linkC = null, lens = "all";
let focusIdx = 0;

/* ---- build the canvas -------------------------------------------------- */
function build() {
  OBJECTS = bridge.objects();
  EDGES = bridge.edges();
  OBJECTS.forEach((n) => { byId[n.id] = n; });

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
      `<span class="badge">${n.kind.toUpperCase()}</span>` +
      `<span class="sub">${(n.sub || "")}</span></div>` +
      `<div class="tt">${n.label}</div>` +
      `<div class="meta">${meta}</div>` +
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
      drag = { el, dx: ev.clientX - r.left, dy: ev.clientY - r.top, w: r.width / 2, h: r.height / 2 };
    });
    // port-to-port linking: out → in
    el.querySelector(".p-out").addEventListener("mousedown", (ev) => {
      ev.stopPropagation();
      const r = el.getBoundingClientRect();
      linkC = { f: n.id, ex: r.right - 6, ey: r.top + 16, rel: relKind() };
      window.addEventListener("mousemove", lineMove);
      window.addEventListener("mouseup", lineUp);
    });
    el.querySelector(".p-in").addEventListener("mouseup", (ev) => {
      ev.stopPropagation();
      if (linkC && linkC.f !== n.id) {
        EDGES.push({ from: linkC.f, to: n.id, rel: linkC.rel });
      }
      linkC = null;
      window.removeEventListener("mousemove", lineMove);
      window.removeEventListener("mouseup", lineUp);
      paint();
    });
    el.addEventListener("dblclick", () => { sel = n.id; showInsp(); });
    el.addEventListener("focus", () => { focusIdx = NODE_ORDER.indexOf(n.id); });
    el.addEventListener("keydown", nodeKey);
  });

  buildList();
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
function relKind() { return document.getElementById("relkind").value || "LINKS_TO"; }

function lineMove(ev) {
  const r = world.getBoundingClientRect();
  svg.innerHTML = `<line x1="${linkC.ex}" y1="${linkC.ey}" x2="${ev.clientX - r.left}"` +
    ` y2="${ev.clientY - r.top}" stroke-dasharray="2 3" stroke="#2a3a38"></line>`;
}
function lineUp() { linkC = null; svg.innerHTML = ""; paint(); }

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
      `<text class="rel" x="${(ax + bx) / 2}" y="${(ay + by) / 2}">${rel.rel}</text>`;
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
  document.getElementById("i-type").textContent = `${n.kind} · typed artifact`;
  const f = document.getElementById("i-fields");
  f.innerHTML = "";
  Object.entries(n.fields).forEach(([k, v]) => {
    f.insertAdjacentHTML("beforeend", `<tr><td class="k">${k}</td><td>${v}</td></tr>`);
  });
  const rels = relFor(sel);
  document.getElementById("i-rels").innerHTML =
    rels.length
      ? rels.map((r) => r.from === sel
          ? `<span class="arrow">→</span> ${r.rel} · ${byId[r.to] ? byId[r.to].label : r.to}`
          : `<span class="arrow">←</span> ${r.rel} · ${byId[r.from] ? byId[r.from].label : r.from}`).join("<br>")
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

/* canvas drag + link follow mouse */
document.addEventListener("mouseup", () => { drag = null; });
document.getElementById("cv").addEventListener("mousemove", (ev) => {
  if (drag) {
    drag.el.style.left = (ev.clientX - drag.dx + drag.w) + "px";
    drag.el.style.top = (ev.clientY - drag.dy + drag.h) + "px";
    drag.obj.x = drag.el.style.left; drag.obj.y = drag.el.style.top;
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
      r.from === n.id ? `<b>→</b> ${r.rel} → ${byId[r.to] ? byId[r.to].label : r.to}`
                      : `<b>←</b> ${r.rel} ← ${byId[r.from] ? byId[r.from].label : r.from}`)
      .join(" · ");
    const fieldsTxt = Object.entries(n.fields).map(([k, v]) => `${k}: ${v}`).join(" · ");
    tr.innerHTML =
      `<td class="td-type">${n.kind}</td>` +
      `<td class="td-kind"><span class="dot" style="background:${typeColor(n.kind)}"></span>${n.label}</td>` +
      `<td class="lrowkey">${fieldsTxt}</td>` +
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
  listFocus = i;
  listRows[i].focus();
}
function openFromList(id) { sel = id; showInsp(); }

/* toggle list <-> canvas (button AND keyboard shortcut "A") */
function setListMode(on) {
  listMode = on;
  listpanel.hidden = !on;
  document.getElementById("cv").hidden = on;
  document.getElementById("listtoggle").classList.toggle("on", on);
  document.getElementById("listtoggle").setAttribute("aria-pressed", String(on));
  if (on) focusListRow(0);
  else world.querySelectorAll(".node")[0] && world.querySelectorAll(".node")[0].focus();
}
document.getElementById("listtoggle").addEventListener("click", () => setListMode(!listMode));
document.getElementById("inspclose").addEventListener("click", closeInsp);

/* global keyboard shortcuts (no text inputs run, so bare keys are safe) */
document.addEventListener("keydown", (ev) => {
  if (ev.key === "a" || ev.key === "A") { ev.preventDefault(); setListMode(!listMode); }
  if (ev.key === "Escape") closeInsp();
});

build();
