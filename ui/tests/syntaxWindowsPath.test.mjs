import test from 'node:test';
import assert from 'node:assert/strict';
import { createElement } from 'react';
import { detectSyntaxLanguageFromFilePath } from '../src/syntaxLanguage.ts';
import { parsePane } from '../src/workspaceState.ts';
import { CodeView } from '../src/components/CodeView.tsx';
import { render } from './render.mjs';

test('detects filenames and extensions in Windows drive paths', () => {
  assert.equal(detectSyntaxLanguageFromFilePath(String.raw`C:\research\Makefile`), 'makefile');
  assert.equal(detectSyntaxLanguageFromFilePath(String.raw`C:\research.v2\README`), null);
  assert.equal(detectSyntaxLanguageFromFilePath(String.raw`C:\research\script.PY`), 'python');
  assert.equal(detectSyntaxLanguageFromFilePath('C:/research/Makefile'), 'makefile');
});

test('preserves POSIX filename and extension behavior', () => {
  assert.equal(detectSyntaxLanguageFromFilePath('/research/Makefile'), 'makefile');
  assert.equal(detectSyntaxLanguageFromFilePath('/research.v2/README'), null);
  assert.equal(detectSyntaxLanguageFromFilePath(String.raw`/research/name\Makefile`), null);
  assert.equal(detectSyntaxLanguageFromFilePath('/research/script.PY'), 'python');
});

test('renders syntax for a restored Windows Makefile pane', async () => {
  const path = String.raw`C:\research\Makefile`;
  const pane = parsePane({ kind: 'file', path, source: 'abs' });
  assert.equal(pane.path, path);
  const view = await render(createElement(CodeView, { path: pane.path, text: 'all: build\n' }));
  assert.ok(view.container.querySelectorAll('.token').length > 0);
});
