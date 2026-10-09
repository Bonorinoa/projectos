const EXTERNAL_SCHEMES = new Set([
  'data:',
  'file:',
  'ftp:',
  'http:',
  'https:',
  'javascript:',
  'mailto:',
  'tel:',
  'vscode:'
]);

export function normalizeUntrustedText(value) {
  return String(value ?? '')
    .replace(/\r\n?/g, '\n')
    .replaceAll('\u0000', '\uFFFD');
}

/**
 * Gate A deliberately renders hostile documents as text, not HTML. This is a
 * bounded feasibility policy, not a future rich Markdown renderer.
 */
export function renderUntrustedAsText(target, value, createTextNode = null) {
  if (!target || typeof target.replaceChildren !== 'function') {
    throw new TypeError('target must implement replaceChildren');
  }
  const factory = createTextNode ?? ((text) => document.createTextNode(text));
  target.replaceChildren(factory(normalizeUntrustedText(value)));
}

export function isAllowedInAppNavigation(rawValue) {
  const value = String(rawValue ?? '').trim();
  if (value === '' || value.startsWith('#')) return true;
  try {
    const url = new URL(value, 'tauri://localhost/');
    if (EXTERNAL_SCHEMES.has(url.protocol)) return false;
    return url.protocol === 'tauri:' && url.hostname === 'localhost';
  } catch {
    return false;
  }
}

export function describeFixtureSafety(value) {
  const normalized = normalizeUntrustedText(value);
  return {
    renderer: 'text-node-only',
    characters: [...normalized].length,
    containsMarkupLikeText: /<[^>]+>/.test(normalized),
    activeContentExecuted: false,
    remoteAssetsRequested: false,
    privilegedIpcAvailable: false
  };
}
