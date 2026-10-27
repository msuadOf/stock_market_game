import { runBoundedCommand } from '../../../scripts/run-with-deadline.mjs';
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { availableParallelism } from 'node:os';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const cwd = fileURLToPath(new URL('../../../', import.meta.url));
const evidence = path.dirname(fileURLToPath(import.meta.url));
const node = '/data1/baiyifan/workplace/stock_market_game/.tmp/node24-env/node';
const names = ['CompanyContractPanel', 'DividendTaxPanel', 'PreferenceRejectionsPanel', 'RightsSubscriptionRejectionPanel', 'use-company-query'];
const files = names.flatMap(name => [`apps/web/src/components/company/${name}.${name === 'use-company-query' ? 'ts' : 'tsx'}`, `apps/web/src/components/company/${name}.test.ts`]);
const tests = files.filter(file => file.endsWith('.test.ts'));
const hash = file => createHash('sha256').update(readFileSync(path.join(cwd, file))).digest('hex');
const discovered = new Set(files);
function visit(relative) {
  const source = readFileSync(path.join(cwd, relative), 'utf8');
  const imports = source.matchAll(/(?:from\s*|import\s*\()\s*["'](\.[^"']+)["']/g);
  for (const match of imports) {
    const base = path.posix.normalize(path.posix.join(path.posix.dirname(relative), match[1]));
    const target = [base, `${base}.ts`, `${base}.tsx`, `${base}/index.ts`].find(candidate => /\.tsx?$/.test(candidate) && existsSync(path.join(cwd, candidate)));
    if (target !== undefined && !discovered.has(target)) { discovered.add(target); visit(target); }
  }
}
files.forEach(visit);
const closure = [...discovered].sort();
const jobs = [
  { name: 'final-tests-node24', args: ['--test', '--test-timeout=10000', '--test-concurrency=5', ...tests] },
  { name: 'final-typecheck-node24', args: ['apps/web/node_modules/typescript/bin/tsc', '--noEmit', '--strict', '--skipLibCheck', '--target', 'ES2023', '--module', 'ESNext', '--moduleResolution', 'Bundler', '--allowImportingTsExtensions', '--jsx', 'react-jsx', '--esModuleInterop', '--types', 'node', '--typeRoots', '/data1/baiyifan/workplace/stock_market_game/node_modules/.pnpm/node_modules/@types', '--noUnusedLocals', '--noUnusedParameters', ...files] },
  { name: 'final-lint-node24', args: ['apps/web/node_modules/oxlint/bin/oxlint', '--deny-warnings', ...files] },
];
const sourceSha256 = Object.fromEntries(files.map(file => [file, hash(file)]));
const typeClosureSha256 = Object.fromEntries(closure.map(file => [file, hash(file)]));
const results = await Promise.all(jobs.map(async job => {
  const startedAt = new Date().toISOString(); let stdout = '', stderr = '', result;
  try {
    await runBoundedCommand({ command: node, args: job.args, timeoutMs: 10000, cwd, captureOutput: true, onStdout: chunk => { stdout += chunk; }, onStderr: chunk => { stderr += chunk; } }); result = { exitCode: 0 };
  } catch (failure) { result = { exitCode: 1, error: String(failure) }; }
  writeFileSync(path.join(evidence, `${job.name}.stdout`), stdout);
  writeFileSync(path.join(evidence, `${job.name}.stderr`), stderr);
  return { ...job, command: node, startedAt, completedAt: new Date().toISOString(), result };
}));
const unchanged = files.every(file => sourceSha256[file] === hash(file)) && closure.every(file => typeClosureSha256[file] === hash(file));
const receipt = { nodeVersion: process.version, availableParallelism: availableParallelism(), independentJobs: 3, testFileConcurrency: 5, caseTimeoutMs: 10000, processTreeDeadlineMs: 10000, sourceSha256, typeClosureSha256, unchanged,
  base: '3984e14167202dc645ec340fbd2a73e661c71c8a', parserCandidateSource: '369a6ed45a9620004e863cec0aa23c1ee3952ce8', parserLocalCherryPick: '12c468b7', parserIndependentReview: '待协调确认', results };
writeFileSync(path.join(evidence, 'final-receipt.json'), JSON.stringify(receipt, null, 2) + '\n');
if (!unchanged || results.some(({ result }) => result.exitCode !== 0)) process.exitCode = 1;
process.stdout.write(JSON.stringify({ unchanged, results }, null, 2) + '\n');
