import assert from "node:assert/strict";
import test from "node:test";
import { createElement } from "react";
import { CsvTable } from "../src/components/CsvTable.tsx";
import { render } from "./render.mjs";

test("CSV preview includes an explicitly quoted empty record", async () => {
  const view = await render(createElement(CsvTable, {
    path: "results.csv", text: 'value\n""\n\nalpha\n', truncated: false,
  }));
  assert.equal(view.container.querySelector('[role="table"]').getAttribute("aria-rowcount"), "3");
});
