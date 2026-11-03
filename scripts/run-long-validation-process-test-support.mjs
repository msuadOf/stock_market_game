#!/usr/bin/env node
// 仅供 run-long-validation-process.test.mjs 使用的独立终态观察器。
// 本进程在 spawn 前记录同机可比较的 process.hrtime.bigint()，轮询 /proc 的
// PID+birth 与 Z/X/消失时刻；它不参与被测 CLI 的 deadline 预算，也不相信 CLI
// 自报的退出时间。报告延迟只影响本观察器，不影响 1600ms 硬门。
import { spawn } from "node:child_process";
import { once } from "node:events";
import fs from "node:fs";
import fsp from "node:fs/promises";
import path from "node:path";

const [mode, sourceRoot, dir] = process.argv.slice(2);
if (!["retained-child", "slow-proc"].includes(mode) || !sourceRoot || !dir) {
  throw new Error("用法：run-long-validation-process-test-support.mjs <retained-child|slow-proc> <sourceRoot> <dir>");
}
const hrtime = () => process.hrtime.bigint();
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
async function withTimeout(promise, timeoutMs) {
  let timer;
  try {
    return await Promise.race([promise, new Promise((resolve) => { timer = setTimeout(() => resolve(undefined), timeoutMs); })]);
  } finally {
    clearTimeout(timer);
  }
}
const readyPath = path.join(dir, "observer-ready");
const goPath = path.join(dir, "observer-go");
const recordPath = path.join(dir, "owned-child.json");
const markerPath = path.join(dir, "late-marker");
const hitPath = path.join(dir, "slow-proc-hits.jsonl");
const reportPath = path.join(dir, "observer-report.json");
const report = (value) => fsp.writeFile(reportPath, JSON.stringify(value));
const identity = async (pid) => {
  const raw = await fsp.readFile(`/proc/${pid}/stat`, "utf8");
  const fields = raw.slice(raw.lastIndexOf(")") + 2).split(" ");
  return { state: fields[0], birth: fields[19] };
};
const waitForFile = async (target, deadline) => {
  while (hrtime() < deadline) {
    if (fs.existsSync(target)) return true;
    await sleep(1);
  }
  return false;
};

let worker;
let owned;
let started;
let readyAt;
let cliTerminal;
let ownedTerminal;
let timedOut = false;
let stderr = "";
try {
  fs.writeFileSync(readyPath, String(hrtime()));
  readyAt = hrtime();
  const goDeadline = hrtime() + 2_000_000_000n;
  if (!await waitForFile(goPath, goDeadline)) throw new Error("父测试未在 2000ms 内发送 observer-go");
  const childCode = `const fs=require('node:fs');const raw=fs.readFileSync('/proc/'+process.pid+'/stat','utf8');fs.writeFileSync(${JSON.stringify(recordPath)},JSON.stringify({pid:process.pid,birth:raw.slice(raw.lastIndexOf(')')+2).split(' ')[19]}));setTimeout(()=>fs.writeFileSync(${JSON.stringify(markerPath)},String(Number(process.hrtime.bigint())/1e6)),1800);setInterval(()=>{},1000);`;
  started = hrtime();
  worker = spawn(process.execPath, [path.join(sourceRoot, "scripts/run-long-validation.mjs"), "1600", "--", process.execPath, "-e", childCode], {
    cwd: sourceRoot,
    env: {
      ...process.env,
      STOCK_GAME_FIXTURE_DEADLINE_MODE: mode,
      STOCK_GAME_FIXTURE_DEADLINE_SOURCE: sourceRoot,
      STOCK_GAME_FIXTURE_PROC_HITS: hitPath,
      NODE_OPTIONS: `--import=${path.join(dir, "deadline-preload.mjs")}`,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  worker.stderr.on("data", (chunk) => { stderr += chunk; });
  worker.stdout.resume();
  const closed = once(worker, "close");
  let cliBirth;
  const bindDeadline = hrtime() + 200_000_000n;
  while (hrtime() < bindDeadline) {
    try { cliBirth = (await identity(worker.pid)).birth; break; }
    catch (error) {
      if (error.code !== "ENOENT" && error.code !== "ESRCH") throw error;
      await sleep(1);
    }
  }
  if (cliBirth === undefined) throw new Error("200ms 内未能绑定 CLI PID+birth");
  const recordDeadline = started + 2_000_000_000n;
  while (hrtime() < recordDeadline) {
    try { owned = JSON.parse(await fsp.readFile(recordPath, "utf8")); break; }
    catch (error) { if (error.code !== "ENOENT" && error.code !== "EACCES" && !(error instanceof SyntaxError)) throw error; }
    await sleep(1);
  }
  if (owned === undefined) throw new Error("2000ms 内未观察到 owned child 身份记录");

  const observe = async (pid, expectedBirth) => {
    let expected = expectedBirth;
    while (hrtime() < started + 3_000_000_000n) {
      const before = hrtime();
      try {
        const current = await identity(pid);
        if (expected === undefined) expected = current.birth;
        else if (current.birth !== expected) {
          return { kind: "identity-changed", seen: true, birth: current.birth, before: String(before), after: String(hrtime()) };
        }
        if (current.state === "Z" || current.state === "X") {
          return { kind: current.state, seen: true, birth: current.birth, before: String(before), after: String(hrtime()) };
        }
      } catch (error) {
        if (error.code === "ENOENT" || error.code === "ESRCH") {
          return { kind: "disappeared", seen: expected !== undefined, birth: expected ?? null, before: String(before), after: String(hrtime()) };
        }
        throw error;
      }
      await sleep(1);
    }
    return { kind: "still-live", seen: expected !== undefined, birth: expected ?? null, before: String(hrtime()), after: String(hrtime()) };
  };

  [cliTerminal, ownedTerminal] = await Promise.all([
    observe(worker.pid, cliBirth),
    observe(owned.pid, owned.birth),
  ]);
  if (cliTerminal.kind === "still-live" || ownedTerminal.kind === "still-live") {
    timedOut = true;
    try { worker.kill("SIGKILL"); } catch (error) { if (error.code !== "ESRCH") throw error; }
    try {
      const current = await identity(owned.pid);
      if (current.birth === owned.birth && current.state !== "Z" && current.state !== "X") process.kill(owned.pid, "SIGKILL");
    } catch (error) { if (error.code !== "ENOENT" && error.code !== "ESRCH") throw error; }
  }
  const close = await withTimeout(closed, 1_000);
  if (close === undefined) {
    timedOut = true;
    try { worker.kill("SIGKILL"); } catch (error) { if (error.code !== "ESRCH") throw error; }
    await withTimeout(closed, 1_000);
  }
  const hits = fs.existsSync(hitPath)
    ? fs.readFileSync(hitPath, "utf8").trim().split("\n").filter(Boolean).map((line) => JSON.parse(line))
    : [];
  await report({
    mode,
    readyAt: String(readyAt),
    started: String(started),
    cliPid: worker.pid,
    cliBirth,
    cliTerminal,
    cliTerminalMs: cliTerminal === undefined ? null : Number(BigInt(cliTerminal.after) - started) / 1e6,
    owned,
    ownedTerminal,
    ownedTerminalMs: ownedTerminal === undefined ? null : Number(BigInt(ownedTerminal.after) - started) / 1e6,
    markerExists: fs.existsSync(markerPath),
    timedOut,
    exitCode: worker.exitCode,
    signalCode: worker.signalCode,
    stderr,
    hits,
  });
} catch (error) {
  await report({ mode, error: error.stack ?? String(error), readyAt: readyAt === undefined ? null : String(readyAt), started: started === undefined ? null : String(started), timedOut, stderr });
  try { worker?.kill("SIGKILL"); } catch (killError) { if (killError.code !== "ESRCH") throw killError; }
  process.exitCode = 1;
}
