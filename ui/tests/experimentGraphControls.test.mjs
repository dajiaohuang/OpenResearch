import assert from "node:assert/strict";
import test from "node:test";
import { act, createElement } from "react";
import { TreeView } from "../src/components/TreeView.tsx";
import { m } from "../src/paraglide/messages.js";
import { render, serve, settle, type } from "./render.mjs";

test("search and status controls show matches and clear their state", async () => {
  serve(() => ({}));
  const experiment = (id, parent = null) => ({ id, projectId: "p", slug: id, title: id, parentExperimentId: parent, branchName: id, createdAt: 1, archived: false });
  const experiments = [experiment("root"), experiment("match", "root"), experiment("other", "root"), { ...experiment("archived", "root"), archived: true }];
  const noop = () => {};
  const moves = [];
  const props = {
    experiments, archiveActions: new Map(experiments.map((e) => [e.id, {}])), showArchived: false,
    runs: [], project: { id: "p" }, onOpenView: noop, onOpenCode: noop, onArchive: noop, onDelete: noop,
    agentSessionId: null, onShowProjectScope: noop, onRestoreRegion: noop, viewport: null, onViewportChange: (value) => moves.push(value),
  };
  const { container, rerender } = await render(createElement(TreeView, props));
  await settle();
  const fit = [...container.querySelectorAll("button")].find((button) => button.textContent === m.tree_fit_graph());
  assert.equal(fit.disabled, true, "fit waits for the nodes to be measured");
  const input = container.querySelector(`input[aria-label="${m.tree_search_graph()}"]`);
  assert.ok(input);
  await type(input, "match");
  await settle();
  const results = container.querySelector(`[role="region"][aria-label="${m.tree_search_graph()}"]`);
  assert.ok(results.textContent.includes("match (match)"));
  await type(input, "archived");
  assert.ok(results.textContent.includes("archived (archived)"), "search finds an archived experiment without changing the saved archive preference");
  await type(input, "match");
  const status = container.querySelector(`select[aria-label="${m.settings_page_status()}"]`);
  const canvas = container.querySelector(`[aria-label="${m.tree_canvas_controls()}"]`);
  const inputArrow = new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true });
  await act(async () => input.dispatchEvent(inputArrow));
  assert.equal(inputArrow.defaultPrevented, false);
  const canvasArrow = new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true });
  const beforePan = moves.length;
  await act(async () => canvas.dispatchEvent(canvasArrow));
  assert.equal(canvasArrow.defaultPrevented, true, "an initialized canvas handles its own navigation keys");
  await settle();
  assert.ok(moves.length > beforePan, "a keyboard pan saves its viewport");
  await act(async () => { status.value = "running"; status.dispatchEvent(new Event("change", { bubbles: true })); });
  assert.equal(results.textContent.trim(), m.artifacts_filter_no_match());
  const clear = [...container.querySelectorAll("button")].find((button) => button.textContent === m.artifacts_filter_clear());
  assert.ok(clear);
  await act(async () => clear.click());
  assert.equal(input.value, "");
  assert.equal(status.value, "all");
  await type(input, "other");
  await rerender(createElement(TreeView, { ...props, project: { id: "q" } }));
  assert.equal(input.value, "", "a new project starts without the previous project's search");
});

test("a large chain renders its controls without changing a saved viewport", async () => {
  serve(() => ({}));
  const experiments = Array.from({ length: 170 }, (_, i) => ({ id: String(i), projectId: "p", slug: `exp-${i}`, parentExperimentId: i ? String(i - 1) : null, branchName: `exp-${i}`, createdAt: i, archived: false }));
  const changes = [];
  const noop = () => {};
  const props = {
    experiments, archiveActions: new Map(experiments.map((e) => [e.id, {}])), showArchived: true,
    runs: [], project: { id: "p" }, onOpenView: noop, onOpenCode: noop, onArchive: noop, onDelete: noop,
    agentSessionId: null, onShowProjectScope: noop, onRestoreRegion: noop,
    viewport: { x: 7, y: 9, zoom: 0.5 }, onViewportChange: (v) => changes.push(v),
  };
  const { container, rerender } = await render(createElement(TreeView, props));
  await settle();
  assert.ok([...container.querySelectorAll("button")].some((button) => button.textContent === m.tree_fit_graph()));
  await rerender(createElement(TreeView, props));
  assert.deepEqual(changes, []);
});
