import assert from "node:assert/strict";
import test from "node:test";
import { act, createElement } from "react";
import { WorktreeTab } from "../src/components/WorktreeTab.tsx";
import { m } from "../src/paraglide/messages.js";
import { find, render, serve, settle, type } from "./render.mjs";

test("a path omitted from the tree can be copied to artifacts", async () => {
  let posted;
  serve((path, init) => {
    if (path.startsWith("/api/chat/sessions?")) return { sessions: [] };
    if (path === "/api/chat/sessions/s/worktree") return { exists: true, branch: "main", files: [] };
    if (path.startsWith("/api/projects/p/code-tree")) return { root: "worktree", path: "/repo", entries: [] };
    if (path === "/api/projects/p/files/publish") {
      posted = JSON.parse(init.body);
      return { path: "ignored-output" };
    }
    return undefined;
  });
  const project = { id: "p", baselineBranch: "main", repoPath: "/repo", githubEnabled: false };
  await render(createElement(WorktreeTab, {
    sessionId: "s", project, view: "files", toggled: new Set(),
    onViewChange() {}, onToggledChange() {}, onOpenFile() {}, canRenameFile() { return true; },
  }));
  await settle();
  const field = document.querySelector(`input[placeholder="${m.worktree_open_path_label()}"]`);
  assert.ok(field);
  await type(field, "ignored-output");
  const button = find("button", m.worktree_copy_to_artifacts());
  assert.ok(button);
  await act(async () => button.click());
  await settle();
  const dialog = document.querySelector('[role="dialog"]');
  assert.ok(dialog);
  assert.ok(dialog.textContent.includes(m.worktree_copy_changes_warning()));
  assert.equal(posted, undefined);
  await act(async () => find('[role="dialog"] button', m.common_cancel()).click());
  await settle();
  assert.equal(posted, undefined);
  await act(async () => button.click());
  await settle();
  await act(async () => find('[role="dialog"] button', m.worktree_copy_to_artifacts()).click());
  await settle();
  assert.deepEqual(posted, { sessionId: "s", path: "ignored-output" });
});
