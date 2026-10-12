/* Shared editor invariants; also executable under node for behavioral tests. */
(function(root) {
  "use strict";
  function editorText(raw) { return raw.replace(/\r\n?/g, "\n"); }
  function lineEndingInfo(raw) {
    const endings = raw.match(/\r\n|\r|\n/g) || [];
    const counts = new Map();
    for (const ending of endings) counts.set(ending, (counts.get(ending) || 0) + 1);
    let chosen = "\n", most = 0;
    // Equal counts preserve the first encountered style.
    for (const [ending, count] of counts) if (count > most) { chosen = ending; most = count; }
    return { ending: chosen, mixed: counts.size > 1 };
  }
  function diskText(doc, displayedText) {
    const normalized = editorText(displayedText);
    // Preserve every byte of unchanged mixed-ending files.
    if (normalized === editorText(doc.text)) return doc.text;
    return normalized.replace(/\n/g, lineEndingInfo(doc.text).ending);
  }
  function rawOffset(raw, displayOffset) {
    let offset = 0;
    for (let seen = 0; seen < displayOffset; seen++) {
      offset += raw[offset] === "\r" && raw[offset + 1] === "\n" ? 2 : 1;
    }
    return offset;
  }
  // Positions come from a textarea (normalized LF, UTF-16). Scopes refer to
  // original disk bytes, including retained CRLF/CR line endings.
  function byteSelection(text, start, end, whole = false) {
    if (whole) return { byteStart: 0, byteEnd: new TextEncoder().encode(text).length };
    const displayed = editorText(text);
    if (!Number.isInteger(start) || !Number.isInteger(end) || start < 0 || end < start || end > displayed.length) throw new Error("Invalid text selection.");
    for (const offset of [start, end]) {
      const prev = displayed.charCodeAt(offset - 1), next = displayed.charCodeAt(offset);
      if (prev >= 0xd800 && prev <= 0xdbff && next >= 0xdc00 && next <= 0xdfff) throw new Error("Select complete characters, including the whole emoji.");
    }
    if (start === end) throw new Error("Select the passage you want to work on first.");
    const encoder = new TextEncoder();
    return { byteStart: encoder.encode(text.slice(0, rawOffset(text, start))).length, byteEnd: encoder.encode(text.slice(0, rawOffset(text, end))).length };
  }
  function isDirty(doc, text) { return Boolean(doc && editorText(doc.text) !== editorText(text)); }
  function needsCloseGuard(doc, text, proposal, inFlight, operationBusy) {
    return isDirty(doc, text) || Boolean(proposal || inFlight || operationBusy);
  }
  function needsReconciliation(receipt) { return Boolean(receipt.requires_reconciliation); }
  function canApply(proposal, doc, text, savedHash) {
    return Boolean(proposal && doc && proposal.rel_path === doc.rel_path && doc.hash === savedHash && !isDirty(doc, text));
  }
  function requestOptions(modelValue, tokenValue, ceilingValue) {
    const model = String(modelValue).trim(), maxTokens = Number(tokenValue), ceilingUsd = Number(ceilingValue);
    if (!model) throw new Error("Enter an exact OpenRouter model ID in settings first.");
    if (!Number.isInteger(maxTokens) || maxTokens < 1 || maxTokens > 8192) throw new Error("Maximum output tokens must be a whole number from 1 to 8192.");
    if (String(ceilingValue).trim() === "" || !Number.isFinite(ceilingUsd) || ceilingUsd < 0 || ceilingUsd > 10) throw new Error("Set a dollar ceiling from 0 to 10 USD. Zero allows only free routes.");
    return { model, maxTokens, ceilingUsd };
  }
  function canEdit(doc, operationBusy) { return Boolean(doc && !operationBusy); }
  const api = { byteSelection, isDirty, canApply, requestOptions, canEdit, editorText, diskText, lineEndingInfo, needsCloseGuard, needsReconciliation };
  if (typeof module !== "undefined" && module.exports) module.exports = api;
  root.ProjectOSEditorState = api;
})(typeof window === "undefined" ? globalThis : window);
