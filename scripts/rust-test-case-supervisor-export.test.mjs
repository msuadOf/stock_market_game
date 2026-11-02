import assert from "node:assert/strict";
import { chmod, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import path from "node:path";
import { it } from "node:test";
import { buildRustTestExecutionPolicy } from "./run-full-regression.mjs";
import { executeSupervisedRustTestBinaries, removeExportRootWithinDeadline } from "./rust-test-case-supervisor.mjs";

async function fixtureDirectory(topic) {
  const root = path.resolve(".tmp/deadline-hardgate-tests");
  await mkdir(root, { recursive: true });
  return mkdtemp(path.join(root, `${topic}-`));
}

// POSIX shebang fixture 在 Windows 上不能直接作为可执行文件启动；目录注入由普通 case 单元测试覆盖。
it("真实 watchdog 为导出用例隔离输出并清理临时目录", {
  timeout: 10000,
  skip: process.platform === "win32" ? "Windows 不执行 POSIX shebang fixture" : false,
}, async () => {
  const dir = await fixtureDirectory("export-isolation");
  const binary = path.join(dir, "fixture.mjs");
  const receipts = path.join(dir, "receipts");
  await writeFile(binary, `#!${process.execPath}\nimport fs from 'node:fs';\nconst args = process.argv.slice(2);\nif (args.includes('--list')) { if (!args.includes('--ignored')) console.log('a::export_bindings_one: test\\nb::export_bindings_two: test'); } else { const out = process.env.TS_RS_EXPORT_DIR; fs.mkdirSync(out, { recursive: true }); fs.writeFileSync(out + '/type.ts', args[0]); fs.appendFileSync(${JSON.stringify(receipts)}, out + '\\n'); }\n`);
  await chmod(binary, 0o755);
  try {
    const result = await executeSupervisedRustTestBinaries({
      artifacts: [{ executable: binary, label: "fixture" }], policy: buildRustTestExecutionPolicy(8, 1),
      cwd: process.cwd(), env: { ...process.env, TMPDIR: dir }, remainingMs: () => 8000,
    });
    assert.equal(result.case_count, 2);
    const outputs = (await readFile(receipts, "utf8")).trim().split("\n");
    assert.equal(new Set(outputs).size, 2);
    assert.ok(outputs.every((output) => output.startsWith(path.join(dir, "rust-export-"))));
    assert.ok(outputs.every((output) => !existsSync(output)));
    assert.ok((await readdir(dir)).every((name) => !name.startsWith("rust-export-")));
  } finally { await rm(dir, { recursive: true, force: true }); }
});

it("真实清理可在 1000ms 内删除批量导出目录", { timeout: 10000 }, async () => {
  const dir = await fixtureDirectory("export-bulk-cleanup");
  const root = path.join(dir, "rust-export-bulk");
  await Promise.all(Array.from({ length: 100 }, async (_, index) => {
    const caseDir = path.join(root, `case-${index}`);
    await mkdir(caseDir, { recursive: true });
    await writeFile(path.join(caseDir, "type.ts"), "export {};\n");
  }));
  try {
    const started = Number(process.hrtime.bigint()) / 1e6;
    await removeExportRootWithinDeadline({ root, deadline: started + 1000 });
    const elapsed = Number(process.hrtime.bigint()) / 1e6 - started;
    assert.equal(existsSync(root), false);
    assert.ok(elapsed < 1000, `批量导出目录清理耗时 ${elapsed.toFixed(1)}ms`);
  } finally { await rm(dir, { recursive: true, force: true }); }
});
