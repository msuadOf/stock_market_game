import assert from "node:assert/strict";
import { chmod, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import path from "node:path";
import { it } from "node:test";
import { buildRustTestExecutionPolicy } from "./run-full-regression.mjs";
import { executeSupervisedRustTestBinaries } from "./rust-test-case-supervisor.mjs";

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function fixtureDirectory(topic) {
  const root = path.resolve(".tmp/deadline-hardgate-tests");
  await mkdir(root, { recursive: true });
  return mkdtemp(path.join(root, `${topic}-`));
}

async function killRecordedPids(paths) {
  for (const pidPath of paths) {
    try {
      const pid = Number((await readFile(pidPath, "utf8")).trim());
      if (Number.isSafeInteger(pid) && pid > 0) process.kill(pid, "SIGKILL");
    } catch (error) { if (error.code !== "ENOENT" && error.code !== "ESRCH") throw error; }
  }
}

it("两个导出 case 同时失败时清理全部已登记孙进程", {
  timeout: 10000,
  skip: process.platform !== "linux" ? "该用例依赖 POSIX shebang 与 /proc token 清理" : false,
}, async () => {
  const dir = await fixtureDirectory("export-double-failure");
  const binary = path.join(dir, "fixture.mjs");
  const receipts = path.join(dir, "receipts");
  const ids = ["one", "two"];
  const caseReady = Object.fromEntries(ids.map((id) => [id, path.join(dir, `case-ready-${id}`)]));
  const grandchildReady = Object.fromEntries(ids.map((id) => [id, path.join(dir, `grandchild-ready-${id}`)]));
  const markers = Object.fromEntries(ids.map((id) => [id, path.join(dir, `escaped-${id}`)]));
  const grandchild = (id) => `const fs=require('node:fs');const out=process.env.TS_RS_EXPORT_DIR;fs.writeFileSync(${JSON.stringify(grandchildReady[id])},String(process.pid));setInterval(()=>{fs.mkdirSync(out,{recursive:true});fs.writeFileSync(${JSON.stringify(markers[id])},'escaped');},100);`;
  const helper = (id) => `const {spawn}=require('node:child_process');spawn(process.execPath,['-e',${JSON.stringify(grandchild(id))}],{detached:true,stdio:'ignore'}).unref();`;
  const helpers = Object.fromEntries(ids.map((id) => [id, helper(id)]));
  const source = `#!${process.execPath}\nimport { spawn } from 'node:child_process';\nimport fs from 'node:fs';\nconst args = process.argv.slice(2);\nif (args.includes('--list')) { if (!args.includes('--ignored')) console.log('a::export_bindings_failure_one: test\\nb::export_bindings_failure_two: test'); } else { const id = args[0].includes('one') ? 'one' : 'two'; const other = id === 'one' ? 'two' : 'one'; const ready = ${JSON.stringify(caseReady)}; const gcReady = ${JSON.stringify(grandchildReady)}; const helpers = ${JSON.stringify(helpers)}; const out = process.env.TS_RS_EXPORT_DIR; fs.mkdirSync(out, { recursive: true }); fs.writeFileSync(out + '/type.ts', 'export {};\\n'); fs.appendFileSync(${JSON.stringify(receipts)}, out + '\\n'); fs.writeFileSync(ready[id], 'ready'); const until = Date.now() + 5000; while (!fs.existsSync(ready[other]) && Date.now() < until) await new Promise((resolve) => setTimeout(resolve, 5)); spawn(process.execPath, ['-e', helpers[id]], { detached: true, stdio: 'ignore' }).unref(); const untilGc = Date.now() + 5000; while (!fs.existsSync(gcReady[id]) && Date.now() < untilGc) await new Promise((resolve) => setTimeout(resolve, 5)); await new Promise((resolve) => setTimeout(resolve, 100)); process.exit(1); }\n`;
  await writeFile(binary, source);
  await chmod(binary, 0o755);
  try {
    await assert.rejects(executeSupervisedRustTestBinaries({
      artifacts: [{ executable: binary, label: "fixture" }], policy: buildRustTestExecutionPolicy(8, 1),
      cwd: process.cwd(), env: { ...process.env, TMPDIR: dir }, remainingMs: () => 6000,
    }), /export_bindings_failure_(one|two).*failed|failed.*export_bindings_failure_(one|two)/s);
    const outputs = (await readFile(receipts, "utf8")).trim().split("\n");
    assert.equal(outputs.length, 2);
    assert.equal(new Set(outputs).size, 2);
    assert.ok(outputs.every((output) => output.startsWith(path.join(dir, "rust-export-"))));
    for (const marker of Object.values(markers)) await rm(marker, { force: true });
    await sleep(500);
    assert.ok(outputs.every((output) => !existsSync(output)), "两个导出根目录都必须删除");
    assert.ok(Object.values(markers).every((marker) => !existsSync(marker)), "两个孙进程都不得重建目录");
    assert.ok((await readdir(dir)).every((name) => !name.startsWith("rust-export-")));
  } finally {
    await killRecordedPids(Object.values(grandchildReady));
    await rm(dir, { recursive: true, force: true });
  }
});
