import assert from "node:assert/strict";
import test from "node:test";
import { parseFilePath } from "../src/fileTabPath.ts";

test("file tabs keep their checkout or artifact source", () => {
  assert.deepEqual(parseFilePath("reports/a.md", "/repo", "session", "/files/p", "p"), { path: "reports/a.md", sessionId: "session" });
  assert.deepEqual(parseFilePath("artifacts/a.md", "/repo", "session", "/files/p", "p"), { path: "a.md", source: "artifacts" });
  assert.deepEqual(parseFilePath("/files/p/a.md", "/repo", "session", "/files/p", "p"), { path: "a.md", source: "artifacts" });
  assert.deepEqual(parseFilePath("/elsewhere/a.md", "/repo", "session", "/files/p", "p"), { path: "/elsewhere/a.md", source: "abs" });
  assert.deepEqual(parseFilePath("C:/work/a.md", "/repo", "session", "/files/p", "p"), { path: "C:/work/a.md", source: "abs" });
});
