import assert from "node:assert/strict";
import test from "node:test";
import { act, createElement } from "react";
import { AgentMessageCard } from "../src/components/AgentMessageCard.tsx";
import { render } from "./render.mjs";

for (const [text, preview] of [["-42 degrees C", "-42 degrees C"], ["- item", "item"]]) {
  test(`answer preview preserves the intended text: ${text}`, async () => {
    const { container } = await render(createElement(AgentMessageCard, {
      part: { id: "p", type: "text", text, orx: { kind: "answer" } },
    }));
    const button = container.querySelector("button[aria-expanded]");
    assert.equal(button.getAttribute("aria-label"), preview);
    assert.equal(button.textContent, preview);
    await act(async () => button.click());
    assert.ok(container.textContent.includes(text === "- item" ? "item" : text));
  });
}
