/* ProjectOS — mock kernel bridge (app shell only, NO network).
 *
 * Replaces the Tauri invoke layer so app/ui runs standalone in a browser.
 * The surface mirrors what the real native kernel exposes so a later swap to
 * `window.__TAURI__.core.invoke` does not change app.js:
 *   - listProjects()  -> known projects (registration stub)
 *   - objects()       -> typed artifact objects (id, kind, label, fields, x, y)
 *   - edges()         -> typed relations (from, to, rel)
 *   - receipts()      -> redacted receipt facts
 *   - register()      -> project_register stub
 *   - open()          -> project_open stub
 *
 * Data here is typed so the canvas and the a11y list expose the SAME records.
 */
window.projectos = (() => {
  // Typed artifact objects. `kind` is one of the six artifact types in the
  // locked vocabulary; `fields` hold the typed fields shown in the inspector.
  const objects = [
    { id: "d1", kind: "Draft",  label: "The economics of attention, in twelfths",
      x: 330, y: 210,
      fields: { anchor: "argument locked", milestone: "09c1", state: "accepted rev 0017", stale: "false" } },
    { id: "r1", kind: "Receipt", label: "Receipt · Rephrase 0017",
      x: 730, y: 150,
      fields: { route: "openrouter/deepseek-v4.1-flash", cost: "$0.0023", verdict: "accepted", outcome: "revision created" } },
    { id: "g1", kind: "Evidence", label: "Lineage — 3 revisions, 1 anchor",
      x: 1130, y: 130,
      fields: { dataset: "lineage-map", n: "3", contradiction: "0" } },
    { id: "s1", kind: "Scope", label: "Premise & promise",
      x: 1180, y: 450,
      fields: { promise: "measurable units mislead", audience: "applied economists" } },
    { id: "r2", kind: "Recipe", label: "Structural review — block D",
      x: 510, y: 480,
      fields: { blocks: "argument|evidence|flow", status: "approved", budget: "$0.05" } },
    { id: "e1", kind: "Evidence", label: "γ = 0.41 [0.33, 0.50]",
      x: 910, y: 580,
      fields: { dataset: "SCHWARTZ-2019", n: "58", contradiction: "false" } },
    { id: "v1", kind: "Revision", label: "rev 0017 · 9c02",
      x: 150, y: 540,
      fields: { before: "1a3f", after: "9c02", status: "committed" } }
  ];

  // Typed edges — dashed connectors labelled with the relation kind.
  const edges = [
    { from: "d1", to: "r1", rel: "PRODUCED_BY" },
    { from: "r1", to: "g1", rel: "SUPPORTS" },
    { from: "d1", to: "g1", rel: "CITES" },
    { from: "d1", to: "s1", rel: "GOVERNED_BY" },
    { from: "d1", to: "r2", rel: "EXECUTED" },
    { from: "r2", to: "e1", rel: "PRODUCED" },
    { from: "v1", to: "d1", rel: "REVISION_OF" },
    { from: "r2", to: "v1", rel: "APPLIED_TO" }
  ];

  const receipts = [
    { id: "rec-0017", at: 1728420000, op: "rephrase", rel_path: "draft.md", outcome: "accepted", cost: "$0.0023" },
    { id: "rec-0018", at: 1728420060, op: "append",   rel_path: "draft.md", outcome: "revision created", cost: "$0.0012" }
  ];

  const projects = [
    { uuid: "11111111-1111-4111-8111-111111111111", folder: "/tmp/projectos-demo", declaredType: "publication", sidecarVersion: 1 }
  ];

  return {
    bridge: "mock",           // marks the mock; a real bridge reports "tauri"
    listProjects() { return projects; },
    objects() { return objects; },
    edges() { return edges; },
    receipts() { return receipts; },
    // Command stubs mirror the Tauri commands project_register / project_open.
    register(folder, declaredType) {
      return { ok: true, folder, declaredType, note: "register stub (mock bridge — no network)" };
    },
    open(folder) {
      return { ok: true, folder, uuid: "11111111-1111-4111-8111-111111111111", note: "open stub (mock bridge — no network)" };
    }
  };
})();
