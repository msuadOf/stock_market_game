// 仅运行独占纯模块 harness；每个 case 与整个执行批次均受进程外 10000ms deadline。
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { runBoundedCommand } from '../../../scripts/run-with-deadline.mjs';

const evidence = path.dirname(fileURLToPath(import.meta.url));
const cwd = path.resolve(evidence, '../../..');
const artifacts = '/data1/baiyifan/workplace/stock_market_game/.tmp/mixed-calendar-c2a';
const dependencyTarget = '/data1/baiyifan/workplace/stock_market_game/.tmp/mixed-calendar-c1a/target/debug';
const rlib = path.join(dependencyTarget, 'libengine.rlib');
const thiserror = path.join(dependencyTarget, 'deps/libthiserror-44f263b99c8e22f7.rlib');
const [phase, label] = process.argv.slice(2);
if (!['compile', 'cases'].includes(phase) || !/^[a-z0-9-]+$/.test(label ?? '')) throw new Error('需要 compile|cases 及唯一 label');
mkdirSync(artifacts, { recursive: true });
const binary = path.join(artifacts, label);
function fingerprint(file) {
  const data = readFileSync(file);
  return { bytes: data.length, sha256: createHash('sha256').update(data).digest('hex') };
}
const files = [
  'packages/engine/src/session/exchange_calendar_model.rs',
  'packages/engine/src/session/market_calendar_context.rs',
  'packages/engine/src/session/market_calendar_context_tests.rs',
  'packages/engine/src/session.rs',
  'agents/branch-completion/mixed-calendar-c2a/harness.rs',
  'agents/branch-completion/mixed-calendar-c2a/run-validation.mjs',
].map(file => path.join(cwd, file));
const sourceBefore = Object.fromEntries(files.map(file => [file, fingerprint(file)]));
const dependencyFiles = Object.fromEntries([rlib, thiserror].map(file => [file, fingerprint(file)]));
if (dependencyFiles[rlib].sha256 !== 'e64df868878f8b575522ad454bf0d44928267504263aab427e7b85ce3b29df3d') throw new Error('真实 C1a rlib 指纹不符');
if (sourceBefore[files[0]].sha256 !== '173c0633b3489a852612458832046f3106e21b28c0c571d6f2ede90f8e33caf5') throw new Error('本树 C1a 源码指纹不符');
async function run(command, args, log) {
  const started = Date.now();
  let stdout = '', stderr = '', result;
  try {
    result = { exitCode: 0, ...await runBoundedCommand({ command, args, timeoutMs: 10000, cwd, captureOutput: true,
      onStdout: chunk => { stdout += chunk; }, onStderr: chunk => { stderr += chunk; } }) };
  } catch (error) {
    const exited = /^bounded command exited with (\d+)/.exec(error.message);
    result = { exitCode: exited ? Number(exited[1]) : /deadline|budget/.test(error.message) ? 124 : 1,
      stdout, stderr, error: error.message };
  }
  const record = { command, args, timeoutMs: 10000, elapsedMs: Date.now() - started, ...result };
  writeFileSync(path.join(evidence, `${log}.json`), JSON.stringify(record, null, 2));
  return record;
}

if (phase === 'compile') {
  const samples = [];
  const monitor = setInterval(() => {
    const processList = spawnSync('ps', ['-eo', 'pid,ppid,pcpu,nlwp,args'], { encoding: 'utf8', timeout: 1000 });
    if (processList.error || processList.status !== 0) throw new Error(`进程采样失败：${processList.error?.message ?? processList.stderr}`);
    const lines = processList.stdout.trim().split('\n');
    const rows = lines.slice(1).map(line => ({ line, fields: line.trim().split(/\s+/) }));
    const owned = new Set(rows.filter(row => row.line.includes('rustc --edition=2021 --test') && row.line.includes(path.join(evidence, 'harness.rs'))).map(row => row.fields[0]));
    let changed = true;
    while (changed) {
      changed = false;
      for (const row of rows) if (owned.has(row.fields[1]) && !owned.has(row.fields[0])) { owned.add(row.fields[0]); changed = true; }
    }
    samples.push({ at: new Date().toISOString(), processes: rows.filter(row => owned.has(row.fields[0])).map(row => row.line) });
  }, 200);
  try {
    const result = await run('rustc', ['--edition=2021', '--test', path.join(evidence, 'harness.rs'),
      '--extern', `engine=${rlib}`, '--extern', `thiserror=${thiserror}`,
      '-L', `dependency=${dependencyTarget}/deps`, '-C', 'codegen-units=16', '-C', 'debuginfo=0', '-o', binary], `${label}-compile`);
    const sourceAfter = Object.fromEntries(files.map(file => [file, fingerprint(file)]));
    if (JSON.stringify(sourceAfter) !== JSON.stringify(sourceBefore)) throw new Error('编译期间源码漂移');
    const dependenciesAfter = Object.fromEntries([rlib, thiserror].map(file => [file, fingerprint(file)]));
    if (JSON.stringify(dependencyFiles) !== JSON.stringify(dependenciesAfter)) throw new Error('编译期间依赖漂移');
    writeFileSync(path.join(evidence, `${label}-compile-manifest.json`), JSON.stringify({
      classification: '轻量纯模块编译，不是完整 engine crate 构建', sourceBefore, sourceAfter,
      dependencyFiles, dependencyAuthorization: '2026-10-26 root 明确批准只读复用 C1a rlib；其构建属于 C1a red-build，非本片构建',
      codegenUnits: 16, commandDeadlineMs: 10000, samples,
      binary: result.exitCode === 0 ? { path: binary, ...fingerprint(binary) } : null,
    }, null, 2));
    console.log(JSON.stringify({ label, exitCode: result.exitCode, elapsedMs: result.elapsedMs }));
    process.exitCode = result.exitCode;
  } finally { clearInterval(monitor); }
} else {
  const manifest = JSON.parse(readFileSync(path.join(evidence, `${label}-compile-manifest.json`), 'utf8'));
  if (JSON.stringify(manifest.sourceAfter) !== JSON.stringify(sourceBefore)) throw new Error('当前源码与编译 manifest 不一致');
  if (fingerprint(binary).sha256 !== manifest.binary?.sha256) throw new Error('binary 与编译 manifest 不一致');
  const listed = await run(binary, ['--list', '--format', 'terse'], `${label}-list`);
  if (listed.exitCode !== 0) throw new Error('枚举测试失败');
  const cases = listed.stdout.split('\n').filter(line => line.startsWith('market_calendar_context::tests::') && line.endsWith(': test')).map(line => line.slice(0, -6));
  if (cases.length !== 13 || new Set(cases).size !== 13) throw new Error(`C2-A case 数量不符：${cases.length}`);
  const results = [];
  let next = 0;
  await Promise.all(Array.from({ length: 8 }, async () => {
    while (next < cases.length) {
      const name = cases[next++];
      const result = await run(binary, [name, '--exact', '--nocapture', '--test-threads=1'], `${label}-${name.split('::').at(-1)}`);
      results.push({ name, exitCode: result.exitCode, elapsedMs: result.elapsedMs });
    }
  }));
  const sourceAfter = Object.fromEntries(files.map(file => [file, fingerprint(file)]));
  if (JSON.stringify(sourceAfter) !== JSON.stringify(sourceBefore)) throw new Error('运行期间源码漂移');
  const record = { concurrency: 8, caseDeadlineMs: 10000, batchExternalDeadlineMs: 10000,
    binary: manifest.binary, sourceBefore, sourceAfter, results: results.sort((a, b) => a.name.localeCompare(b.name)) };
  writeFileSync(path.join(evidence, `${label}-results.json`), JSON.stringify(record, null, 2));
  console.log(JSON.stringify(record.results));
  process.exitCode = results.some(result => result.exitCode !== 0) ? 1 : 0;
}
