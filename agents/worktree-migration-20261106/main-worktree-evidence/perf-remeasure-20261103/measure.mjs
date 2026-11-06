#!/usr/bin/env node
// 最终 20% 性能复测驱动：ABBA，运行在共享 300s 进程外 supervisor 内。
// 运行中按固定间隔采样整棵进程树的 CPU（utime+stime）与线程状态（含 R 可运行线程）。
// 每轮有独立上限 PERF_PER_RUN_CAP_MS，到点**真正** 对进程组发 SIGKILL，并在
// PERF_CLEANUP_MS 内确认进程树消失；批次总上限由外层共享 supervisor 保证。
//
// 用法（正式）：
//   PERF_CORES=<16核> node scripts/run-long-validation.mjs 300000 -- node <本文件>
// 用法（自测，不跑探针）：
//   PERF_SELFTEST=kill PERF_PER_RUN_CAP_MS=1500 PERF_CLEANUP_MS=3000 node <本文件>
//   PERF_SELFTEST=sample node <本文件>
import { spawn } from "node:child_process";
import fsp from "node:fs/promises";
import path from "node:path";

const ROOT = "<WORKSPACE>";
const BASE = "<TMP>/final-perf-20pct-20261103";
const OUT = "<WORKSPACE>/.worktree/main-merge-2026-10-25/agents/perf-remeasure-20261103/samples";
const SETUP = path.join(ROOT, "agents/engine-max-performance/default-setup.json");
const CORES = process.env.PERF_CORES;
const SAMPLE_INTERVAL_MS = Number(process.env.PERF_SAMPLE_INTERVAL_MS || 100);
const PER_RUN_CAP_MS = Number(process.env.PERF_PER_RUN_CAP_MS || 120000);
const CLEANUP_MS = Number(process.env.PERF_CLEANUP_MS || 5000);
const USER_HZ = 100; // Linux USER_HZ：jiffies → 秒（probe 与宿主同机）

const PLAN = [
  ["1-main", path.join(BASE, "main/release/examples/runtime_sync_policy_probe")],
  ["2-integration", "<TMP>/stock-game-perf-remeasure-93b36/release/examples/runtime_sync_policy_probe"],
  ["3-integration", "<TMP>/stock-game-perf-remeasure-93b36/release/examples/runtime_sync_policy_probe"],
  ["4-main", path.join(BASE, "main/release/examples/runtime_sync_policy_probe")],
];

function fail(m) { throw new Error(m); }

function parseStat(stat) {
  const close = stat.lastIndexOf(")");
  if (close < 0) return null;
  const state = stat.slice(close + 1).trimStart()[0];
  const rest = stat.slice(close + 2).trim().split(/\s+/); // rest[0]=state, rest[11]=utime(14), rest[12]=stime(15)
  const utime = Number(rest[11]); const stime = Number(rest[12]);
  if (!Number.isFinite(utime) || !Number.isFinite(stime)) return null;
  return { state, cpuJiffies: utime + stime };
}

async function sampleTree(rootPid) {
  const pending = [rootPid]; const visited = new Set();
  let rssKiB = 0, totalThreads = 0, runnableThreads = 0, cpuJiffies = 0;
  while (pending.length) {
    const pid = pending.pop();
    if (visited.has(pid)) continue;
    visited.add(pid);
    try {
      const [status, taskEntries] = await Promise.all([
        fsp.readFile(`/proc/${pid}/status`, "utf8"),
        fsp.readdir(`/proc/${pid}/task`, { withFileTypes: true }),
      ]);
      const m = status.match(/^VmRSS:\s+(\d+)\s+kB$/m);
      if (m) rssKiB += Number(m[1]);
      const kids = await Promise.all(taskEntries.filter((e) => e.isDirectory() && /^\d+$/.test(e.name)).map(async (e) => {
        try {
          const [stat, children] = await Promise.all([
            fsp.readFile(`/proc/${pid}/task/${e.name}/stat`, "utf8"),
            fsp.readFile(`/proc/${pid}/task/${e.name}/children`, "utf8"),
          ]);
          const parsed = parseStat(stat);
          if (!parsed) fail(`malformed task stat for pid ${pid} task ${e.name}`);
          return { ...parsed, children };
        } catch (error) { if (error?.code === "ENOENT" || error?.code === "ESRCH") return null; throw error; }
      }));
      for (const t of kids) {
        if (!t) continue;
        totalThreads += 1; cpuJiffies += t.cpuJiffies;
        if (t.state === "R") runnableThreads += 1;
        for (const c of t.children.trim().split(/\s+/).filter(Boolean)) pending.push(Number(c));
      }
    } catch (error) { if (error?.code !== "ENOENT" && error?.code !== "ESRCH") throw error; }
  }
  return { process_count: visited.size, rss_bytes: rssKiB * 1024, total_threads: totalThreads, runnable_threads: runnableThreads, cpu_jiffies: cpuJiffies };
}

function sleep(ms) { return new Promise((r) => setTimeout(r, ms)); }

async function pidAlive(pid) {
  try { const s = await fsp.readFile(`/proc/${pid}/stat`, "utf8"); const st = s.slice(s.lastIndexOf(")") + 2).trimStart()[0]; return st !== "Z" && st !== "X"; }
  catch (error) { if (error.code === "ENOENT" || error.code === "ESRCH") return false; throw error; }
}

async function killTree(pid, deadline) {
  try { process.kill(-pid, "SIGKILL"); } catch (e) { if (e.code !== "ESRCH") throw e; }
  try { process.kill(pid, "SIGKILL"); } catch (e) { if (e.code !== "ESRCH") throw e; }
  while (await pidAlive(pid)) {
    if (Date.now() >= deadline) throw new Error(`process tree of pid ${pid} not confirmed dead within cleanup ${CLEANUP_MS}ms`);
    await sleep(5);
  }
}

async function runOne(label, command, args, { record = true } = {}) {
  const env = { ...process.env, ENGINE_STEP_RUNTIME: "1", ENGINE_SPIN_US: "500" };
  const started = Date.now();
  const child = spawn(command, args, { cwd: "/tmp", env, detached: true, stdio: ["ignore", "pipe", "pipe"] });
  let stdout = "", stderr = "";
  child.stdout.on("data", (c) => { stdout += c; });
  child.stderr.on("data", (c) => { stderr += c; });
  const samples = [];
  let timedOut = false, capKillError = null, capKillMs = null;
  const ended = () => child.exitCode !== null || child.signalCode !== null;
  const exit = new Promise((resolve) => child.on("close", (code, signal) => resolve({ code, signal })));
  const sampler = (async () => {
    while (!ended()) {
      try { samples.push({ t_ms: Date.now() - started, ...(await sampleTree(child.pid)) }); } catch (e) { stderr += `\n[sampler] ${e.message}`; }
      await sleep(SAMPLE_INTERVAL_MS);
    }
  })();
  // 每轮上限：真正发 SIGKILL，而不是只置位。
  const capTimer = setTimeout(() => {
    timedOut = true;
    const t0 = Date.now();
    killTree(child.pid, Date.now() + CLEANUP_MS)
      .catch((e) => { capKillError = e.message; })
      .finally(() => { capKillMs = Date.now() - t0; });
  }, PER_RUN_CAP_MS);
  const res = await exit;
  clearTimeout(capTimer);
  // 若结束异常（非 0 退出 / 被信号终结）也确保残余进程组被清理。
  if (res.code !== 0 || res.signal) {
    try { await killTree(child.pid, Date.now() + CLEANUP_MS); } catch (e) { capKillError = capKillError ?? e.message; }
  }
  await sampler;
  const wallMs = Date.now() - started;

  const cpuJiffiesMax = samples.length ? Math.max(...samples.map((s) => s.cpu_jiffies)) : 0;
  const runnable = samples.map((s) => s.runnable_threads);
  const threads = samples.map((s) => s.total_threads);
  const procs = samples.map((s) => s.process_count);
  const summary = {
    label, command, args, binary: args.includes(SETUP) ? args[args.indexOf(SETUP) - 1] : command,
    exit_code: res.code, signal: res.signal, timed_out: timedOut,
    cap_kill_ms: capKillMs, cap_kill_error: capKillError,
    wall_ms: wallMs, stderr_len: stderr.length,
    sampling: {
      interval_ms: SAMPLE_INTERVAL_MS, sample_count: samples.length,
      process_count: procs.length ? { min: Math.min(...procs), max: Math.max(...procs) } : null,
      total_threads: threads.length ? { min: Math.min(...threads), max: Math.max(...threads) } : null,
      runnable_threads: runnable.length ? { min: Math.min(...runnable), max: Math.max(...runnable), mean: runnable.reduce((a, b) => a + b, 0) / runnable.length, max_observed: Math.max(...runnable) } : null,
      cpu_seconds_total_max: cpuJiffiesMax / USER_HZ,
      cpu_utilization_cores_mean: (cpuJiffiesMax / USER_HZ) / (wallMs / 1000),
    },
    samples,
  };
  if (record) {
    await fsp.writeFile(path.join(OUT, `${label}.sample.json`), JSON.stringify(summary, null, 2) + "\n");
    if (stdout.trim().length) await fsp.writeFile(path.join(OUT, `${label}.json`), stdout);
    await fsp.writeFile(path.join(OUT, `${label}.err`), stderr);
  }
  console.log(`${label} exit=${res.code} signal=${res.signal} wall_ms=${wallMs} samples=${samples.length} runnable_max=${summary.sampling.runnable_threads?.max_observed} cpu_cores_mean=${summary.sampling.cpu_utilization_cores_mean.toFixed(2)} timed_out=${timedOut} cap_kill_ms=${capKillMs}`);
  return { ...summary, res, stdout };
}

async function main() {
  if (!CORES) fail("PERF_CORES 未设置");
  await fsp.mkdir(OUT, { recursive: true });
  console.log(`batch_start=${new Date().toISOString()} cores=${CORES} per_run_cap_ms=${PER_RUN_CAP_MS} cleanup_ms=${CLEANUP_MS}`);
  for (const [label, bin] of PLAN) {
    const r = await runOne(label, "taskset", ["-c", CORES, bin, SETUP, "every_tick", "42", "1"]);
    if (r.exit_code !== 0 || r.timed_out) { console.log(`ABORT at ${label}`); process.exitCode = 2; return; }
  }
  console.log(`batch_finish=${new Date().toISOString()}`);
}

async function selfTestSample() {
  const child = spawn(process.execPath, ["-e", "const t=Date.now()+2000; while(Date.now()<t){}"], { detached: true, stdio: "ignore" });
  const samples = [];
  while (child.exitCode === null && child.signalCode === null) { samples.push(await sampleTree(child.pid)); await sleep(100); }
  const cpuMax = Math.max(...samples.map((s) => s.cpu_jiffies));
  console.log(JSON.stringify({ selftest: "sample", samples: samples.length, total_threads_max: Math.max(...samples.map((s)=>s.total_threads)), runnable_max: Math.max(...samples.map((s)=>s.runnable_threads)), cpu_seconds_max: cpuMax / USER_HZ }, null, 2));
}

// 验证每轮上限确实 kill：起一个挂起子进程，cap=1.5s，须被 SIGKILL 且进程树消失、总耗时受限。
async function selfTestKill() {
  const label = "selftest-kill";
  const r = await runOne(label, process.execPath, ["-e", "setInterval(()=>{},1000)"], { record: false });
  const killedBySignal = r.signal === "SIGKILL" || r.signal === "SIGTERM";
  const ok = r.timed_out === true && r.cap_kill_error === null && killedBySignal && r.wall_ms <= PER_RUN_CAP_MS + CLEANUP_MS + 2000;
  console.log(JSON.stringify({ selftest: "kill", ok, timed_out: r.timed_out, signal: r.signal, cap_kill_ms: r.cap_kill_ms, cap_kill_error: r.cap_kill_error, wall_ms: r.wall_ms, cap_ms: PER_RUN_CAP_MS, cleanup_ms: CLEANUP_MS }, null, 2));
  if (!ok) process.exitCode = 1;
}

if (process.env.PERF_SELFTEST === "kill") {
  selfTestKill().catch((e) => { console.error(`selftest kill FAIL: ${e.message}`); process.exitCode = 1; });
} else if (process.env.PERF_SELFTEST === "sample") {
  selfTestSample().catch((e) => { console.error(`selftest sample FAIL: ${e.message}`); process.exitCode = 1; });
} else {
  main().catch((e) => { console.error(`measure2 FAIL: ${e.message}`); process.exitCode = 1; });
}
