import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  describeFixtureSafety,
  isAllowedInAppNavigation,
  normalizeUntrustedText,
  renderUntrustedAsText
} from '../src/lib/document-safety.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const fixtureDirectory = path.join(here, '..', 'fixtures', 'hostile');

test('all hostile fixtures use the text-only rendering boundary', () => {
  const fixtures = fs.readdirSync(fixtureDirectory).sort();
  assert.deepEqual(fixtures, [
    'active-content.html',
    'active-content.md',
    'active-content.pdf',
    'active-content.tex',
    'extracted-pdf-text.txt'
  ]);

  for (const fixture of fixtures) {
    const original = fs.readFileSync(path.join(fixtureDirectory, fixture), 'utf8');
    const target = {
      children: [],
      replaceChildren(...children) { this.children = children; }
    };
    const createTextNode = (data) => ({ nodeType: 3, data });
    renderUntrustedAsText(target, original, createTextNode);
    assert.equal(target.children.length, 1, fixture);
    assert.equal(target.children[0].nodeType, 3, fixture);
    assert.equal(target.children[0].data, normalizeUntrustedText(original), fixture);
    assert.match(describeFixtureSafety(original).renderer, /^text-node-only$/);
  }
});

test('navigation policy rejects external and executable schemes', () => {
  for (const value of [
    'https://attacker.invalid',
    'http://attacker.invalid',
    '//attacker.invalid/path',
    'javascript:alert(1)',
    'data:text/html,<script>alert(1)</script>',
    'file:///etc/passwd',
    'mailto:someone@example.com'
  ]) {
    assert.equal(isAllowedInAppNavigation(value), false, value);
  }
  assert.equal(isAllowedInAppNavigation('#local-section'), true);
  assert.equal(isAllowedInAppNavigation(''), true);
});

test('normalization preserves visible attack text and removes null bytes', () => {
  assert.equal(normalizeUntrustedText('a\r\nb\rc\u0000d'), 'a\nb\nc\uFFFDd');
});
