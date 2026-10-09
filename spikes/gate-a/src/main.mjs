import {
  describeFixtureSafety,
  isAllowedInAppNavigation,
  normalizeUntrustedText,
  renderUntrustedAsText
} from './lib/document-safety.mjs';

const byId = (id) => document.getElementById(id);
const tauriInvoke = window.__TAURI__?.core?.invoke;
let activeRequestId = null;

async function detectRuntime() {
  if (!tauriInvoke) return;
  try {
    const metadata = await tauriInvoke('shell_metadata');
    byId('runtime-signal').classList.add('live');
    byId('runtime-label').textContent = `Tauri ${metadata.tauriVersion}`;
    byId('runtime-detail').textContent = `${metadata.os} · ${metadata.architecture} · 0 privileged commands`;
  } catch (error) {
    byId('runtime-detail').textContent = `IPC error: ${String(error)}`;
  }
}

function showFixture(name, value) {
  const text = normalizeUntrustedText(value);
  renderUntrustedAsText(byId('source-view'), text);
  renderUntrustedAsText(byId('safe-preview'), text);
  const safety = describeFixtureSafety(text);
  byId('document-status').textContent = 'Rendered inertly';
  byId('file-status').textContent = name;
  byId('safety-note').textContent = `${safety.characters} characters · text nodes only · active content unavailable · no remote asset or privileged IPC surface`;
}

async function ingestFile(file) {
  if (!file) return;
  if (file.type === 'application/pdf' || file.name.toLowerCase().endsWith('.pdf')) {
    showFixture(file.name, `[PDF bytes withheld from the WebView]\n\n${file.name}\n${file.size} bytes\n\nUse the separately supplied extracted-text fixture to test PDF text selection. This spike never injects PDF bytes into HTML.`);
    return;
  }
  showFixture(file.name, await file.text());
}

byId('file-input').addEventListener('change', (event) => ingestFile(event.target.files?.[0]));
for (const name of ['dragenter', 'dragover']) {
  byId('drop-zone').addEventListener(name, (event) => {
    event.preventDefault();
    byId('drop-zone').classList.add('dragging');
  });
}
for (const name of ['dragleave', 'drop']) {
  byId('drop-zone').addEventListener(name, (event) => {
    event.preventDefault();
    byId('drop-zone').classList.remove('dragging');
  });
}
byId('drop-zone').addEventListener('drop', (event) => ingestFile(event.dataTransfer?.files?.[0]));

byId('editor').addEventListener('compositionstart', () => {
  byId('ime-log').textContent = 'Composition started…';
  byId('input-status').textContent = 'Composing';
});
byId('editor').addEventListener('compositionupdate', (event) => {
  byId('ime-log').textContent = `Composition update: ${event.data || '(empty)'}`;
});
byId('editor').addEventListener('compositionend', (event) => {
  byId('ime-log').textContent = `Composition committed: ${event.data || '(empty)'}`;
  byId('input-status').textContent = 'IME observed';
});
byId('copy-button').addEventListener('click', async () => {
  await navigator.clipboard.writeText(byId('editor').value);
  byId('input-status').textContent = 'Copied';
});
byId('paste-button').addEventListener('click', async () => {
  const value = await navigator.clipboard.readText();
  byId('editor').setRangeText(value, byId('editor').selectionStart, byId('editor').selectionEnd, 'end');
  byId('input-status').textContent = 'Pasted';
});

byId('start-ipc').addEventListener('click', async () => {
  if (!tauriInvoke || activeRequestId) return;
  activeRequestId = `probe_${Date.now()}`;
  byId('start-ipc').disabled = true;
  byId('cancel-ipc').disabled = false;
  byId('ipc-status').textContent = 'Running';
  byId('ipc-log').textContent = `Active request: ${activeRequestId}`;
  let elapsed = 0;
  const ticker = setInterval(() => {
    elapsed += 1;
    byId('ipc-progress').value = Math.min(elapsed / 2, 100);
  }, 25);
  try {
    const result = await tauriInvoke('cancellable_probe', { requestId: activeRequestId, steps: 200 });
    byId('ipc-status').textContent = result.cancelled ? 'Cancelled' : 'Completed';
    byId('ipc-log').textContent = `${result.requestId}: ${result.completedSteps}/200 steps; cancelled=${result.cancelled}`;
  } catch (error) {
    byId('ipc-status').textContent = 'Failed';
    byId('ipc-log').textContent = String(error);
  } finally {
    clearInterval(ticker);
    byId('ipc-progress').value = 100;
    activeRequestId = null;
    byId('start-ipc').disabled = false;
    byId('cancel-ipc').disabled = true;
  }
});
byId('cancel-ipc').addEventListener('click', async () => {
  if (!tauriInvoke || !activeRequestId) return;
  const accepted = await tauriInvoke('cancel_probe', { requestId: activeRequestId });
  byId('ipc-log').textContent = accepted ? 'Cancellation accepted by Rust.' : 'No active request matched.';
});

const focusable = [...document.querySelectorAll('button, input, textarea, [tabindex="0"]')];
byId('focus-tour').addEventListener('click', () => {
  const next = focusable.find((element) => element !== document.activeElement && !element.disabled);
  next?.focus();
  byId('focus-log').textContent = `Moved focus to: ${next?.getAttribute('aria-label') || next?.textContent?.trim() || next?.id}`;
});

const list = byId('virtual-list');
const windowElement = byId('virtual-window');
const rowHeight = 36;
function renderVirtualRows() {
  const start = Math.max(0, Math.floor(list.scrollTop / rowHeight) - 4);
  const end = Math.min(10_000, start + Math.ceil(list.clientHeight / rowHeight) + 8);
  const fragment = document.createDocumentFragment();
  for (let index = start; index < end; index += 1) {
    const row = document.createElement('div');
    row.className = 'virtual-row';
    row.setAttribute('role', 'listitem');
    row.innerHTML = `<strong>ART-${String(index + 1).padStart(5, '0')}</strong><span>Fixture artifact ${index + 1}</span><span>${index % 3 ? 'Markdown' : 'LaTeX'}</span>`;
    fragment.append(row);
  }
  windowElement.style.transform = `translateY(${start * rowHeight}px)`;
  windowElement.replaceChildren(fragment);
  byId('list-status').textContent = `${start + 1}–${end}`;
}
list.addEventListener('scroll', renderVirtualRows, { passive: true });
renderVirtualRows();

document.addEventListener('click', (event) => {
  const link = event.target.closest?.('a[href]');
  if (link && !isAllowedInAppNavigation(link.getAttribute('href'))) {
    event.preventDefault();
  }
});
window.addEventListener('dragover', (event) => event.preventDefault());
window.addEventListener('drop', (event) => event.preventDefault());

showFixture('built-in.html', '<script>window.__TAURI__.core.invoke("shell", {})</script>\n<img src="https://attacker.invalid/beacon">\n<a href="https://attacker.invalid">leave app</a>');
detectRuntime();
