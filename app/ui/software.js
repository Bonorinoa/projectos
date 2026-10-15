/* Explicit local Git staging and content-bound review. No worker or shell UI. */
(function(root) {
  "use strict";
  function createController(document) {
    let bridge, guard, project = null, repository = null, worktree = null, review = null;
    let dialog, nodes, busy = false, generation = 0, returnFocus = null;
    function element(tag, className, text) {
      const node = document.createElement(tag);
      if (className) node.className = className;
      if (text != null) node.textContent = String(text);
      return node;
    }
    function button(text, action) {
      const node = element("button", "", text); node.type = "button";
      node.addEventListener("click", action); return node;
    }
    function fact(list, label, value) {
      list.append(element("dt", "", label), element("dd", "", value == null ? "Not recorded" : value));
    }
    function message(text, error = false) {
      nodes.message.textContent = text;
      nodes.message.classList.toggle("error", error);
    }
    function available() { return Boolean(project?.declared_type === "software" && bridge?.native && typeof bridge.softwareStatus === "function"); }
    function update() {
      nodes.title.textContent = project ? `${project.name || "Project"} · Software` : "Software workspace";
      nodes.refresh.disabled = busy || !available();
      nodes.create.disabled = busy || !available() || !repository || Boolean(worktree);
      nodes.review.disabled = busy || !worktree || typeof bridge?.softwareReview !== "function";
      nodes.export.disabled = busy || !review || typeof bridge?.softwareExportReview !== "function";
      nodes.close.disabled = busy;
      dialog.setAttribute("aria-busy", String(busy));
      nodes.repoFacts.replaceChildren();
      if (repository) {
        fact(nodes.repoFacts, "Repository", repository.root);
        fact(nodes.repoFacts, "Branch", repository.branch || "Detached HEAD");
        fact(nodes.repoFacts, "Current commit", repository.head);
      }
      nodes.repoEmpty.hidden = Boolean(repository);
      nodes.repoEmpty.textContent = !project ? "Open a registered project folder first." : project.declared_type !== "software" ? "This project is not registered as Software." : !bridge?.native ? "Git actions are available in the ProjectOS desktop app." : "Inspect the repository to begin.";
      const dirty = repository?.dirty_files || [];
      nodes.dirtySummary.textContent = dirty.length ? `${dirty.length} changed ${dirty.length === 1 ? "file" : "files"} in your main checkout` : "Main checkout has no reported changes";
      nodes.dirty.hidden = !repository;
      nodes.dirtyFiles.replaceChildren();
      for (const path of dirty) nodes.dirtyFiles.append(element("li", "", path));
      nodes.stageFacts.replaceChildren();
      nodes.stageEmpty.hidden = Boolean(worktree);
      if (worktree) {
        fact(nodes.stageFacts, "Staging folder", worktree.path);
        fact(nodes.stageFacts, "Base commit", worktree.base_head);
        fact(nodes.stageFacts, "Worktree ID", worktree.id);
      }
      nodes.reviewPanel.hidden = !review;
    }
    function clearReview() {
      review = null;
      nodes.reviewFacts.replaceChildren(); nodes.files.replaceChildren(); nodes.notes.replaceChildren();
    }
    function showReview(value) {
      if (!value || value.worktree_id !== worktree?.id || value.base_head !== worktree?.base_head || !value.id || !value.content_hash || !Array.isArray(value.files)) {
        throw new Error("The returned review does not match this worktree. Refresh and review again.");
      }
      review = value;
      nodes.reviewFacts.replaceChildren();
      fact(nodes.reviewFacts, "Review ID", value.id);
      fact(nodes.reviewFacts, "Base commit", value.base_head);
      fact(nodes.reviewFacts, "Content fingerprint", value.content_hash);
      nodes.files.replaceChildren();
      if (!value.files.length) nodes.files.append(element("p", "software-note", "No supported changes to review in this worktree."));
      for (const file of value.files) {
        const detail = element("details", "software-file"); detail.open = value.files.length === 1;
        detail.append(element("summary", "", `${file.status} · ${file.path}`));
        const hashes = element("dl", "software-facts");
        fact(hashes, "Before", file.before_hash || "File absent");
        fact(hashes, "After", file.after_hash || "File absent"); detail.append(hashes);
        const diff = element("pre", "software-diff");
        diff.setAttribute("tabindex", "0"); diff.setAttribute("aria-label", `Patch for ${file.path}`);
        for (const line of String(file.diff || "").split("\n")) {
          const className = line.startsWith("+") && !line.startsWith("+++") ? "software-add" : line.startsWith("-") && !line.startsWith("---") ? "software-remove" : "";
          diff.append(element("span", className, line + "\n"));
        }
        detail.append(diff); nodes.files.append(detail);
      }
      nodes.notes.replaceChildren();
      for (const note of value.capability_notes || []) nodes.notes.append(element("li", "", note));
    }
    async function run(task) {
      if (busy) return false;
      busy = true; update();
      const current = generation;
      try {
        if (typeof guard !== "function" || !await guard()) return false;
        if (current !== generation) return false;
        await task(current); return true;
      } catch (error) {
        if (current === generation) message(error?.message || String(error), true);
        return false;
      } finally { busy = false; update(); }
    }
    async function inspect() {
      if (!available()) return false;
      return run(async current => {
        clearReview(); repository = null; update();
        message("Inspecting the local repository…");
        const result = await bridge.softwareStatus();
        if (current !== generation) return;
        repository = result;
        message(worktree && repository.head !== worktree.base_head ? "The main checkout moved since staging began. Review can inspect staging; export will recheck the base." : "Repository inspected. Create staging explicitly when you are ready.");
      });
    }
    async function createWorktree() {
      if (!available() || !repository || worktree) return false;
      return run(async current => {
        clearReview(); message("Creating an isolated worktree from the current commit…");
        const result = await bridge.softwareCreateWorktree(true);
        if (current !== generation) return;
        worktree = result;
        message("Staging folder created. Make local edits there, then return to review its patch. The staging folder remains on disk after you close this view.");
      });
    }
    async function reviewWorktree() {
      if (!available() || !worktree) return false;
      return run(async current => {
        clearReview(); message("Reading the staging changes and binding this review to their contents…");
        const result = await bridge.softwareReview(worktree.id);
        if (current !== generation) return;
        showReview(result);
        message("Review captured. Inspect every changed file. Export rechecks the reviewed contents.");
      });
    }
    async function exportReview() {
      if (!available() || !review || typeof bridge?.softwareExportReview !== "function") return false;
      return run(async current => {
        const captured = review;
        message("Rechecking the review before choosing a patch destination…");
        try {
          const destination = await bridge.softwareExportReview(captured.id, captured.content_hash);
          if (current === generation) message(destination == null ? "Export cancelled. No patch file was written." : `Reviewed patch exported to ${destination}`);
        } catch (error) { if (current === generation) clearReview(); throw error; }
      });
    }
    function close() {
      if (busy) { message("Finish the current Git operation before closing this view."); return false; }
      if (dialog?.open) dialog.close();
      returnFocus?.focus(); return true;
    }
    function build() {
      dialog = element("dialog", "software-workspace"); dialog.id = "software-workspace";
      dialog.setAttribute("aria-labelledby", "software-title");
      nodes = {};
      const header = element("header", "software-heading");
      nodes.title = element("h2", "", "Software workspace"); nodes.title.id = "software-title";
      nodes.close = button("Close", close); nodes.close.id = "software-close";
      header.append(nodes.title, nodes.close); dialog.append(header);
      dialog.append(element("p", "software-intro", "Inspect your repository, stage changes in a separate folder, and review the resulting patch."));
      nodes.message = element("p", "software-message"); nodes.message.id = "software-message";
      nodes.message.setAttribute("role", "status"); nodes.message.setAttribute("aria-live", "polite");
      dialog.append(nodes.message);
      const columns = element("div", "software-columns");
      const repo = element("section", "software-block");
      repo.append(element("h3", "", "1 · Repository"));
      nodes.repoEmpty = element("p", "software-note"); nodes.repoFacts = element("dl", "software-facts");
      nodes.dirty = element("details", "software-dirty"); nodes.dirtySummary = element("summary"); nodes.dirtyFiles = element("ul");
      nodes.dirty.append(nodes.dirtySummary, nodes.dirtyFiles);
      nodes.refresh = button("Refresh repository", inspect); nodes.refresh.id = "software-refresh";
      repo.append(nodes.repoEmpty, nodes.repoFacts, nodes.dirty, nodes.refresh);
      const staging = element("section", "software-block");
      staging.append(element("h3", "", "2 · Isolated staging"));
      nodes.stageEmpty = element("p", "software-note", "Starts from the current commit. Uncommitted changes stay in your main checkout; copy any intended edits into staging yourself.");
      nodes.stageFacts = element("dl", "software-facts");
      nodes.create = button("Create isolated worktree", createWorktree); nodes.create.id = "software-create";
      nodes.review = button("Refresh patch review", reviewWorktree); nodes.review.id = "software-review";
      const stageActions = element("div", "software-actions"); stageActions.append(nodes.create, nodes.review);
      staging.append(nodes.stageEmpty, nodes.stageFacts, stageActions);
      columns.append(repo, staging); dialog.append(columns);
      nodes.reviewPanel = element("section", "software-block software-review-panel"); nodes.reviewPanel.id = "software-review-panel";
      nodes.reviewPanel.append(element("h3", "", "3 · Reviewed patch"));
      nodes.reviewFacts = element("dl", "software-facts"); nodes.files = element("div", "software-files"); nodes.notes = element("ul", "software-notes");
      nodes.export = button("Export reviewed patch…", exportReview); nodes.export.id = "software-export";
      const apply = button("Apply to main checkout", () => {}); apply.disabled = true;
      apply.setAttribute("aria-describedby", "software-apply-hold");
      const reviewActions = element("div", "software-actions"); reviewActions.append(nodes.export, apply);
      const hold = element("p", "software-note", "Applying to the main checkout awaits integration with ProjectOS recovery. Review and export do not change your main files or create a commit."); hold.id = "software-apply-hold";
      nodes.reviewPanel.append(nodes.reviewFacts, nodes.files, nodes.notes, reviewActions, hold); dialog.append(nodes.reviewPanel);
      const validation = element("section", "software-block software-held"); validation.append(element("h3", "", "Validation · awaiting Gate C"));
      const validationNote = element("p", "software-note", "Tests and supervised app checks are unavailable until the worker and computer-use isolation gate passes. This view records no test or app-check results."); validationNote.id = "software-validation-hold";
      const tests = button("Run tests", () => {}), appCheck = button("Supervised app check", () => {});
      for (const node of [tests, appCheck]) { node.disabled = true; node.setAttribute("aria-describedby", validationNote.id); }
      const validationActions = element("div", "software-actions"); validationActions.append(tests, appCheck);
      validation.append(validationNote, validationActions); dialog.append(validation);
      dialog.addEventListener("cancel", event => { event.preventDefault(); close(); });
      document.body.append(dialog);
    }
    function init(options) {
      bridge = options.bridge; guard = options.onBeforeAction;
      if (!dialog) build();
      if (Object.hasOwn(options, "project")) setProject(options.project);
      update(); return api;
    }
    function setProject(value) {
      const identity = value?.uuid || value?.folder || null;
      const previous = project?.uuid || project?.folder || null;
      project = value;
      if (identity !== previous || !value) {
        generation++; repository = null; worktree = null;
        if (nodes) { clearReview(); message("Project changed. Inspect its repository to begin."); }
      }
      if (nodes) update();
    }
    async function open() {
      if (!dialog || busy) return false;
      if (typeof guard !== "function" || !await guard()) return false;
      returnFocus = document.activeElement;
      update();
      if (!dialog.open) dialog.showModal();
      nodes.close.focus();
      if (available()) await inspect();
      else message(!project ? "Open a project folder before using Software." : project.declared_type !== "software" ? "Software tools are disabled because this project was registered as Substack." : "Open the ProjectOS desktop app to inspect this repository.");
      return true;
    }
    const api = { init, open, close, setProject, isOpen: () => Boolean(dialog?.open), isBusy: () => busy };
    return api;
  }
  if (typeof module !== "undefined" && module.exports) module.exports = { createController };
  if (root.document) root.ProjectOSSoftware = createController(root.document);
})(typeof window === "undefined" ? globalThis : window);
