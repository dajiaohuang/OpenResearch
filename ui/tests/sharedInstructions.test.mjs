import assert from "node:assert/strict";
import test from "node:test";
import { act, createElement } from "react";
import { SharedInstructionsSettings } from "../src/components/SettingsPage.tsx";
import { m } from "../src/paraglide/messages.js";
import { find, render, serve, settle, type } from "./render.mjs";

test("external edit keeps the draft when saving conflicts", async () => {
  let sent;
  serve((path, init) => {
    if (path !== "/api/settings/shared-instructions") return undefined;
    if (!init?.method || init.method === "GET") return { content: "original" };
    sent = JSON.parse(init.body);
    return new Response("changed on disk", { status: 409 });
  });
  await render(createElement(SharedInstructionsSettings));
  await settle();
  const input = document.querySelector("textarea");
  assert.equal(input.value, "original");
  await type(input, " draft");
  const draft = input.value;
  await act(async () => find("button", m.common_save()).click());
  await settle();
  assert.deepEqual(sent, { expected: "original", content: draft });
  assert.equal(input.value, draft);
  assert.ok(document.querySelector('[role="alert"]'));
});
