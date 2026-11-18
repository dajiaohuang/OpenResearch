import assert from "node:assert/strict";
import test from "node:test";
import { act, createElement } from "react";
import { ArtifactsTab } from "../src/components/ArtifactsTab.tsx";
import { RuntimeContext } from "../src/RemoteRuntime.tsx";
import { m } from "../src/paraglide/messages.js";
import { render, serve, settle, type } from "./render.mjs";

test("search shows an artifact beyond the loaded tree and its path can be copied", async (t) => {
  const base = { name: "base.txt", path: "base.txt", isDir: false, size: 1, modifiedAt: 0, presentation: "text" };
  const match = { ...base, name: "late.txt", path: "reports/late.txt" };
  let query;
  const clipboard = t.mock.method(navigator.clipboard, "writeText", async () => {});
  serve((path, init) => {
    if (path === "/_orx/runtime") return { kind: "local", version: "test", buildChannel: "stable" };
    if (path.startsWith("/api/projects/p/files/search?")) {
      query = new URL(path, "http://local").searchParams.get("q");
      return { dir: "/artifacts", truncated: true, entries: [{ ...base, name: "reports", path: "reports", isDir: true, children: [match] }] };
    }
    if (path.startsWith("/api/projects/p/files/file?")) return new Response("preview body");
    if (path === "/api/projects/p/file/location") {
      assert.deepEqual(JSON.parse(init.body), { path: "reports/late.txt", source: "artifact" });
      return { path: "/artifacts/reports/late.txt" };
    }
    return undefined;
  });
  const view = await render(createElement(RuntimeContext, { value: { kind: "local" } }, createElement(ArtifactsTab, {
    project: { id: "p" }, artifacts: { dir: "/artifacts", entries: [base], truncated: true },
    onOpenFile() {}, canRenameFile() { return false; },
  })));
  await settle();
  await act(async () => document.querySelector(`[aria-label="${m.artifacts_filter()}"]`).click());
  await settle();
  await type(document.querySelector("input"), "reports/late");
  await act(async () => new Promise((resolve) => setTimeout(resolve, 350)));
  await settle();
  assert.equal(query, "reports/late");
  assert.ok(document.body.textContent.includes("late.txt"));
  assert.ok(document.body.textContent.includes(m.artifacts_tab_listing_truncated_the_folder_has_more_artifacts()));
  const row = document.querySelector('[data-keyboard-path="reports/late.txt"]');
  assert.ok(row);
  await act(async () => row.click());
  await settle();
  assert.equal(row.getAttribute("aria-pressed"), "true");
  await act(async () => view.container.querySelector(`[aria-label="${m.artifacts_copy_path()}"]`).click());
  await settle();
  assert.equal(clipboard.mock.calls[0].arguments[0], "/artifacts/reports/late.txt");
  await view.unmount();
});
test("search reports a server read error instead of showing no matches", async () => {
  const base = { name: "base.txt", path: "base.txt", isDir: false, size: 1, modifiedAt: 0, presentation: "text" };
  serve((path) => {
    if (path === "/_orx/runtime") return { kind: "local", version: "test", buildChannel: "stable" };
    if (path.startsWith("/api/projects/p/files/search?")) return new Response("read failed", { status: 500 });
    return undefined;
  });
  const view = await render(createElement(RuntimeContext, { value: { kind: "local" } }, createElement(ArtifactsTab, {
    project: { id: "p" }, artifacts: { dir: "/artifacts", entries: [base], truncated: true },
    onOpenFile() {}, canRenameFile() { return false; },
  })));
  await settle();
  await act(async () => document.querySelector(`[aria-label="${m.artifacts_filter()}"]`).click());
  await settle();
  await type(document.querySelector("input"), "missing");
  await act(async () => new Promise((resolve) => setTimeout(resolve, 350)));
  await settle();
  assert.match(document.querySelector('[role="alert"]').textContent, /read failed/);
  assert.ok(!document.body.textContent.includes(m.artifacts_filter_no_match()));
  await view.unmount();
});

test("artifact root can be revealed on a local workspace", async () => {
  const base = { name: "base.txt", path: "base.txt", isDir: false, size: 1, modifiedAt: 0, presentation: "text" };
  let body;
  serve((path, init) => {
    if (path === "/api/projects/p/files/reveal") { body = JSON.parse(init.body); return { ok: true }; }
    return undefined;
  });
  const tab = createElement(ArtifactsTab, { project: { id: "p" }, artifacts: { dir: "/artifacts", entries: [base] }, onOpenFile() {}, canRenameFile() { return false; } });
  const view = await render(createElement(RuntimeContext, { value: { kind: "local" } }, tab));
  await act(async () => view.container.querySelector(`[aria-label="${m.file_viewer_reveal_in_file_manager()}"]`).click());
  await settle();
  assert.deepEqual(body, { path: "" });
  await view.unmount();
});

test("an empty artifact directory can be revealed", async () => {
  let body;
  serve((path, init) => {
    if (path === "/api/projects/p/files/reveal") { body = JSON.parse(init.body); return { ok: true }; }
    return undefined;
  });
  const view = await render(createElement(RuntimeContext, { value: { kind: "local" } },
    createElement(ArtifactsTab, { project: { id: "p" }, artifacts: { dir: "/artifacts", entries: [] }, onOpenFile() {}, canRenameFile() { return false; } })));
  await act(async () => view.container.querySelector(`[aria-label="${m.file_viewer_reveal_in_file_manager()}"]`).click());
  await settle();
  assert.deepEqual(body, { path: "" });
  await view.unmount();
});

test("remote artifact mode hides the local root action", async () => {
  const view = await render(createElement(RuntimeContext, { value: { kind: "local" } },
    createElement(ArtifactsTab, { project: { id: "p" }, artifacts: { dir: "/artifacts", entries: [] }, remote: true, onOpenFile() {}, canRenameFile() { return false; } })));
  assert.equal(view.container.querySelector(`[aria-label="${m.file_viewer_reveal_in_file_manager()}"]`), null);
  await view.unmount();
});
