import assert from "node:assert/strict";
import test from "node:test";
import { act, createElement } from "react";
import { FileChip, FileScopeContext } from "../src/components/FileReference.tsx";
import { Md } from "../src/components/Md.tsx";
import { QueryObserver } from "@tanstack/react-query";
import { RuntimeContext } from "../src/RemoteRuntime.tsx";
import { m } from "../src/paraglide/messages.js";
import { find, render, serve, settle } from "./render.mjs";

async function openChip({ remote = false, exp, changed = false, figure = false, preview = false } = {}) {
  const requests = [];
  serve((path, init) => {
    requests.push({ path, init });
    if (path === "/api/projects") return { projects: [{ id: "p", repoPath: "/repo", artifactsDir: "/files/p", slug: "p" }] };
    if (path.startsWith("/api/file-manager?")) return { os: "windows", name: "Explorer" };
    if (path.startsWith("/api/projects/p/file?")) return { path: figure ? "table1.png" : "report.md", root: "worktree", notFound: false, content: "body", binary: false, truncated: false, presentation: "text" };
    if (path === "/api/projects/p/file/location") {
      if (changed && JSON.parse(init.body).expectedPath) return Response.json({ error: "file location changed" }, { status: 400 });
      return { path: remote ? "/remote/work/report.md" : "C:/work/report.md" };
    }
    return undefined;
  });
  const view = await render(createElement(RuntimeContext, { value: remote ? { kind: "ssh", session: { host: "remote" } } : { kind: "local" } },
    createElement(FileScopeContext, { value: { projectId: "p", sessionId: "s" } }, figure
      ? createElement(Md, { text: '[Table 1](https://example.com/paper)\n\n![Results](table1.png "Table 1. Results")', onOpenFile() {} })
      : createElement(FileChip, { path: "report.md", exp, onOpenFile() {} }))));
  const observer = preview ? new QueryObserver(view.workspace.queryClient, view.workspace.queries.resolvedFileQuery("p", "report.md", "repo", "s")) : null;
  const unsubscribe = observer?.subscribe(() => {});
  if (observer) await settle();
  await act(async () => view.container.querySelector("button").dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: 10, clientY: 10 })));
  await settle();
  return { view, requests, unsubscribe };
}

test("local file action resolves the preview checkout before copying", async (t) => {
  const clipboard = t.mock.method(navigator.clipboard, "writeText", async () => {});
  const { view, requests } = await openChip();
  const copy = find('[role="menuitem"]', m.artifacts_copy_path());
  assert.ok(copy);
  assert.equal(document.querySelectorAll('[role="menuitem"]').length, 3);
  await act(async () => copy.click());
  await settle();
  const actions = requests.filter((request) => request.path === "/api/projects/p/file/location");
  assert.equal(actions.length, 2);
  assert.deepEqual(JSON.parse(actions[1].init.body), { path: "report.md", source: "checkout", sessionId: "s", root: "worktree", expectedPath: "C:/work/report.md" });
  assert.equal(clipboard.mock.calls[0].arguments[0], "C:/work/report.md");
  await view.unmount();
});

test("remote file permits copy but never local reveal", async () => {
  const { view } = await openChip({ remote: true });
  assert.ok(find('[role="menuitem"]', m.artifacts_copy_path()));
  assert.equal(document.querySelectorAll('[role="menuitem"]').length, 2);
  await view.unmount();
});

test("historical references do not expose disk actions", async () => {
  const { view, requests } = await openChip({ exp: "experiment" });
  assert.ok(document.body.textContent.includes(m.file_tree_location_historical()));
  assert.ok(!find('[role="menuitem"]', m.artifacts_copy_path()));
  assert.ok(!requests.some((request) => request.path.startsWith("/api/projects/p/file?")));
  await view.unmount();
});

test("a changed location is not copied", async (t) => {
  const clipboard = t.mock.method(navigator.clipboard, "writeText", async () => {});
  const { view } = await openChip({ changed: true });
  await act(async () => find('[role="menuitem"]', m.artifacts_copy_path()).click());
  await settle();
  assert.equal(clipboard.mock.calls.length, 0);
  await view.unmount();
});

test("a numbered figure link exposes the file context menu", async () => {
  const { view, requests } = await openChip({ figure: true });
  assert.ok(find('[role="menuitem"]', m.artifacts_copy_path()));
  const request = requests.find((item) => item.path === "/api/projects/p/file/location");
  assert.equal(JSON.parse(request.init.body).path, "table1.png");
  await view.unmount();
});

test("an open preview without source identity disables location actions", async () => {
  const { view, requests, unsubscribe } = await openChip({ preview: true });
  assert.ok(document.body.textContent.includes(m.file_tree_location_preview_unverified()));
  assert.ok(!find('[role="menuitem"]', m.artifacts_copy_path()));
  assert.ok(!requests.some((request) => request.path === "/api/projects/p/file/location"));
  unsubscribe();
  await view.unmount();
});

test("a preview opened after the menu prevents copying", async (t) => {
  const clipboard = t.mock.method(navigator.clipboard, "writeText", async () => {});
  const { view, requests } = await openChip();
  const observer = new QueryObserver(view.workspace.queryClient, view.workspace.queries.resolvedFileQuery("p", "report.md", "repo", "s"));
  const unsubscribe = observer.subscribe(() => {});
  await settle();
  await act(async () => find('[role="menuitem"]', m.artifacts_copy_path()).click());
  await settle();
  assert.equal(clipboard.mock.calls.length, 0);
  assert.equal(requests.filter((request) => request.path === "/api/projects/p/file/location").length, 1);
  unsubscribe();
  await view.unmount();
});
