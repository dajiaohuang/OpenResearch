import assert from "node:assert/strict";
import test from "node:test";
import { createElement, useEffect, useRef, useState } from "react";
import { FileFind } from "../src/components/FileFind.tsx";
import { m } from "../src/paraglide/messages.js";
import { press, render, settle, type } from "./render.mjs";

function Fixture({ editor }) {
  const target = useRef(null);
  const [ready, setReady] = useState(false);
  useEffect(() => setReady(true), []);
  return createElement("div", { ref: target },
    editor ? createElement("textarea", { defaultValue: "alpha Beta alpha", "aria-label": "file" }) :
      createElement("div", { "data-file-search-content": true }, "alpha ", createElement("strong", null, "Be"), "ta alpha"),
    ready && createElement(FileFind, { target, editor, revision: "alpha Beta alpha", request: 1, onClose() {} }));
}

test("find selects a match in an editable file", async () => {
  const view = await render(createElement(Fixture, { editor: true }));
  await settle();
  const field = view.container.querySelector(`input[aria-label="${m.pdf_preview_find()}"]`);
  await type(field, "beta");
  await press("Enter", field);
  const editor = view.container.querySelector("textarea");
  assert.equal(editor.value.slice(editor.selectionStart, editor.selectionEnd), "Beta");
  await view.unmount();
});

test("find crosses markup within a read-only file", async () => {
  const view = await render(createElement(Fixture, { editor: false }));
  await settle();
  const field = view.container.querySelector(`input[aria-label="${m.pdf_preview_find()}"]`);
  await type(field, "beta");
  await press("Enter", field);
  assert.equal(window.getSelection().toString(), "Beta");
  await view.unmount();
});

test("F3 navigates file matches and wraps", async () => {
  const view = await render(createElement(Fixture, { editor: true }));
  await settle();
  const field = view.container.querySelector(`input[aria-label="${m.pdf_preview_find()}"]`);
  const editor = view.container.querySelector("textarea");
  await type(field, "alpha");
  await press("Enter", field);
  assert.equal(editor.selectionStart, 0);
  await press("F3", editor);
  assert.equal(editor.selectionStart, 11);
  await press("F3", editor);
  assert.equal(editor.selectionStart, 0);
  await press("F3", editor, { shiftKey: true });
  assert.equal(editor.selectionStart, 11);
  await view.unmount();
});

test("no matches cannot navigate", async () => {
  const view = await render(createElement(Fixture, { editor: true }));
  await settle();
  const field = view.container.querySelector(`input[aria-label="${m.pdf_preview_find()}"]`);
  await type(field, "missing");
  assert.match(view.container.querySelector('[aria-live="polite"]').textContent, /0\s*\/\s*0/);
  assert.equal(view.container.querySelector(`button[aria-label="${m.pdf_preview_next_match()}"]`).disabled, true);
  await view.unmount();
});

test("search treats punctuation literally", async () => {
  const view = await render(createElement(Fixture, { editor: true }));
  await settle();
  const field = view.container.querySelector(`input[aria-label="${m.pdf_preview_find()}"]`);
  await type(field, ".*");
  assert.match(view.container.querySelector('[aria-live="polite"]').textContent, /0\s*\/\s*0/);
  await view.unmount();
});
