import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

test("审计G26：手动开发lint遇warning非零退出，正常输入仍成功", { timeout: 10000 }, () => {
  const web = fileURLToPath(new URL("../apps/web/", import.meta.url));
  const script = JSON.parse(readFileSync(`${web}/package.json`, "utf8")).scripts.lint.split(/\s+/);
  assert.equal(script.shift(), "oxlint");
  const root = fileURLToPath(new URL("../.tmp/", import.meta.url));
  mkdirSync(root, { recursive: true });
  const directory = mkdtempSync(`${root}/warning-policy-`);
  const filename = `${directory}/fixture.js`;
  try {
    writeFileSync(filename, "const unused = 1;\n");
    const args = [...script, "--threads=2", "-A", "all", "-W", "no-unused-vars", filename];
    const baseline = spawnSync(`${web}/node_modules/.bin/oxlint`, args.filter(argument => argument !== "--deny-warnings"), { cwd: web, encoding: "utf8" });
    assert.equal(baseline.error, undefined);
    assert.equal(baseline.status, 0, "原无deny-warnings入口会把warning当作成功");
    assert.match(baseline.stdout + baseline.stderr, /no-unused-vars/);
    const warning = spawnSync(`${web}/node_modules/.bin/oxlint`, args, { cwd: web, encoding: "utf8" });
    assert.equal(warning.error, undefined);
    assert.equal(warning.status, 1, `${warning.error ?? ""}\n${warning.stdout}\n${warning.stderr}`);
    writeFileSync(filename, "export const used = 1;\n");
    const valid = spawnSync(`${web}/node_modules/.bin/oxlint`, args, { cwd: web, encoding: "utf8" });
    assert.equal(valid.error, undefined);
    assert.equal(valid.status, 0, `${valid.error ?? ""}\n${valid.stdout}\n${valid.stderr}`);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
