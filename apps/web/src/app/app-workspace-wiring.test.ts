import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

test("App installs the movable workspace and removes narrow-landscape empty positions", () => {
  const source = readFileSync(new URL("../App.tsx", import.meta.url), "utf8");
  const css = readFileSync(new URL("../App.css", import.meta.url), "utf8");
  assert.ok(source.includes("<WorkspaceGrid orientation={orientation}"));
  assert.ok(source.includes("</WorkspaceGrid>"));
  assert.equal(css.includes(".layout-desktop .pos-panel { display: none; }"), false);
  assert.equal(css.includes(".layout-desktop .app-grid {"), false);
});
