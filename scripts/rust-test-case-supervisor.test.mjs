import assert from "node:assert/strict";
import { chmod, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { it } from "node:test";
import { pathToFileURL } from "node:url";
import { runBoundedCommand } from "./run-with-deadline.mjs";
import { buildRustTestExecutionPolicy, executeRustTestBinaries } from "./run-full-regression.mjs";
import { executeSupervisedRustTestBinaries } from "./rust-test-case-supervisor.mjs";

async function fixtureDirectory(topic) {
  const root = path.resolve(".tmp/deadline-hardgate-tests");
  await mkdir(root, { recursive: true });
  return mkdtemp(path.join(root, `${topic}-`));
}

function assertNotLive(pid) {
  try { const stat = readFileSync(`/proc/${pid}/stat`, "utf8"); assert.match(stat.slice(stat.lastIndexOf(")") + 2), /^[ZX] /); }
  catch (error) { if (error.code !== "ENOENT") throw error; }
}

it("主循环阻塞不能让真实 Rust 形态子链越过硬截止，包括 detached 孙进程", { timeout: 10000, skip: process.platform !== "linux" }, async (t) => {
  const tmp = path.resolve(".tmp/deadline-hardgate-tests");
  await mkdir(tmp, { recursive: true });
  const dir = await mkdtemp(path.join(tmp, "blocked-parent-"));
  const fixture = path.join(dir, "fixture.mjs");
  const executable = path.join(dir, "fixture.sh");
  const pids = path.join(dir, "pids.json");
  const escaped = path.join(dir, "late-marker");
  const receiptPath = path.join(dir, "watchdog-receipts.jsonl");
  const preload = path.join(dir, "watchdog-receipt-preload.mjs");
  // 父 loop 故意阻塞到总期限之后，及时确认必须直接由独立 watchdog 留存。
  await writeFile(preload, `import { appendFileSync } from 'node:fs';
if (process.argv[1] === ${JSON.stringify(path.join(import.meta.dirname, "rust-test-case-supervisor.mjs"))} && process.argv[2] === '--watchdog') {
  const record = (message) => appendFileSync(${JSON.stringify(receiptPath)}, JSON.stringify({ producer_pid: process.pid, collected_ms: Number(process.hrtime.bigint()) / 1e6, message }) + '\\n');
  process.on('message', (message) => { if (message.kind === 'initialize') record(message); });
  const originalSend = process.send;
  process.send = function (...args) {
    record(args[0]);
    return Reflect.apply(originalSend, this, args);
  };
  const originalNodeOptions = ${JSON.stringify(process.env.NODE_OPTIONS)};
  if (originalNodeOptions === undefined) delete process.env.NODE_OPTIONS;
  else process.env.NODE_OPTIONS = originalNodeOptions;
}
`);
  const source = `#!${process.execPath}
import { spawn } from 'node:child_process';
import fs from 'node:fs';
if (process.argv.includes('--list')) { if (!process.argv.includes('--ignored')) console.log('hanging::case: test'); }
else {
  const child=spawn(process.execPath,['-e',${JSON.stringify(`require('node:fs').writeFileSync(${JSON.stringify(path.join(dir, "grandchild-ready"))},'ready');setTimeout(()=>require('node:fs').writeFileSync(${JSON.stringify(escaped)},'deadline escaped'),1650);setInterval(()=>{},1000)`)}],{detached:true,stdio:'ignore'});
  fs.writeFileSync(${JSON.stringify(pids)},JSON.stringify([process.pid,child.pid]));
  setInterval(()=>{},1000);
}
`;
  await writeFile(fixture, source); await chmod(fixture, 0o755);
  // 清单查询不需启动 Node；case 仍以 exec 保持真实 Node root 的 PID。
  const shellQuote = (value) => `'${value.replaceAll("'", "'\\''")}'`;
  await writeFile(executable, `#!/bin/sh
if [ "$1" = "--list" ]; then
  for argument in "$@"; do
    if [ "$argument" = "--ignored" ]; then exit 0; fi
  done
  printf '%s\\n' 'hanging::case: test'
else
  exec ${shellQuote(process.execPath)} ${shellQuote(fixture)} "$@"
fi
`);
  await chmod(executable, 0o755);
  const logs = [];
  let parentFailure;
  let blockStarted;
  let blockFinished;
  try {
    const result = executeRustTestBinaries({ artifacts: [{ executable, label: "真实自启动形态" }],
      policy: buildRustTestExecutionPolicy(4, 1), cwd: process.cwd(),
      env: { ...process.env, NODE_OPTIONS: [process.env.NODE_OPTIONS, `--import=${pathToFileURL(preload).href}`].filter((option) => option !== undefined).join(" ") },
      run: runBoundedCommand, remainingMs: () => 2200, now: Date.now, log: (line) => logs.push(JSON.parse(line)),
    });
    let executorSettled = false;
    result.then(() => { executorSettled = true; }, () => { executorSettled = true; });
    const rejection = assert.rejects(result, (error) => {
      parentFailure = error;
      assert.match(error.message, /deadline/);
      return true;
    });
    const grandchildReady = path.join(dir, "grandchild-ready");
    const ready = await Promise.race([
      (async () => {
        while (!existsSync(grandchildReady) && !executorSettled) await new Promise((resolve) => setTimeout(resolve, 5));
        return existsSync(grandchildReady);
      })(),
      rejection.then(() => false),
    ]);
    assert.equal(ready, true, "监督结束前真实孙进程必须已确认 ready");
    assert.equal(executorSettled, false, "不能在监督已结束后补做阻塞");
    assert.ok(existsSync(pids), "真实 root/孙进程必须已启动再阻塞主循环");
    blockStarted = Number(process.hrtime.bigint()) / 1e6;
    const blockUntil = performance.now() + 2500;
    while (performance.now() < blockUntil) {}
    blockFinished = Number(process.hrtime.bigint()) / 1e6;
    await rejection;
    assert.equal(existsSync(escaped), false, "独立 watchdog 必须在主循环恢复前终止孙进程");
    const actualPids = JSON.parse(await readFile(pids, "utf8"));
    for (const pid of actualPids) {
      try { const stat = readFileSync(`/proc/${pid}/stat`, "utf8"); assert.match(stat.slice(stat.lastIndexOf(")") + 2), /^[ZX] /); }
      catch (error) { if (error.code !== "ENOENT") throw error; }
    }
    const receipts = (await readFile(receiptPath, "utf8")).trim().split("\n").map((line) => JSON.parse(line));
    const initialize = receipts.find(({ message }) => message.kind === "initialize");
    assert.ok(initialize, "必须记录实际 batch deadline，不能估算执行界");
    assert.ok(Number.isFinite(initialize.message.batchDeadline));
    assert.ok(blockStarted < initialize.message.batchDeadline - 1000, "父 loop 必须在真实 batch 执行界前进入阻塞");
    const domain = receipts.find(({ message }) => message.kind === "log" && message.value.phase === "rust-supervisor-domain");
    assert.ok(domain, "必须由真实独立 watchdog 留存收据");
    const producerPid = domain.message.value.supervisor_pid;
    assert.ok(receipts.every((receipt) => receipt.producer_pid === producerPid), "收据必须来自同一 watchdog");
    const spawnedReceipt = receipts.find(({ message }) => message.kind === "log" && message.value.phase === "rust-case-spawned" && message.value.context === "真实自启动形态 case hanging::case");
    assert.ok(spawnedReceipt, "必须绑定实际 case 的 spawn 收据");
    const spawned = spawnedReceipt.message.value;
    assert.equal(spawned.pid, actualPids[0]);
    assert.ok(spawned.token.startsWith(`${domain.message.value.launcher_pid}:`));
    const confirmedReceipt = receipts.find(({ message }) => message.kind === "log" && message.value.phase === "rust-case-termination-confirmed" && message.value.token === spawned.token && message.value.context === spawned.context);
    assert.ok(confirmedReceipt, "必须在父 loop 恢复前独立确认实际 case 子链终止");
    const confirmed = confirmedReceipt.message.value;
    assert.ok([confirmed.confirmed_ms, confirmed.total_deadline_ms, confirmedReceipt.collected_ms].every(Number.isFinite));
    assert.ok(confirmed.total_deadline_ms <= initialize.message.batchDeadline, "case 总截止不得晚于实际 batch 总截止");
    assert.ok(confirmed.confirmed_ms <= confirmed.total_deadline_ms, JSON.stringify(confirmed));
    assert.ok(confirmedReceipt.collected_ms >= confirmed.confirmed_ms && confirmedReceipt.collected_ms <= confirmed.total_deadline_ms, JSON.stringify(confirmedReceipt));
    const failureReceipt = receipts.at(-1);
    assert.equal(failureReceipt.message.kind, "failure");
    assert.equal(failureReceipt.message.domain_closed, true, "独立 watchdog 必须确认整个 launcher 域关闭");
    assert.match(failureReceipt.message.error.message, /hanging::case.*deadline|deadline.*hanging::case/s);
    assert.doesNotMatch(failureReceipt.message.error.message, /unconfirmed/);
    assert.ok(Number.isFinite(failureReceipt.collected_ms) && failureReceipt.collected_ms <= confirmed.total_deadline_ms, JSON.stringify(failureReceipt));
    assert.ok(blockStarted <= confirmed.confirmed_ms && confirmedReceipt.collected_ms <= failureReceipt.collected_ms && failureReceipt.collected_ms < blockFinished, "独立确认和域关闭收据必须实际产生于父 loop 阻塞期间");
  } finally {
    try {
      t.diagnostic(JSON.stringify({ phase: "blocked-parent-watchdog-receipts", block_started_ms: blockStarted, block_finished_ms: blockFinished,
        parent_error: parentFailure?.stack, parent_logs: logs, pids: existsSync(pids) ? readFileSync(pids, "utf8") : undefined,
        receipt: existsSync(receiptPath) ? readFileSync(receiptPath, "utf8") : undefined }));
    } finally { await rm(dir, { recursive: true, force: true }); }
  }
});

for (const crashWatchdog of [false, true]) it(crashWatchdog
  ? "独立 watchdog 崩溃时父进程停止批次并清理已知 launcher 子链"
  : "launcher 真正 spawn 后卡住且未回报 PID，独立 watchdog 仍可杀完整已知域",
{ timeout: 10000, skip: process.platform !== "linux" }, async () => {
  const dir = await fixtureDirectory(crashWatchdog ? "watchdog-crash" : "unknown-pid");
  const launcherPath = path.join(dir, "launcher.mjs");
  const pidsPath = path.join(dir, "pids.json");
  const marker = path.join(dir, "escaped");
  const childCode = `const {spawn}=require('node:child_process');const fs=require('node:fs');const grand=spawn(process.execPath,['-e',${JSON.stringify(`setTimeout(()=>require('node:fs').writeFileSync(${JSON.stringify(marker)},'escaped'),1200);setInterval(()=>{},1000)`)}],{detached:true,stdio:'ignore'});fs.writeFileSync(${JSON.stringify(pidsPath)},JSON.stringify([process.pid,grand.pid]));setInterval(()=>{},1000);`;
  await writeFile(launcherPath, `import {spawn} from 'node:child_process';import fs from 'node:fs';
const now=()=>Number(process.hrtime.bigint())/1e6;
process.on('message',(message)=>{
 if(message.kind==='initialize'){const started=now();process.send({kind:'arm',token:String(process.pid)+':fixture',started,total:started+1600,execution:started+600,timeoutMs:1600,context:'unknown PID case'});}
 else if(message.kind==='armed'){
  const child=spawn(process.execPath,['-e',${JSON.stringify(childCode)}],{detached:true,stdio:'ignore'});
  ${crashWatchdog ? `const timer=setInterval(()=>{if(fs.existsSync(${JSON.stringify(pidsPath)})){clearInterval(timer);process.send({kind:'log',value:{phase:'fixture-child-ready'}});}},5);` : "const until=now()+2500;while(now()<until){}"}
 }
});process.send({kind:'ready'});
`);
  let supervisorPid;
  const logs = [];
  try {
    await assert.rejects(executeSupervisedRustTestBinaries({ artifacts: [], policy: buildRustTestExecutionPolicy(4, 1),
      cwd: process.cwd(), env: process.env, remainingMs: () => 2800, launcherPath,
      log: (line) => {
        const entry = JSON.parse(line); logs.push(entry);
        if (entry.phase === "rust-supervisor-domain") supervisorPid = entry.supervisor_pid;
        if (crashWatchdog && entry.phase === "fixture-child-ready") process.kill(supervisorPid, "SIGKILL");
      },
    }), crashWatchdog ? /SIGKILL|without a confirmed result/ : /unknown PID case.*deadline/);
    assert.ok(existsSync(pidsPath), "真实 root/孙进程已启动");
    for (const pid of JSON.parse(await readFile(pidsPath, "utf8"))) assertNotLive(pid);
    assert.equal(existsSync(marker), false);
    if (!crashWatchdog) {
      assert.equal(logs.some((entry) => entry.phase === "rust-case-spawned"), false, "不依赖事后 PID 注册");
      const confirmed = logs.find((entry) => entry.phase === "rust-case-termination-confirmed");
      assert.ok(confirmed.confirmed_ms <= confirmed.total_deadline_ms);
    }
  } finally { await rm(dir, { recursive: true, force: true }); }
});

