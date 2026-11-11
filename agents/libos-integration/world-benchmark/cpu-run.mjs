// 独立进程树 CPU 采样；结果仅为 ps 报告值，不当作精确积分或功耗。
import {execFile, spawn} from 'node:child_process';
import {promisify} from 'node:util';
import {readFile, writeFile} from 'node:fs/promises';
import {performance} from 'node:perf_hooks';
import path from 'node:path';
import {runBoundedCommand} from '../../../scripts/run-with-deadline.mjs';
import {parseFineWaitUs} from './wait-config.mjs';
import {parseCpuSnapshot, aggregateDescendants, verifyCpuResult} from './cpu-samples.mjs';
import {failureDetail} from './failure-detail.mjs';

const fine = parseFineWaitUs(process.argv[2]);
const workerMode = process.env.LIBOS_DIAGNOSTIC_WORKER ?? 'std';
if (workerMode !== 'std' && workerMode !== 'libos') throw Error('下行模式必须是 std 或 libos');
const mathMode = process.env.LIBOS_DIAGNOSTIC_MATH ?? 'std';
if (mathMode !== 'std' && mathMode !== 'libm') throw Error('观察概率模式必须是 std 或 libm');
const output = process.argv[3];
if (process.platform !== 'darwin') throw Error('此 CPU 探针仅验证过 macOS 进程树及清理语义');
if (process.argv.length !== 4 || !output || !path.isAbsolute(output)) throw Error('需要 fine 微秒与绝对输出路径');
let rootPid, timer, inFlight, phase = 'before', pending = '', failure;
const samples = [], markers = [], started = performance.now(), controller = new AbortController();
const runPs = promisify(execFile);
function consumeStderr(chunk) {
  pending += String(chunk);
  const lines = pending.split('\n'); pending = lines.pop();
  for (const line of lines) {
    if (line === 'LIBOS_CPU_WINDOW:cpu-sample-start') {
      if (phase !== 'before') throw Error('CPU 采样开始标记重复或错序');
      phase = 'sample'; markers.push({phase, atMs:performance.now() - started});
    } else if (line === 'LIBOS_CPU_WINDOW:cpu-sample-end') {
      if (phase !== 'sample') throw Error('CPU 采样结束标记错序');
      phase = 'after'; markers.push({phase, atMs:performance.now() - started});
    }
  }
}
async function collect() {
  const from = performance.now() - started, initial = phase;
  const {stdout} = await runPs('/bin/ps', ['-axo', 'pid=,ppid=,pcpu=,rss='], {timeout:2000, maxBuffer:4 * 1024 * 1024});
  const rows = parseCpuSnapshot(stdout);
  if (!rows.some(row => row.pid === rootPid)) {
    samples.push({fromMs:from, toMs:performance.now() - started, phaseStart:initial, phaseEnd:phase, rootMissing:true});
    return;
  }
  samples.push({fromMs:from, toMs:performance.now() - started, phaseStart:initial, phaseEnd:phase, ...aggregateDescendants(rows, rootPid)});
}
function schedule() {
  if (inFlight) return;
  inFlight = collect().catch(error => { failure = error; controller.abort(error); }).finally(() => { inFlight = undefined; });
}
try {
  const env = {...process.env, LIBOS_DIAGNOSTIC_MODE:'libos', LIBOS_DIAGNOSTIC_FINE_US:String(fine), LIBOS_DIAGNOSTIC_OUTPUT:`${output}.result.json`};
  await runBoundedCommand({command:process.execPath, args:['agents/libos-integration/world-benchmark/run.mjs'], env, timeoutMs:120000, signal:controller.signal, captureOutput:true,
    spawnProcess:(...args) => { const child = spawn(...args); rootPid = child.pid; timer = setInterval(schedule, 200); return child; },
    onStderr:consumeStderr,
  });
  if (phase !== 'after') throw Error('未观测完整 CPU 采样窗口');
} catch (error) { failure = failure ?? error; }
finally {
  if (timer) clearInterval(timer);
  if (inFlight) await inFlight;
}
const inside = samples.filter(sample => sample.phaseStart === 'sample' && sample.phaseEnd === 'sample' && !sample.rootMissing);
if (!failure && inside.length < 3) failure = Error('采样窗口内进程树样本少于 3 次');
let resultSha256 = null;
if (!failure) {
  try { resultSha256 = verifyCpuResult(await readFile(`${output}.result.json`), fine, workerMode, mathMode); }
  catch (error) { failure = error; }
}
const record = {workerMode, mathMode, fineWaitUs:fine, rootPid, intervalMs:200, markers, samples, sampleWindowCount:inside.length, resultSha256, error:failure ? failureDetail(failure) : null};
await writeFile(`${output}.cpu.json`, JSON.stringify(record, null, 2), {flag:'wx'});
if (failure) throw failure;
console.log(JSON.stringify({fine, sampleWindowCount:inside.length, cpuFile:`${output}.cpu.json`}));
