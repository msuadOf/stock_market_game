import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { existsSync, readFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { it } from "node:test";
import { earliestCleanupDeadline, executeSupervisedRustTestBinaries, exportRootExistsWithinDeadline, removeExportRootWithinDeadline, waitForLauncherClose } from "./rust-test-case-supervisor.mjs";

async function setup(topic) {
  const root = path.resolve(".tmp/deadline-hardgate-tests");
  await mkdir(root, { recursive: true });
  const dir = await mkdtemp(path.join(root, `${topic}-`));
  return { dir, launcherPath: path.join(dir, "launcher.mjs") };
}

it("成功批次确认 launcher 退出后不再 taskkill 已释放 PID，保持 Windows 契约", { timeout: 10000 }, async () => {
  const { dir, launcherPath } = await setup("confirmed-close");
  await writeFile(launcherPath, "process.on('message',m=>{if(m.kind==='initialize')process.send({kind:'result',value:{case_count:0}});else if(m.kind==='shutdown')process.disconnect();});process.send({kind:'ready'});");
  let kills = 0;
  try {
    const result = await executeSupervisedRustTestBinaries({ artifacts: [], policy: {}, cwd: process.cwd(), env: process.env,
      remainingMs: () => 3000, launcherPath, terminateDomain: async () => { kills += 1; throw new Error("taskkill exit 128: released PID"); } });
    assert.equal(result.case_count, 0); assert.equal(kills, 0);
  } finally { await rm(dir, { recursive: true, force: true }); }
});

it("导出目录清理失败必须使成功批次显式失败", { timeout: 10000 }, async () => {
  const { dir, launcherPath } = await setup("export-cleanup-failure");
  await writeFile(launcherPath, `import fs from 'node:fs';
process.on('message',m=>{if(m.kind==='initialize'){fs.mkdirSync(m.exportRoot,{recursive:true});process.send({kind:'result',value:{case_count:0}});}else if(m.kind==='shutdown')process.disconnect();});process.send({kind:'ready'});`);
  try {
    await assert.rejects(executeSupervisedRustTestBinaries({ artifacts: [], policy: {}, cwd: process.cwd(), env: { ...process.env, TMPDIR: dir },
      remainingMs: () => 3000, launcherPath, cleanupExportRoot: async () => { throw new Error("rm fixture failed"); } }),
    /export cleanup failed.*rm fixture failed.*residual export directories may remain/);
  } finally { await rm(dir, { recursive: true, force: true }); }
});

it("导出目录清理超过剩余期限时立即失败", { timeout: 10000 }, async () => {
  const started = Number(process.hrtime.bigint()) / 1e6;
  await assert.rejects(removeExportRootWithinDeadline({
    root: path.join(os.tmpdir(), "rust-export-deadline-fixture"), deadline: started + 100,
    run: () => new Promise(() => {}),
  }), /export cleanup.*deadline/i);
  assert.ok(Number(process.hrtime.bigint()) / 1e6 - started < 1000);
});

it("导出目录检查只把 ENOENT 视为无需清理", { timeout: 10000 }, async () => {
  const root = path.join(os.tmpdir(), "rust-export-inspect-fixture");
  const deadline = Number(process.hrtime.bigint()) / 1e6 + 1000;
  assert.equal(await exportRootExistsWithinDeadline({ root, deadline, inspect: async () => { throw Object.assign(new Error("missing"), { code: "ENOENT" }); } }), false);
  await assert.rejects(exportRootExistsWithinDeadline({ root, deadline, inspect: async () => { throw Object.assign(new Error("permission denied"), { code: "EACCES" }); } }),
    /export cleanup failed.*EACCES.*residual export directories may remain/);
});

it("launcher close 不确认必须在当前 total 截止报错", { timeout: 10000 }, async () => {
  const started = Number(process.hrtime.bigint()) / 1e6;
  await assert.rejects(waitForLauncherClose(new Promise(() => {}), started + 40), /close remains unconfirmed.*total deadline/);
  assert.ok(Number(process.hrtime.bigint()) / 1e6 - started < 250);
});

it("后发 case 失败不能借用它的较晚预算拖过旧 case 的清理截止", { timeout: 10000 }, async () => {
  const started = Number(process.hrtime.bigint()) / 1e6;
  const older = { total: started + 40 };
  const laterFailure = { total: started + 180 };
  const deadline = earliestCleanupDeadline(started + 300, laterFailure, [older, laterFailure]);
  await assert.rejects(waitForLauncherClose(new Promise(() => {}), deadline), /close remains unconfirmed/);
  assert.ok(Number(process.hrtime.bigint()) / 1e6 - started < 120, "不得等到后发失败 case 的 180ms 截止");
});

it("截止已过也必须保留已发生的 cleanup rejection，不能留下 unhandled Promise", { timeout: 10000 }, async () => {
  const cleanup = Promise.reject(new Error("original cleanup failed at cutoff"));
  await assert.rejects(waitForLauncherClose(cleanup, Number(process.hrtime.bigint()) / 1e6 - 1), /original cleanup failed at cutoff/);
});

it("launcher 报告候选 result 后非零退出仍失败，不能以 result 掩盖异常", { timeout: 10000 }, async () => {
  const { dir, launcherPath } = await setup("result-exit-failure");
  await writeFile(launcherPath, "process.on('message',m=>{if(m.kind==='initialize')process.send({kind:'result',value:{case_count:0}});else if(m.kind==='shutdown'){process.exitCode=19;process.disconnect();}});process.send({kind:'ready'});");
  try {
    await assert.rejects(executeSupervisedRustTestBinaries({ artifacts: [], policy: {}, cwd: process.cwd(), env: process.env,
      remainingMs: () => 2500, launcherPath }), /launcher.*19/);
  } finally { await rm(dir, { recursive: true, force: true }); }
});

it("已过期 finish 及非法 arm 均不能撤销独立截止", { timeout: 10000 }, async () => {
  for (const invalidArm of [false, true]) {
    const { dir, launcherPath } = await setup(invalidArm ? "invalid-arm" : "late-finish");
    await writeFile(launcherPath, `const now=()=>Number(process.hrtime.bigint())/1e6;process.on('message',m=>{
if(m.kind==='initialize'){const started=now();process.send({kind:'arm',token:String(process.pid)+':late',started,total:started+500,execution:${invalidArm ? "'invalid'" : "started+100"},timeoutMs:500,context:'late finish case'});}
else if(m.kind==='armed'){setTimeout(()=>process.send({kind:'finished',token:m.token}),140);}});process.send({kind:'ready'});
`);
    try {
      await assert.rejects(executeSupervisedRustTestBinaries({ artifacts: [], policy: {}, cwd: process.cwd(), env: process.env,
        remainingMs: () => 2000, launcherPath }), invalidArm ? /invalid or duplicate arm token/ : /late finish case.*deadline/);
    } finally { await rm(dir, { recursive: true, force: true }); }
  }
});

it("64 路完成风暴不能堵住独立 watchdog 的挂例截止和孙进程清理", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  const { dir, launcherPath } = await setup("completion-storm");
  const pidsPath = path.join(dir, "pids.json");
  const marker = path.join(dir, "escaped");
  const hanging = `const{spawn}=require('node:child_process');const fs=require('node:fs');const grand=spawn(process.execPath,['-e',${JSON.stringify(`setTimeout(()=>require('node:fs').writeFileSync(${JSON.stringify(marker)},'escaped'),1700);setInterval(()=>{},1000)`)}],{detached:true,stdio:'ignore'});fs.writeFileSync(${JSON.stringify(pidsPath)},JSON.stringify([process.pid,grand.pid]));setInterval(()=>{},1000);`;
  await writeFile(launcherPath, `import{spawn}from'node:child_process';const now=()=>Number(process.hrtime.bigint())/1e6;const pending=new Map();
process.on('message',m=>{if(m.kind==='initialize'){for(let i=0;i<64;i++){const token=String(process.pid)+':'+i;const started=now();pending.set(token,i);process.send({kind:'arm',token,started,total:started+2500,execution:started+1300,timeoutMs:2500,context:i===0?'storm hanging':'storm fast '+i});}}
else if(m.kind==='armed'){const i=pending.get(m.token);const child=spawn(process.execPath,['-e',i===0?${JSON.stringify(hanging)}:'setTimeout(()=>{},250)'],{stdio:'ignore',detached:false,env:{...process.env,STOCK_GAME_RUST_CASE_TOKEN:m.token}});process.send({kind:'spawned',token:m.token,pid:child.pid});child.on('close',code=>{if(code===0)process.send({kind:'finished',token:m.token});});}});process.send({kind:'ready'});
`);
  const logs = [];
  try {
    await assert.rejects(executeSupervisedRustTestBinaries({ artifacts: [], policy: {}, cwd: process.cwd(), env: process.env,
      remainingMs: () => 4500, launcherPath, log: (line) => logs.push(JSON.parse(line)) }), /storm hanging.*deadline/);
    assert.equal(existsSync(marker), false);
    for (const pid of JSON.parse(await readFile(pidsPath, "utf8"))) {
      try { const stat = readFileSync(`/proc/${pid}/stat`, "utf8"); assert.match(stat.slice(stat.lastIndexOf(")") + 2), /^[ZX] /); }
      catch (error) { if (error.code !== "ENOENT") throw error; }
    }
    const confirmed = logs.find((entry) => entry.phase === "rust-case-termination-confirmed");
    assert.ok(confirmed.confirmed_ms <= confirmed.total_deadline_ms, JSON.stringify(confirmed));
    process.stdout.write(`completion-storm ${JSON.stringify(confirmed)}\n`);
  } finally { await rm(dir, { recursive: true, force: true }); }
});
