/* Local drafts and explicit, reviewed OpenRouter requests over native IPC. */
"use strict";
(() => {
  const api = window.projectos, invariants = window.ProjectOSEditorState;
  const el = id => document.getElementById(id);
  let workspace = null, doc = null, status = { configured: false, online: false, busy: false, unresolved: false };
  let operationBusy = false, inFlight = false, proposal = null, proposalHash = null, prepared = null, preparedHash = null;
  let unsavedResolver = null, streamAttempt = null, folderCreate = false;
  const text = el("draft-text");
  function dirty() { return invariants.isDirty(doc, text.value); }
  function message(value, error = false) { el("app-status").textContent = String(value); el("app-status").classList.toggle("error", error); }
  function requestOptions() {
    return invariants.requestOptions(el("provider-model").value, el("max-tokens").value, el("ceiling-usd").value);
  }
  function update() {
    const busy = operationBusy || inFlight || status.busy;
    el("workspace-name").textContent = workspace ? workspace.name : "No project open";
    el("folder-label").textContent = workspace ? workspace.folder : "Open an existing project, or register a folder as a new project.";
    for (const id of ["open-project", "register-project"]) el(id).disabled = !api.native || busy;
    for (const id of ["close-project", "create-draft", "draft-path", "draft-list", "refresh-project", "milestone-name", "save-milestone"]) el(id).disabled = !workspace || busy;
    text.disabled = !invariants.canEdit(doc, operationBusy);
    for (const id of ["browse-folder", "submit-folder"]) el(id).disabled = !api.native || busy;
    el("workspace-folder").disabled = operationBusy;
    el("save-draft").disabled = !doc || !dirty() || busy;
    for (const id of ["reload-draft", "export-draft"]) el(id).disabled = !doc || busy;
    el("preview-toggle").disabled = !doc;
    el("provider-settings").disabled = !api.native || operationBusy;
    el("store-key").disabled = !api.native || busy || !el("api-key").value.trim();
    el("forget-key").disabled = !api.native || busy || !status.configured;
    el("online-toggle").disabled = !api.native || operationBusy;
    el("online-toggle").checked = Boolean(status.online);
    el("provider-badge").textContent = inFlight ? "Request running" : status.online ? "Online · approval required" : "Offline";
    el("key-state").textContent = status.configured ? "A key is stored in the operating system credential store." : "No stored API key.";
    el("draft-title").textContent = doc ? doc.rel_path : "Your writing workbench";
    let draftState = doc ? (dirty() ? "Unsaved changes · save before requesting AI" : "Saved · exact file contents are used for AI scope") : "Local editing works offline. AI requests require your approval.";
    if (doc && invariants.lineEndingInfo(doc.text).mixed && dirty()) draftState += " · Saving edits normalizes mixed line endings to this file's predominant style";
    el("draft-state").textContent = draftState;
    let reason = "Select text for a passage action. Review draft sends the whole saved draft.";
    if (!doc) reason = "Open or create a draft first.";
    else if (dirty()) reason = "Save your changes before preparing a request.";
    else if (!status.configured) reason = "Store your OpenRouter API key in settings.";
    else if (!status.online) reason = "Online access is off. Enable it in OpenRouter settings when you are ready.";
    else if (!el("provider-model").value.trim()) reason = "Enter your exact OpenRouter model ID in settings.";
    else if (status.unresolved) reason = "A prior request has an unknown outcome. Open Activity and acknowledge it before another request.";
    else if (busy) reason = "A request or file operation is in progress.";
    const ready = Boolean(doc && !dirty() && status.configured && status.online && !status.unresolved && !busy && el("provider-model").value.trim() && !proposal);
    document.querySelectorAll("[data-action]").forEach(button => { button.disabled = !ready || (button.dataset.action !== "ReviewDraft" && text.selectionStart === text.selectionEnd); });
    if (proposal) reason = "Accept or reject the current proposal before preparing another request.";
    el("action-availability").textContent = reason;
    el("selection-state").textContent = doc ? `${text.selectionEnd - text.selectionStart} characters selected` : "Select text for an action, or review the entire saved draft.";
    const valid = invariants.canApply(proposal, doc, text.value, proposalHash);
    el("accept-proposal").disabled = !valid || busy;
    el("reject-proposal").disabled = !proposal || busy;
    const proposalWarning = proposal && !valid ? "The draft changed since this request. Acceptance is disabled. Reject this proposal and prepare a fresh request using the current saved text." : proposal?.annotation ? "Accept saves a linked annotation. It leaves your draft text unchanged. Check any factual claims independently." : proposal ? "Accept replaces only the originally selected passage, after a final native check of the saved file." : "";
    el("proposal-warning").textContent = [proposal?.truncated ? "Output token limit reached. This result may be incomplete. Review it carefully, or reject it and prepare a smaller scope or larger allowed token limit." : "", proposalWarning].filter(Boolean).join(" ");
  }
  async function run(task) {
    if (operationBusy) return;
    operationBusy = true; update();
    try { return await task(); } catch (error) { message(error?.message || String(error), true); }
    finally { operationBusy = false; update(); }
  }
  async function refreshStatus() { status = await api.invoke("provider_status"); update(); }
  function loadDocument(value) {
    doc = value; text.value = invariants.editorText(value?.text || ""); proposal = null; proposalHash = null;
    el("proposal-panel").hidden = true; el("draft-list").value = value?.rel_path || "";
    renderPreview(); update();
  }
  async function save() {
    if (!doc) return false;
    const displayedSnapshot = text.value, savedText = invariants.diskText(doc, displayedSnapshot);
    const result = await api.invoke("draft_save", { relPath: doc.rel_path, expectedHash: doc.hash, text: savedText });
    doc = result; // Do not overwrite text typed while the native save was pending.
    await api.invoke("workspace_refresh"); renderPreview(); update();
    message(text.value === displayedSnapshot ? "Draft saved. Previous contents retained for recovery." : "Draft saved; additional typing is still unsaved.");
    return !dirty();
  }
  async function guardChanges() {
    if (inFlight || status.busy) { message("Cancel or finish the current request first.", true); return false; }
    if (proposal) { message("Accept or reject the current proposal before switching files or leaving this draft.", true); return false; }
    if (!dirty()) return true;
    if (unsavedResolver) return false;
    const decision = await new Promise(resolve => { unsavedResolver = resolve; el("unsaved-dialog").showModal(); });
    if (decision === "cancel") return false;
    if (decision === "discard") { text.value = doc.text; renderPreview(); update(); return true; }
    return await save();
  }
  function resolveUnsaved(decision) {
    el("unsaved-dialog").close(); const resolver = unsavedResolver; unsavedResolver = null; resolver?.(decision);
  }
  el("unsaved-save").onclick = () => resolveUnsaved("save");
  el("unsaved-discard").onclick = () => resolveUnsaved("discard");
  el("unsaved-cancel").onclick = () => resolveUnsaved("cancel");
  el("unsaved-dialog").addEventListener("cancel", e => { e.preventDefault(); resolveUnsaved("cancel"); });
  function setWorkspace(value) {
    workspace = value;
    const list = el("draft-list"), previous = doc?.rel_path; list.replaceChildren();
    for (const path of value?.documents || []) { const option = document.createElement("option"); option.value = path; option.textContent = path; list.appendChild(option); }
    if (previous) list.value = previous;
    update();
  }
  window.addEventListener("workspace-changed", e => setWorkspace(e.detail));
  async function openWorkspace(create, folder) {
    if (!await guardChanges()) return;
    const value = await api.invoke("workspace_open", { create, ...(folder ? { folder } : {}) });
    if (value) { el("folder-dialog").close(); loadDocument(null); message(create ? "Project registered and opened. Create or select a draft." : "Project opened. Choose a draft to begin."); }
  }
  function chooseFolder(create) {
    folderCreate = create; el("workspace-folder").value = "";
    el("folder-dialog-title").textContent = create ? "Register a new project folder" : "Open project folder";
    el("folder-dialog-description").textContent = create ? "Choose an existing folder to register as a ProjectOS project. Paste its absolute path or browse on this computer." : "Choose an existing project folder. Paste its absolute path or browse on this computer.";
    el("submit-folder").textContent = create ? "Register project" : "Open folder";
    el("folder-dialog").showModal(); el("workspace-folder").focus();
  }
  el("open-project").onclick = () => chooseFolder(false);
  el("register-project").onclick = () => chooseFolder(true);
  el("cancel-folder").onclick = () => el("folder-dialog").close();
  el("browse-folder").onclick = () => run(() => openWorkspace(folderCreate));
  el("folder-form").onsubmit = e => {
    e.preventDefault(); const folder = el("workspace-folder").value.trim();
    if (!folder) { message("Paste an absolute folder path, or use Browse.", true); return; }
    run(() => openWorkspace(folderCreate, folder));
  };
  el("close-project").onclick = () => run(async () => { if (await guardChanges()) { await api.invoke("workspace_close"); loadDocument(null); message("Project closed."); } });
  el("draft-list").onchange = () => {
    const path = el("draft-list").value;
    run(async () => {
      if (!await guardChanges()) { el("draft-list").value = doc?.rel_path || ""; return; }
      loadDocument(await api.invoke("draft_open", { relPath: path })); message("Draft loaded from disk.");
    });
  };
  el("create-draft").onclick = () => run(async () => {
    const path = el("draft-path").value.trim();
    if (!path) throw new Error("Enter a relative .md or .txt file path.");
    if (!await guardChanges()) return;
    const created = await api.invoke("draft_create", { relPath: path });
    await api.invoke("workspace_refresh"); loadDocument(created); el("draft-path").value = ""; message("Draft created. Start writing and save your work.");
  });
  el("refresh-project").onclick = () => run(async () => { await api.invoke("workspace_refresh"); await refreshStatus(); message("Project file list and saved activity refreshed. Your editor text is unchanged."); });
  el("save-draft").onclick = () => run(save);
  el("reload-draft").onclick = () => run(async () => { if (await guardChanges()) { loadDocument(await api.invoke("draft_open", { relPath: doc.rel_path })); message("Draft reloaded from disk."); } });
  el("export-draft").onclick = () => run(async () => { if (await guardChanges()) { await api.invoke("draft_export", { relPath: doc.rel_path }); message("Export dialog closed. The export uses the saved draft on disk."); } });
  el("save-milestone").onclick = () => run(async () => {
    const name = el("milestone-name").value.trim(); if (!name) throw new Error("Give this milestone a name.");
    if (!await guardChanges()) return;
    await api.invoke("draft_milestone", { name }); await api.invoke("workspace_refresh"); el("milestone-name").value = ""; message("Milestone saved.");
  });
  text.addEventListener("input", () => { renderPreview(); update(); });
  for (const event of ["select", "keyup", "click"]) text.addEventListener(event, update);
  document.addEventListener("selectionchange", () => { if (document.activeElement === text) update(); });
  document.addEventListener("keydown", e => {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s" && doc) { e.preventDefault(); if (!operationBusy && !inFlight && !status.busy && dirty()) run(save); }
  });
  // Restricted Markdown preview: text nodes only, no HTML, links, images or fetch.
  function renderPreview() {
    const preview = el("draft-preview"); preview.replaceChildren();
    let code = null;
    for (const line of text.value.split("\n")) {
      if (/^\s*```/.test(line)) {
        if (code) code = null;
        else { code = document.createElement("pre"); preview.appendChild(code); }
        continue;
      }
      if (code) { code.appendChild(document.createTextNode(line + "\n")); continue; }
      const heading = line.match(/^(#{1,6})\s+(.+)$/);
      const node = document.createElement(heading ? "h" + heading[1].length : "p");
      node.textContent = heading ? heading[2] : line || "\u00a0"; preview.appendChild(node);
    }
  }
  el("preview-toggle").onclick = () => { const on = el("draft-preview").hidden; el("draft-preview").hidden = !on; el("preview-toggle").setAttribute("aria-pressed", String(on)); renderPreview(); };
  el("provider-settings").onclick = () => { el("provider-dialog").showModal(); run(refreshStatus); };
  el("provider-form").onsubmit = e => { e.preventDefault(); el("api-key").value = ""; el("provider-dialog").close(); update(); };
  el("provider-dialog").addEventListener("close", () => { el("api-key").value = ""; update(); });
  el("api-key").oninput = update; el("provider-model").oninput = update;
  el("store-key").onclick = () => {
    // Clear before awaiting so failures cannot leave credentials sitting in the UI.
    const apiKey = el("api-key").value.trim(); el("api-key").value = "";
    if (!apiKey) return;
    run(async () => { await api.invoke("provider_store_key", { apiKey }); await refreshStatus(); message("OpenRouter key stored by the native app."); });
  };
  el("forget-key").onclick = () => run(async () => { await api.invoke("provider_forget_key"); await refreshStatus(); message("Stored OpenRouter key removed."); });
  el("online-toggle").onchange = () => {
    const online = el("online-toggle").checked;
    run(async () => { status = await api.invoke("provider_set_online", { online }); message(online ? "Online access enabled for this session. Each request needs Send approval." : "Offline. In-flight work is cancelled and nothing will be replayed."); });
  };
  document.querySelectorAll("[data-action]").forEach(button => button.onclick = () => run(async () => {
    const action = button.dataset.action;
    if (!doc || dirty()) throw new Error("Save your draft before requesting AI.");
    if (proposal) throw new Error("Accept or reject the existing proposal first.");
    const options = requestOptions();
    const range = invariants.byteSelection(doc.text, text.selectionStart, text.selectionEnd, action === "ReviewDraft");
    const savedHash = doc.hash, path = doc.rel_path;
    message("Preparing the exact scope and checking current OpenRouter pricing…");
    const result = await api.invoke("action_prepare", { relPath: path, expectedHash: savedHash, ...range, action, ...options });
    if (!doc || doc.rel_path !== path || doc.hash !== savedHash || dirty()) throw new Error("The draft changed during preparation. Save it and prepare a fresh request.");
    prepared = result; preparedHash = savedHash;
    el("approval-kind").textContent = `${result.action} · ${result.annotation ? "Save a separate annotation" : "Propose a replacement"} · ${result.rel_path}`;
    el("approval-text").textContent = result.selected_text;
    const facts = [["Destination", result.endpoint], ["Requested model", result.requested_model], ["Output token limit", result.max_tokens], ["Estimated upper cost", usd(result.estimated_upper_usd)], ["Your cost ceiling", usd(result.ceiling_usd)]];
    el("approval-facts").replaceChildren();
    for (const [label, value] of facts) { const dt = document.createElement("dt"), dd = document.createElement("dd"); dt.textContent = label; dd.textContent = String(value); el("approval-facts").append(dt, dd); }
    el("approval-dialog").showModal(); message("Request prepared. Review the scope before Send.");
  }));
  function usd(value) { return value == null ? "Unknown" : "$" + Number(value).toFixed(6); }
  el("dismiss-approval").onclick = () => { prepared = null; preparedHash = null; el("approval-dialog").close(); message("Prepared request dismissed. No request sent."); };
  el("approval-dialog").addEventListener("cancel", () => { prepared = null; preparedHash = null; message("Prepared request dismissed. No request sent."); });
  el("send-action").onclick = async () => {
    if (!prepared || inFlight || operationBusy) return;
    if (!doc || dirty() || doc.hash !== preparedHash || doc.rel_path !== prepared.rel_path) { message("Draft changed. Prepare a fresh request.", true); prepared = null; el("approval-dialog").close(); return; }
    const scope = prepared, savedHash = preparedHash; prepared = null; preparedHash = null;
    el("approval-dialog").close(); inFlight = true; streamAttempt = null; el("request-progress").hidden = false; el("request-progress").querySelector("span").textContent = "Waiting for OpenRouter…"; update();
    try {
      const result = await api.invoke("action_execute", { preparedId: scope.id });
      proposal = result; proposalHash = savedHash;
      el("proposal-panel").hidden = false; el("proposal-heading").textContent = `${result.action} ${result.annotation ? "annotation" : "proposal"}`;
      el("proposed-label").textContent = result.annotation ? "Separate annotation" : "Suggested replacement";
      el("proposal-original").textContent = result.original;
      el("proposal-text").textContent = result.proposed;
      el("accept-proposal").textContent = result.annotation ? "Accept annotation" : "Accept replacement";
      el("proposal-facts").textContent = `${result.requested_model}${result.resolved_model ? " → " + result.resolved_model : ""} · ${result.usage_tokens ?? "Unknown"} tokens · provider cost ${usd(result.provider_cost_usd)} · ${result.cost_state || "unknown cost"}`;
      message("Proposal received. Review it before accepting. Your draft has not been changed.");
    } catch (error) { if (!proposal) el("proposal-panel").hidden = true; message(error?.message || String(error), true); }
    finally {
      inFlight = false; el("request-progress").hidden = true;
      try { await api.invoke("workspace_refresh"); await refreshStatus(); } catch (error) { message(error?.message || String(error), true); }
      update();
    }
  };
  el("cancel-action").onclick = () => run(async () => { await api.invoke("action_cancel"); message("Cancellation requested. Check Activity for the recorded outcome and any uncertain remote cost."); });
  async function decide(accept) {
    if (!proposal) return;
    if (accept && !invariants.canApply(proposal, doc, text.value, proposalHash)) throw new Error("Draft changed. Reject this proposal and prepare a fresh request.");
    const path = doc?.rel_path, annotation = proposal.annotation;
    await api.invoke("action_decide", { proposalId: proposal.id, accept });
    proposal = null; proposalHash = null; el("proposal-panel").hidden = true;
    if (accept && !annotation) loadDocument(await api.invoke("draft_open", { relPath: path }));
    message(accept ? annotation ? "Annotation saved as a linked artifact. Draft text unchanged." : "Replacement accepted and saved. Previous contents retained for recovery." : "Proposal rejected. Draft text unchanged.");
  }
  el("accept-proposal").onclick = () => run(() => decide(true));
  el("reject-proposal").onclick = () => run(() => decide(false));
  window.addEventListener("open-draft-artifact", e => run(async () => {
    if (!await guardChanges()) return;
    loadDocument(await api.invoke("draft_open", { relPath: e.detail }));
    window.projectosViews.setView("editor"); message("Draft loaded from disk.");
  }));
  window.addEventListener("open-annotation-artifact", e => run(async () => {
    const annotation = await api.invoke("artifact_annotation", { annotationId: e.detail.id });
    el("annotation-title").textContent = e.detail.label;
    el("annotation-text").textContent = annotation;
    el("annotation-dialog").showModal();
    message("Saved annotation opened. Draft text unchanged.");
  }));
  el("close-annotation").onclick = () => el("annotation-dialog").close();
  window.addEventListener("reconcile-attempt", e => run(async () => {
    await api.invoke("provider_reconcile", { attemptId: e.detail }); await api.invoke("workspace_refresh"); await refreshStatus();
    message("Unresolved request acknowledged. No request repeated. Actual provider billing may still be unknown.");
  }));
  window.addEventListener("beforeunload", e => { if (invariants.needsCloseGuard(doc, text.value, proposal, inFlight, operationBusy)) { e.preventDefault(); e.returnValue = ""; } });
  async function startup() {
    update();
    if (!api.native) { message("Browser preview only. Open ProjectOS desktop to edit project files and connect OpenRouter."); return; }
    try {
      await refreshStatus();
      await api.listenProgress(payload => {
        if (!inFlight || !payload || typeof payload.text !== "string") return;
        if (streamAttempt && streamAttempt !== payload.attempt_id) return;
        streamAttempt = payload.attempt_id;
        el("proposal-panel").hidden = false; el("proposal-heading").textContent = "Receiving preview · not ready to accept";
        el("proposal-original").textContent = ""; el("proposal-text").textContent = payload.text;
        el("proposal-facts").textContent = "Request in progress. Usage and provider cost are not yet known.";
      });
      const currentWindow = window.__TAURI__?.window?.getCurrentWindow?.();
      if (currentWindow?.onCloseRequested) await currentWindow.onCloseRequested(async event => {
        if (!invariants.needsCloseGuard(doc, text.value, proposal, inFlight, operationBusy)) return;
        event.preventDefault();
        if (operationBusy) { message("Finish the current file operation before closing.", true); return; }
        await run(async () => { if (await guardChanges()) { doc = null; await currentWindow.destroy(); } });
      });
      message("Open a project to start. Online access is off until you enable it.");
    } catch (error) { message(error?.message || String(error), true); }
  }
  startup();
})();
