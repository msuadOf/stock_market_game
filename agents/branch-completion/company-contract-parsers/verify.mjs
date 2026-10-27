import { runBoundedCommand } from '../../../scripts/run-with-deadline.mjs';
import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { availableParallelism } from 'node:os';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const cwd = fileURLToPath(new URL('../../../', import.meta.url));
const evidence = path.dirname(fileURLToPath(import.meta.url));
const node = '/data1/baiyifan/workplace/stock_market_game/.tmp/node24-env/node';
const modules = ['company-contract-views', 'dividend-tax', 'corporate-action-views'];
const sourcePaths = modules.flatMap(name => [`apps/web/src/host/${name}.ts`, `apps/web/src/host/${name}.test.ts`]);
const hash = relative => createHash('sha256').update(readFileSync(path.join(cwd, relative))).digest('hex');
const sourceSha256 = Object.fromEntries(sourcePaths.map(relative => [relative, hash(relative)]));
const jobs = [
  { name: 'final-tests', args: ['--experimental-strip-types', '--test', '--test-timeout=10000', '--test-concurrency=3', ...sourcePaths.filter(name => name.endsWith('.test.ts'))] },
  { name: 'final-typecheck', args: ['/data1/baiyifan/workplace/stock_market_game/node_modules/.pnpm/typescript@6.0.3/node_modules/typescript/bin/tsc', '--noEmit', '--ignoreConfig', '--target', 'es2023', '--module', 'esnext', '--moduleResolution', 'bundler', '--allowImportingTsExtensions', '--strict', '--skipLibCheck', '--noUnusedLocals', '--noUnusedParameters', '--types', 'node', '--typeRoots', '/data1/baiyifan/workplace/stock_market_game/node_modules/.pnpm/node_modules/@types', ...sourcePaths] },
  { name: 'final-lint', args: ['/data1/baiyifan/workplace/stock_market_game/node_modules/.pnpm/oxlint@1.71.0/node_modules/oxlint/bin/oxlint', '--deny-warnings', ...sourcePaths] },
];
const results = await Promise.all(jobs.map(async job => {
  const startedAt = new Date().toISOString();
  let stdout = '', stderr = '', result;
  try {
    await runBoundedCommand({ command: node, args: job.args, timeoutMs: 10000, cwd, captureOutput: true, onStdout: chunk => { stdout += chunk; }, onStderr: chunk => { stderr += chunk; } });
    result = { exitCode: 0 };
  } catch (error) {
    result = { exitCode: 1, error: String(error) };
  }
  writeFileSync(path.join(evidence, `${job.name}.stdout`), stdout);
  writeFileSync(path.join(evidence, `${job.name}.stderr`), stderr);
  return { ...job, command: node, startedAt, completedAt: new Date().toISOString(), result };
}));
const receipt = { nodeVersion: process.version, availableParallelism: availableParallelism(), independentJobs: jobs.length, testFileConcurrency: 3, caseTimeoutMs: 10000, processTreeDeadlineMs: 10000, sourceSha256, results };
writeFileSync(path.join(evidence, 'final-receipt.json'), JSON.stringify(receipt, null, 2) + '\n');
process.stdout.write(JSON.stringify(receipt, null, 2) + '\n');
if (results.some(({ result }) => result.exitCode !== 0)) process.exitCode = 1;
