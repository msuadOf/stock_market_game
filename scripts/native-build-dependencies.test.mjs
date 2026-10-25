import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { it } from "node:test";

it("native bindings keep the formatter used by the committed TypeScript contract", () => {
  const tree = execFileSync("cargo", [
    "tree", "--locked", "--offline", "-p", "engine", "-e", "features",
  ], { cwd: new URL("..", import.meta.url), encoding: "utf8", timeout: 5000 });
  assert.match(tree, /ts-rs feature "format"/);
  assert.match(tree, /dprint-plugin-typescript/);
});
