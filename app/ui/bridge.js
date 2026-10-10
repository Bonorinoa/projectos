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

  // Redacted receipt facts. Field names mirror the real kernel's
  // `RedactedReceipt` exactly (kernel/src/receipts.rs) so the mock → native
  // swap is mechanical: this mirrors `projectos_kernel::receipts::ReceiptWriter::receipts()`,
  // surfaced to the UI by the bridge as `receipts()` on `window.projectos`.
  //   id, at (u64 unix secs), op, rel_path (never absolute), outcome,
  //   content_hash (Option), route (Option), cost (Option).
  // Exercise every column and all three trace filters: >=2 denials
  // ('read not granted' / 'write not granted'), one 'outcome-unknown',
  // one absolute-looking path already redacted to relative ('etc/passwd'),
  // and one with route + cost set.
  const receipts = [
    { id: "a4f17c9e2b1d0033", at: 1728420000, op: "rephrase",  rel_path: "draft.md",                   outcome: "accepted",            content_hash: "a3f9c21bd0", route: "openrouter/deepseek-v4.1-flash", cost: "$0.0023" },
    { id: "7f2a1b8c40e9d5b1", at: 1728420060, op: "append",    rel_path: "draft.md",                   outcome: "revision created",    content_hash: "9c02aa7f",   route: "openrouter/deepseek-v4.1-flash", cost: "$0.0012" },
    { id: "c81e55f2a9b07c6d", at: 1728420120, op: "read",      rel_path: "notes/research/private.md", outcome: "read not granted",    content_hash: null,         route: null, cost: null },
    { id: "3d0a9b7e51f2c8d4", at: 1728420180, op: "write",     rel_path: "etc/passwd",                 outcome: "write not granted",   content_hash: null,         route: null, cost: null },
    { id: "b6e2fa91c3d4057a", at: 1728420240, op: "stat",      rel_path: "var/log/system.log",         outcome: "outcome-unknown",     content_hash: null,         route: null, cost: null },
    { id: "91c4d8a35b60e2f7", at: 1728420300, op: "summarize", rel_path: "draft.md",                   outcome: "accepted",            content_hash: "f9d20c4b",   route: "local/llama3", cost: "$0.0000" },
    { id: "05f3e7b24a81d6c9", at: 1728420360, op: "delete",    rel_path: "tmp/scratch.txt",            outcome: "read not granted",    content_hash: null,         route: null, cost: null },
    { id: "d7a1c6b03f92e45d", at: 1728420420, op: "index",     rel_path: "notes/reading-list.md",      outcome: "indexed 12 entries",  content_hash: "8b1e07a2",   route: "local", cost: "$0.0000" },
    { id: "2e8b5c1f60a47d9b", at: 1728420480, op: "transcribe",rel_path: "interviews/session-04.wav",  outcome: "read not granted",    content_hash: null,         route: null, cost: null }
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
