import assert from "node:assert/strict";
import { chmod, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import path from "node:path";
import { it } from "node:test";
import { buildRustTestExecutionPolicy } from "./run-full-regression.mjs";
import { executeSupervisedRustTestBinaries } from "./rust-test-case-supervisor.mjs";
import { killRecordedProcesses } from "./rust-test-case-supervisor-test-process.mjs";

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function fixtureDirectory(topic) {
  const root = path.resolve(".tmp/deadline-hardgate-tests");
  await mkdir(root, { recursive: true });
  return mkdtemp(path.join(root, `${topic}-`));
}

async function readIdentity(identityPath) {
  try { return JSON.parse(await readFile(identityPath, "utf8")); }
  catch (error) { if (error.code === "ENOENT") return undefined; throw error; }
}

it("launcher 停顿且 case 超时后仍清理 detached 孙进程", {
  timeout: 10000,
  skip: process.platform !== "linux" ? "该用例依赖 POSIX shebang 与 /proc token 清理" : false,
}, async () => {
  const dir = await fixtureDirectory("export-stopped-launcher");
  const launcherPath = path.join(dir, "launcher.mjs");
  const casePath = path.join(dir, "case.mjs");
  const launcherIdentity = path.join(dir, "launcher.json");
  const childIdentity = path.join(dir, "child.json");
  const grandchildIdentity = path.join(dir, "grandchild.json");
  const receipts = path.join(dir, "receipts");
  const marker = path.join(dir, "grandchild-survived");
  const grandchild = `const fs=require('node:fs');const out=process.env.TS_RS_EXPORT_DIR;setInterval(()=>{fs.mkdirSync(out,{recursive:true});fs.writeFileSync(${JSON.stringify(marker)},'escaped');},100);`;
  await writeFile(casePath, `#!${process.execPath}\nimport fs from 'node:fs';\nimport { spawn } from 'node:child_process';\nconst out = process.env.TS_RS_EXPORT_DIR;\nfs.mkdirSync(out, { recursive: true });\nfs.writeFileSync(out + '/type.ts', 'export {};\\n');\nfs.appendFileSync(${JSON.stringify(receipts)}, out + '\\n');\nconst selfStat = fs.readFileSync('/proc/self/stat', 'utf8');\nconst selfBirth = selfStat.slice(selfStat.lastIndexOf(')') + 2).split(' ')[19];\nfs.writeFileSync(${JSON.stringify(childIdentity)}, JSON.stringify({ pid: process.pid, birth: selfBirth }));\nconst child = spawn(process.execPath, ['-e', ${JSON.stringify(grandchild)}], { detached: true, stdio: 'ignore' });\nconst childStat = fs.readFileSync('/proc/' + child.pid + '/stat', 'utf8');\nconst childBirth = childStat.slice(childStat.lastIndexOf(')') + 2).split(' ')[19];\nfs.writeFileSync(${JSON.stringify(grandchildIdentity)}, JSON.stringify({ pid: child.pid, birth: childBirth }));\nprocess.exit(0);\n`);
  await chmod(casePath, 0o755);
  await writeFile(launcherPath, `import fs from 'node:fs';\nimport path from 'node:path';\nimport { spawn } from 'node:child_process';\nconst now = () => Number(process.hrtime.bigint()) / 1e6;\nlet exportRoot;\nconst selfStat = fs.readFileSync('/proc/self/stat', 'utf8');\nconst selfBirth = selfStat.slice(selfStat.lastIndexOf(')') + 2).split(' ')[19];\nfs.writeFileSync(${JSON.stringify(launcherIdentity)}, JSON.stringify({ pid: process.pid, birth: selfBirth }));\nprocess.on('message', async (message) => {\n  if (message.kind === 'initialize') {\n    exportRoot = message.exportRoot;\n    const started = now();\n    process.send({ kind: 'arm', token: String(process.pid) + ':stopped', started, total: started + 5000, execution: started + 1200, timeoutMs: 5000, context: 'stopped launcher case' });\n  } else if (message.kind === 'armed') {\n    spawn(process.execPath, [${JSON.stringify(casePath)}], { stdio: 'ignore', env: { ...process.env, STOCK_GAME_RUST_CASE_TOKEN: message.token, TS_RS_EXPORT_DIR: path.join(exportRoot, '0') } });\n    const until = Date.now() + 4000;\n    while ((!fs.existsSync(${JSON.stringify(childIdentity)}) || !fs.existsSync(${JSON.stringify(grandchildIdentity)})) && Date.now() < until) await new Promise((resolve) => setTimeout(resolve, 10));\n    process.kill(process.pid, 'SIGSTOP');\n  }\n});\nprocess.send({ kind: 'ready' });\n`);
  try {
    await assert.rejects(executeSupervisedRustTestBinaries({
      artifacts: [], policy: buildRustTestExecutionPolicy(4, 1), cwd: process.cwd(), env: { ...process.env, TMPDIR: dir },
      remainingMs: () => 8000, launcherPath,
    }), /stopped launcher case.*deadline/);
    const outputs = (await readFile(receipts, "utf8")).trim().split("\n");
    assert.equal(outputs.length, 1);
    assert.ok(outputs[0].startsWith(path.join(dir, "rust-export-")));
    await rm(marker, { force: true });
    await sleep(500);
    assert.equal(existsSync(outputs[0]), false, "超时后必须删除导出根目录");
    assert.equal(existsSync(marker), false, "launcher 停顿遗留的孙进程不得重建目录");
  } finally {
    const records = (await Promise.all([launcherIdentity, childIdentity, grandchildIdentity].map(readIdentity))).filter((record) => record !== undefined);
    if (records.length > 0) killRecordedProcesses(records);
    await rm(dir, { recursive: true, force: true });
  }
});
