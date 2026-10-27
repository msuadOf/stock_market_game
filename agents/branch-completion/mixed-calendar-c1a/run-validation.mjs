// 每个 case 独立进程树 10s；同批使用 8 个并发 worker。
import { readdirSync, mkdirSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { runBoundedCommand } from '../../../scripts/run-with-deadline.mjs';
const evidence = path.dirname(fileURLToPath(import.meta.url));
const cwd = path.resolve(evidence, '../../..');
const artifacts = '/data1/baiyifan/workplace/stock_market_game/.tmp/mixed-calendar-c1a';
const target = path.join(artifacts, 'target');
mkdirSync(artifacts, { recursive: true });
const [phase, label = phase, selection] = process.argv.slice(2);
const env = { ...process.env, CARGO_TARGET_DIR: target, CARGO_BUILD_JOBS: '8',
  CARGO_PROFILE_DEV_CODEGEN_UNITS: '16', CARGO_PROFILE_DEV_DEBUG: '0', CARGO_INCREMENTAL: '0' };
async function run(command, args, timeoutMs, log) {
  const started = Date.now();
  let stdout = '', stderr = '', r;
  try {
    const result = await runBoundedCommand({ command, args, timeoutMs, cwd, env, captureOutput: true,
      onStdout: chunk => { stdout += chunk; }, onStderr: chunk => { stderr += chunk; } });
    r = { exitCode: 0, ...result };
  } catch (error) {
    r = { exitCode: /exhausted.*budget/.test(error.message) ? 124 : 1, stdout, stderr, error: error.message };
  }
  writeFileSync(path.join(evidence, `${log}.log`), JSON.stringify({ command, args, timeoutMs, elapsedMs: Date.now()-started, ...r }, null, 2));
  return r;
}
if (phase === 'build') {
  writeFileSync(path.join(evidence, `${label}-resources.json`), JSON.stringify({ cwd, target, jobs: 8, codegenUnits: 16, debug: 0, incremental: false, sharedDeadlineMs: 300000 }, null, 2));
  const monitor = setInterval(() => {
    const r = spawnSync('ps', ['-C', 'rustc,cc,ld,lld', '-o', 'pid,ppid,pcpu,nlwp,comm'], { encoding: 'utf8', timeout: 1000 });
    if (r.status === 0) writeFileSync(path.join(evidence, `${label}-process-sample.txt`), r.stdout);
  }, 3000);
  try { const r = await run('cargo', ['build', '-p', 'engine', '--lib', '--locked', '-j', '8'], 300000, label); console.log(JSON.stringify(r)); process.exitCode = r.exitCode; }
  finally { clearInterval(monitor); }
} else if (phase === 'harness') {
  const deps = path.join(target, 'debug/deps');
  const thiserror = readdirSync(deps).filter(n => /^libthiserror-.*\.rlib$/.test(n));
  if (thiserror.length !== 1) throw new Error(`预期唯一 thiserror rlib，实际 ${thiserror.length}`);
  const r = await run('rustc', ['--edition=2021', '--test', path.join(evidence, selection ?? 'harness.rs'),
    '--extern', `engine=${target}/debug/libengine.rlib`, '--extern', `thiserror=${deps}/${thiserror[0]}`,
    '-L', `dependency=${deps}`, '-C', 'codegen-units=16', '-C', 'debuginfo=0', '-o', `${artifacts}/${label}`], 300000, `${label}-compile`);
  console.log(JSON.stringify(r)); process.exitCode = r.exitCode;
} else if (phase === 'cases') {
  const binary = `${artifacts}/${label}`;
  const listed = await run(binary, ['--list', '--format', 'terse'], 10000, `${label}-list`);
  if (listed.exitCode !== 0) throw new Error('枚举 case 失败');
  const cases = listed.stdout.split('\n').filter(s => s.endsWith(': test')).map(s => s.slice(0, -6)).filter(name => selection === undefined || name.includes(selection));
  if (cases.length !== (selection === undefined ? 20 : 1)) throw new Error(`case 数量不符：${cases.length}`);
  let next = 0;
  const results = [];
  await Promise.all(Array.from({ length: 8 }, async () => {
    while (next < cases.length) {
      const name = cases[next++];
      const r = await run(binary, [name, '--exact', '--nocapture', '--test-threads=1'], 10000, `${label}-${name.split('::').at(-1)}`);
      results.push({ name, exitCode: r.exitCode });
    }
  }));
  results.sort((a,b) => a.name.localeCompare(b.name));
  writeFileSync(path.join(evidence, `${label}-results.json`), JSON.stringify({ caseDeadlineMs: 10000, concurrency: 8, results }, null, 2));
  console.log(JSON.stringify(results));
  process.exitCode = results.some(r => r.exitCode !== 0) ? 1 : 0;
} else { throw new Error(`未知 phase ${phase}`); }
