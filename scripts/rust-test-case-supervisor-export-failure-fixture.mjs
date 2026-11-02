import assert from "node:assert/strict";
import { chmod, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import path from "node:path";
import { buildRustTestExecutionPolicy } from "./run-full-regression.mjs";
import { executeSupervisedRustTestBinaries } from "./rust-test-case-supervisor.mjs";

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function fixtureDirectory(topic) {
  const root = path.resolve(".tmp/deadline-hardgate-tests");
  await mkdir(root, { recursive: true });
  return mkdtemp(path.join(root, `${topic}-`));
}

export async function runFailingExportCase(mode, { remainingMs = 6000 } = {}) {
  const dir = await fixtureDirectory(`export-${mode}`);
  const binary = path.join(dir, "fixture.mjs");
  const receipts = path.join(dir, "receipts");
  const marker = path.join(dir, "grandchild-survived");
  const ready = path.join(dir, "grandchild-ready");
  const grandchild = `const fs=require('node:fs');const out=process.env.TS_RS_EXPORT_DIR;const marker=${JSON.stringify(marker)};const ready=${JSON.stringify(ready)};fs.writeFileSync(ready,String(process.pid));setInterval(()=>{fs.mkdirSync(out,{recursive:true});fs.writeFileSync(marker,'escaped');},100);`;
  const finish = mode === "failure" ? "process.exit(1);" : "setInterval(() => {}, 1000);";
  await writeFile(binary, `#!${process.execPath}\nimport { spawn } from 'node:child_process';\nimport fs from 'node:fs';\nconst args = process.argv.slice(2);\nif (args.includes('--list')) { if (!args.includes('--ignored')) console.log('a::export_bindings_failure: test'); } else { const out = process.env.TS_RS_EXPORT_DIR; fs.mkdirSync(out, { recursive: true }); fs.writeFileSync(out + '/type.ts', 'export {};\\n'); fs.appendFileSync(${JSON.stringify(receipts)}, out + '\\n'); spawn(process.execPath, ['-e', ${JSON.stringify(grandchild)}], { detached: true, stdio: 'ignore' }); const until = Date.now() + 1000; while (!fs.existsSync(${JSON.stringify(ready)}) && Date.now() < until) await new Promise((resolve) => setTimeout(resolve, 10)); ${finish} }\n`);
  await chmod(binary, 0o755);
  try {
    await assert.rejects(executeSupervisedRustTestBinaries({
      artifacts: [{ executable: binary, label: "fixture" }], policy: buildRustTestExecutionPolicy(8, 1),
      cwd: process.cwd(), env: { ...process.env, TMPDIR: dir }, remainingMs: () => remainingMs,
    }), mode === "failure" ? /export_bindings_failure.*failed|failed.*export_bindings_failure/s : /deadline/);
    const outputs = (await readFile(receipts, "utf8")).trim().split("\n");
    assert.equal(outputs.length, 1);
    assert.ok(outputs[0].startsWith(path.join(dir, "rust-export-")));
    assert.equal(existsSync(outputs[0]), false, "失败或超时后必须删除导出根目录");
    await rm(marker, { force: true });
    await sleep(500);
    assert.equal(existsSync(marker), false, "detached 孙进程不得在清理后重建导出目录");
    assert.equal(existsSync(outputs[0]), false);
    assert.ok((await readdir(dir)).every((name) => !name.startsWith("rust-export-")));
  } finally {
    // 断言失败时也必须终止测试孙进程，不能让 detached 进程泄漏。
    try {
      const pid = Number((await readFile(ready, "utf8")).trim());
      if (Number.isSafeInteger(pid) && pid > 0) process.kill(pid, "SIGKILL");
    } catch (error) { if (error.code !== "ENOENT" && error.code !== "ESRCH") throw error; }
    await rm(dir, { recursive: true, force: true });
  }
}
