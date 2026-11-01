import assert from "node:assert/strict";
import { chmod, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { it } from "node:test";
import { runBoundedCommand } from "./run-with-deadline.mjs";
import { buildRustTestExecutionPolicy } from "./run-full-regression.mjs";
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

