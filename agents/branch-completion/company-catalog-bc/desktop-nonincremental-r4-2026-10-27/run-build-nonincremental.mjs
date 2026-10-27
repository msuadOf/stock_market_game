// 本轮独占构建监督器：保留日志，连续 60 秒无 CPU／输出进展时自动停止进程树。
import fs from 'node:fs';
import path from 'node:path';
import { performance } from 'node:perf_hooks';
import { runBoundedCommand } from '/data1/baiyifan/workplace/stock_market_game/.worktree/main-merge-2026-10-25/scripts/run-with-deadline.mjs';

const planPath = process.argv[2];
if (!planPath) throw new Error('必须指定已冻结 build-plan.json');
const plan = JSON.parse(fs.readFileSync(planPath, 'utf8'));
if (plan.environment.CARGO_INCREMENTAL !== '0') throw new Error('本批必须关闭 Cargo incremental，禁止重建已撤权缓存');
const out = path.dirname(path.resolve(planPath));
const stdoutPath = path.join(out, 'build.stdout');
const stderrPath = path.join(out, 'build.stderr');
if (fs.existsSync(path.join(out, 'build-result.json')) || fs.existsSync(stdoutPath)) throw new Error('已有原件，拒绝覆盖');
fs.writeFileSync(stdoutPath, ''); fs.writeFileSync(stderrPath, '');
const sampleFd = fs.openSync(path.join(out, 'build-cpu.jsonl'), 'wx');
const started = performance.now();
let lastProgress = started;
let previousCpu = new Map();
let stopReason = null;
let under40Reported = false;
const abort = new AbortController();
const samples = [];
function processes() {
  const result = new Map();
  for (const entry of fs.readdirSync('/proc')) {
    if (!/^\d+$/.test(entry)) continue;
    try {
      const raw = fs.readFileSync(`/proc/${entry}/stat`, 'utf8');
      const split = raw.lastIndexOf(')'); const fields = raw.slice(split + 2).trim().split(/\s+/);
      result.set(Number(entry), { pid: Number(entry), ppid: Number(fields[1]), state: fields[0], comm: raw.slice(raw.indexOf('(') + 1, split), cpuTicks: Number(fields[11]) + Number(fields[12]), threads: Number(fields[17]) });
    } catch (error) { if (!['ENOENT', 'ESRCH'].includes(error.code)) throw error; }
  }
  const owned = new Set([process.pid]);
  for (;;) {
    const size = owned.size;
    for (const row of result.values()) if (owned.has(row.ppid)) owned.add(row.pid);
    if (size === owned.size) break;
  }
  return [...owned].filter(pid => pid !== process.pid && result.has(pid)).map(pid => result.get(pid));
}
function sample() {
  const now = performance.now();
  const rows = processes();
  let cpuDelta = 0;
  for (const row of rows) cpuDelta += Math.max(0, row.cpuTicks - (previousCpu.get(row.pid) ?? 0));
  previousCpu = new Map(rows.map(row => [row.pid, row.cpuTicks]));
  if (cpuDelta > 0) lastProgress = now;
  const disk = fs.statfsSync(plan.repositoryRoot); const availableGiB = disk.bavail * disk.bsize / 1024 ** 3;
  const record = { elapsedSeconds: (now - started) / 1000, noProgressSeconds: (now - lastProgress) / 1000, cpuDelta, availableGiB, processes: rows, ioPressure: fs.readFileSync('/proc/pressure/io', 'utf8') };
  fs.writeSync(sampleFd, JSON.stringify(record) + '\n'); samples.push(record);
  if (availableGiB < 40 && !under40Reported) { under40Reported = true; console.error('可用空间低于 40GiB', availableGiB); }
  if (availableGiB < 20 || now - lastProgress >= 60000) {
    stopReason = availableGiB < 20 ? 'disk-under-20GiB' : 'no-cpu-or-log-progress-for-60s';
    abort.abort(new Error(stopReason));
  }
}
const ticker = setInterval(() => { try { sample(); } catch (error) { stopReason = 'sampling-error'; abort.abort(error); } }, 500);
let exitCode = 0; let errorText = null;
try {
  await runBoundedCommand({ command: plan.command[0], args: plan.command.slice(1), cwd: plan.cwd, env: {...process.env, ...plan.environment}, timeoutMs: 300000, cleanupReserveMs: 1000, signal: abort.signal, captureOutput: true,
    onStdout(chunk) { fs.appendFileSync(stdoutPath, chunk); lastProgress = performance.now(); },
    onStderr(chunk) { fs.appendFileSync(stderrPath, chunk); lastProgress = performance.now(); }
  });
} catch (error) { exitCode = 1; errorText = error.stack; console.error(error.message); }
finally { clearInterval(ticker); fs.closeSync(sampleFd); }
const result = { exitCode, error: errorText, stopReason, wallSeconds: (performance.now() - started) / 1000, samples: samples.length, under40Reported, ownedTreeSupervision: 'runBoundedCommand/terminateTree', failedTargetPreservedAsEvidence: exitCode !== 0 };
fs.writeFileSync(path.join(out, 'build-result.json'), JSON.stringify(result, null, 2) + '\n');
console.log(JSON.stringify(result)); process.exitCode = exitCode;
