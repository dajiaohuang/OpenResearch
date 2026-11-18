import assert from "node:assert/strict";
import test from "node:test";
import { act, createElement } from "react";
import { FileChip, FileScopeContext } from "../src/components/FileReference.tsx";
import { RuntimeContext } from "../src/RemoteRuntime.tsx";
import { m } from "../src/paraglide/messages.js";
import { find, render, serve, settle } from "./render.mjs";

async function openChip({ remote = false, exp } = {}) {
  const requests = [];
  serve((path, init) => {
    requests.push({ path, init });
    if (path === "/api/projects") return { projects: [{ id: "p", repoPath: "/repo", artifactsDir: "/files/p", slug: "p" }] };
    if (path.startsWith("/api/file-manager?")) return { os: "windows", name: "Explorer" };
    if (path.startsWith("/api/projects/p/file?")) return { path: "report.md", root: "worktree", notFound: false, content: "body", binary: false, truncated: false, presentation: "text" };
    if (path === "/api/projects/p/file/location") return { path: remote ? "/remote/work/report.md" : "C:/work/report.md" };
    return undefined;
  });
  const view = await render(createElement(RuntimeContext, { value: remote ? { kind: "ssh", session: { host: "remote" } } : { kind: "local" } },
    createElement(FileScopeContext, { value: { projectId: "p", sessionId: "s" } }, createElement(FileChip, { path: "report.md", exp, onOpenFile() {} }))));
  await act(async () => view.container.querySelector("button").dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: 10, clientY: 10 })));
  await settle();
  return { view, requests };
}

test("local file action resolves the preview checkout before copying", async (t) => {
  const clipboard = t.mock.method(navigator.clipboard, "writeText", async () => {});
  const { view, requests } = await openChip();
  const copy = find('[role="menuitem"]', m.artifacts_copy_path());
  assert.ok(copy);
  assert.equal(document.querySelectorAll('[role="menuitem"]').length, 3);
  await act(async () => copy.click());
  await settle();
  const action = requests.find((request) => request.path === "/api/projects/p/file/location");
  assert.deepEqual(JSON.parse(action.init.body), { path: "report.md", source: "checkout", sessionId: "s", root: "worktree" });
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
