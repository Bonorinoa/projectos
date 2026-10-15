/* Native IPC only. Ordinary browser previews never impersonate a project. */
"use strict";
window.projectos = (() => {
  let workspace = null;
  const native = Boolean(window.__TAURI__?.core?.invoke);
  async function invoke(command, args = {}) {
    if (!native) throw new Error("Open ProjectOS desktop to use local files and OpenRouter. This browser preview has no native connection.");
    const result = await window.__TAURI__.core.invoke(command, args);
    if (["workspace_open", "workspace_refresh", "action_decide"].includes(command) && result) setWorkspace(result);
    if (command === "workspace_close") setWorkspace(null);
    return result;
  }
  function setWorkspace(value) {
    workspace = value;
    window.dispatchEvent(new CustomEvent("workspace-changed", { detail: value }));
  }
  return {
    bridge: native ? "tauri" : "unavailable", native, invoke,
    workspace: () => workspace,
    objects: () => workspace?.objects || [],
    edges: () => workspace?.edges || [],
    receipts: () => workspace?.receipts || [],
    softwareStatus: () => invoke("software_status"),
    softwareCreateWorktree: confirmed => invoke("software_create_worktree", { confirmed }),
    softwareReview: worktreeId => invoke("software_review", { worktreeId }),
    softwareExportReview: (reviewId, contentHash) => invoke("software_export_review", { reviewId, contentHash }),
    async listenClose(callback) {
      if (native && window.__TAURI__?.event?.listen) return window.__TAURI__.event.listen("app-close-requested", callback);
      return () => {};
    },
    async listenProgress(callback) {
      if (native && window.__TAURI__?.event?.listen) return window.__TAURI__.event.listen("provider-progress", e => callback(e.payload));
      return () => {};
    }
  };
})();
