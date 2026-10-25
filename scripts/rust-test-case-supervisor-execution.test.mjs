import assert from "node:assert/strict";
import { chmod, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { it } from "node:test";
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

it("专用 launcher 保持 exact 一次覆盖和真实多进程并行", { timeout: 10000, skip: process.platform === "win32" }, async () => {
  const dir = await fixtureDirectory("exact-parallel");
  const fixture = path.join(dir, "fixture.mjs");
  const eventsPath = path.join(dir, "events.jsonl");
  await writeFile(fixture, `#!${process.execPath}
import fs from 'node:fs';
if(process.argv.includes('--list')){if(!process.argv.includes('--ignored'))console.log('a: test\\nb: test\\nc: test\\nd: test');}
else {const name=process.argv[2];const emit=(event)=>fs.appendFileSync(${JSON.stringify(eventsPath)},JSON.stringify({event,name,pid:process.pid,t:Number(process.hrtime.bigint())})+'\\n');emit('start');setTimeout(()=>{emit('end');},120);}
`);
  await chmod(fixture, 0o755);
  try {
    const result = await executeRustTestBinaries({ artifacts: [{ executable: fixture, label: "exact fixture" }],
      policy: buildRustTestExecutionPolicy(4, 1), cwd: process.cwd(), env: process.env, remainingMs: () => 8000, now: Date.now });
    assert.equal(result.case_count, 4); assert.equal(result.max_concurrent_cases, 2);
    const events = (await readFile(eventsPath, "utf8")).trim().split("\n").map(JSON.parse).sort((a, b) => a.t - b.t);
    assert.deepEqual(events.filter((entry) => entry.event === "start").map((entry) => entry.name).sort(), ["a", "b", "c", "d"]);
    assert.equal(new Set(events.map((entry) => entry.pid)).size, 4);
    let active = 0; let peak = 0;
    for (const entry of events) { active += entry.event === "start" ? 1 : -1; peak = Math.max(peak, active); }
    assert.equal(peak, 2); assert.equal(active, 0);
  } finally { await rm(dir, { recursive: true, force: true }); }
});

it("真实非零 stderr 保留且取消在途 sibling，不派排队 case", { timeout: 10000, skip: process.platform === "win32" }, async () => {
  const dir = await fixtureDirectory("failure-cancel");
  const fixture = path.join(dir, "fixture.mjs");
  const siblingPid = path.join(dir, "sibling.pid");
  const queued = path.join(dir, "queued");
  await writeFile(fixture, `#!${process.execPath}
import fs from 'node:fs';
if(process.argv.includes('--list')){if(!process.argv.includes('--ignored'))console.log('fail: test\\nsibling: test\\nqueued: test');}
else if(process.argv[2]==='fail'){const timer=setInterval(()=>{if(fs.existsSync(${JSON.stringify(siblingPid)})){clearInterval(timer);process.stderr.write('original Rust diagnostic\\n');process.exitCode=17;}},5);}
else if(process.argv[2]==='sibling'){fs.writeFileSync(${JSON.stringify(siblingPid)},String(process.pid));setInterval(()=>{},1000);}
else {fs.writeFileSync(${JSON.stringify(queued)},'must not start');}
`);
  await chmod(fixture, 0o755);
  try {
    await assert.rejects(executeRustTestBinaries({ artifacts: [{ executable: fixture, label: "failure fixture" }],
      policy: buildRustTestExecutionPolicy(4, 1), cwd: process.cwd(), env: process.env, remainingMs: () => 8000, now: Date.now }),
    /failure fixture case fail failed:.*17.*original Rust diagnostic/s);
    assert.equal(existsSync(queued), false);
    assertNotLive(Number(await readFile(siblingPid, "utf8")));
  } finally { await rm(dir, { recursive: true, force: true }); }
});

it("launcher 在 spawn 返回前异常退出，已知组中的 root 与 detached 孙进程仍清理", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  const dir = await fixtureDirectory("launcher-eof");
  const launcherPath = path.join(dir, "launcher.mjs");
  const pidsPath = path.join(dir, "pids.json");
  const childCode = `const {spawn}=require('node:child_process');const fs=require('node:fs');const grand=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{detached:true,stdio:'ignore'});fs.writeFileSync(${JSON.stringify(pidsPath)},JSON.stringify([process.pid,grand.pid]));setInterval(()=>{},1000);`;
  await writeFile(launcherPath, `import{spawn}from'node:child_process';import fs from'node:fs';const now=()=>Number(process.hrtime.bigint())/1e6;
process.on('message',m=>{if(m.kind==='initialize'){const started=now();process.send({kind:'arm',token:String(process.pid)+':eof',started,total:started+2400,execution:started+1400,timeoutMs:2400,context:'launcher EOF case'});}
else if(m.kind==='armed'){spawn(process.execPath,['-e',${JSON.stringify(childCode)}],{stdio:'ignore',detached:false});const timer=setInterval(()=>{if(fs.existsSync(${JSON.stringify(pidsPath)}))process.exit(29);},5);}});process.send({kind:'ready'});
`);
  try {
    await assert.rejects(executeSupervisedRustTestBinaries({ artifacts: [], policy: buildRustTestExecutionPolicy(4, 1),
      cwd: process.cwd(), env: process.env, remainingMs: () => 4000, launcherPath }), /launcher unexpectedly closed with 29/);
    for (const pid of JSON.parse(await readFile(pidsPath, "utf8"))) assertNotLive(pid);
  } finally {
    if (existsSync(pidsPath)) for (const pid of JSON.parse(await readFile(pidsPath, "utf8"))) {
      try { process.kill(pid, "SIGKILL"); } catch (error) { if (error.code !== "ESRCH") throw error; }
    }
    await rm(dir, { recursive: true, force: true });
  }
});
