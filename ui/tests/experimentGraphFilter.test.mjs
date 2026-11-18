import assert from "node:assert/strict";
import test from "node:test";
import { graphMatches, saveGraphMove } from "../src/experimentGraphFilter.ts";
import { getViewportForBounds } from "@xyflow/react";

test("search retains ancestor context without leaking unrelated branches", () => {
  const nodes = [
    { id: "a", title: "Baseline", slug: "base", status: "done" },
    { id: "b", title: "Long run", slug: "trial", status: "running" },
    { id: "c", title: "Other", slug: "other", status: null },
  ];
  const edges = [{ source: "a", target: "b" }, { source: "a", target: "c" }];
  const { matching, visible } = graphMatches(nodes, edges, "TRIAL", "running");
  assert.deepEqual([...matching], ["b"]);
  assert.deepEqual([...visible].sort(), ["a", "b"]);
  assert.equal(graphMatches(nodes, edges, "missing", "all").visible.size, 0);
});

test("an invalid cycle terminates when finding context", () => {
  const result = graphMatches([{ id: "a", title: "A", slug: "a", status: null }], [{ source: "b", target: "a" }, { source: "a", target: "b" }], "a", "all");
  assert.deepEqual([...result.visible].sort(), ["a", "b"]);
});

test("a deep graph retains every ancestor of a matching leaf", () => {
  const nodes = Array.from({ length: 5000 }, (_, i) => ({ id: String(i), title: i === 4999 ? "needle" : "", slug: `node-${i}`, status: null }));
  const edges = nodes.slice(1).map((node, i) => ({ source: String(i), target: node.id }));
  const result = graphMatches(nodes, edges, "needle", "all");
  assert.equal(result.matching.size, 1);
  assert.equal(result.visible.size, 5000);
  assert.ok(result.visible.has("0"));
});

test("the lower zoom bound permits fitting a 170-node chain", () => {
  const height = 170 * 132 + 169 * 72;
  const viewport = getViewportForBounds({ x: 0, y: 0, width: 336, height }, 800, 1080, 0.000001, 1, 0.25);
  assert.ok(viewport.zoom < 0.15);
  assert.ok(height * viewport.zoom <= 1080);
});

test("completed moves save only while their original canvas is current", async () => {
  let resolve;
  let current = true;
  const saved = [];
  const flow = { viewport: { x: 1, y: 2, zoom: 0.2 } };
  const pending = saveGraphMove(flow, () => new Promise((yes) => { resolve = yes; }), () => current, (f) => f.viewport, (v) => saved.push(v));
  current = false;
  resolve(true);
  await pending;
  assert.deepEqual(saved, []);
  current = true;
  await saveGraphMove(flow, async () => false, () => current, (f) => f.viewport, (v) => saved.push(v));
  assert.deepEqual(saved, []);
  await saveGraphMove(flow, async () => true, () => current, (f) => f.viewport, (v) => saved.push(v));
  assert.deepEqual(saved, [flow.viewport]);
});
