import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, symlink, unlink, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { chromium } from '../../../apps/web/node_modules/@playwright/test/index.mjs';
import { createServer } from '../../../apps/web/node_modules/vite/dist/node/index.js';
import { evaluateBoundaryCosts } from '../../../scripts/performance/libos-boundary-cost.mjs';
import { parseCpuSet } from '../../../scripts/performance/libos-collect-performance.mjs';
import { assertRetiredSample, profiles, summarizeBrowserWorkerCost } from './report.mjs';

const [name, output, browserBin, policyPath] = process.argv.slice(2);
const pkg = process.env.WASM_PKG;
assert.ok(Object.hasOwn(profiles, name) && [3, 4].includes(process.argv.length - 2) &&
  [pkg, output, browserBin, ...(policyPath === undefined ? [] : [policyPath])]
    .every((entry) => typeof entry === 'string' && path.isAbsolute(entry)),
  '用法：WASM_PKG=<绝对目录> node run.mjs <spawn-join|message-handoff|shutdown-wake|deferred> <报告路径> <Chromium路径> [事先预算JSON]');
const profile = profiles[name];
const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');
const readArtifact = () => Promise.all(['web_wasm_bg.wasm', 'web_wasm.js'].map((file) => readFile(path.join(pkg, file))));
const artifact = await readArtifact();
const policyBytes = policyPath === undefined ? undefined : await readFile(policyPath);
const policy = policyBytes === undefined ? undefined : JSON.parse(policyBytes);
const cpuSet = parseCpuSet(await readFile('/proc/self/status', 'utf8'));
const link = 'agents/libos-contract/browser-perf/pkg';
await symlink(pkg, link, 'dir');
let server, browser;
try {
  server = await createServer({
    configFile: false, root: process.cwd(),
    resolve: { alias: [{ find: /^env$/, replacement: path.resolve('apps/web/src/host/engine-clock-env.ts') }] },
    optimizeDeps: { noDiscovery: true, include: [] },
    server: { host: '127.0.0.1', port: 0, watch: null, hmr: false, fs: { allow: [process.cwd(), pkg] },
      headers: { 'Cross-Origin-Opener-Policy': 'same-origin', 'Cross-Origin-Embedder-Policy': 'require-corp' } },
  });
  await server.listen();
  const { port } = server.httpServer.address();
  browser = await chromium.launch({ executablePath: browserBin, headless: true, timeout: 20_000 });
  const context = await browser.newContext();
  const openPage = async () => {
    const page = await context.newPage();
    await page.goto(`http://127.0.0.1:${port}/agents/libos-contract/browser-perf/index.html`, { timeout: 8_000 });
    await page.waitForFunction(() => typeof window.libosBrowserPerf?.sample === 'function', undefined, { timeout: 8_000 });
    return page;
  };
  const persistent = await openPage();
  const measuredResolution = await persistent.evaluate(() => window.libosBrowserPerf.timerResolutionNs());
  let resolutionNs = measuredResolution;
  const take = async (wrapped) => {
    // Reclaimer 的 owner close 是本 Memory 的终态。每臂独立 Memory，不能偷偷重用已退休 owner。
    const page = name === 'deferred' ? await openPage() : persistent;
    try {
      if (name === 'deferred') {
        resolutionNs = Math.max(resolutionNs, await page.evaluate(() => window.libosBrowserPerf.timerResolutionNs()));
      }
      const witness = await page.evaluate(({ name, wrapped, operations }) =>
        window.libosBrowserPerf.sample(name, wrapped, operations),
      { name, wrapped, operations: profile.operations });
      assertRetiredSample(profile, witness);
      return witness;
    } finally {
      if (page !== persistent) await page.close();
    }
  };
  await take(false);
  await take(true);
  const pairs = [];
  for (let i = 0; i < 31; i++) {
    const forward = i % 2 === 0;
    const first = await take(!forward);
    const second = await take(forward);
    pairs.push({ order: forward ? 'direct-facade' : 'facade-direct', operations: profile.operations,
      direct: forward ? first : second, facade: forward ? second : first });
  }
  const operationStats = summarizeBrowserWorkerCost(name, pairs, resolutionNs);
  const budgetEvaluation = policy === undefined ? { verdict: 'not-asserted' } : {
    ...evaluateBoundaryCosts(operationStats, 'browser', policy), budgetSha256: hash(policyBytes),
  };
  assert.deepEqual(await readArtifact(), artifact, '构建制品在采样期间改变');
  await writeFile(output, JSON.stringify({ schema: 'libos-shared-worker-boundary-cost-v1', name, cpuSet,
    browserVersion: browser.version(), artifact: { wasm: hash(artifact[0]), glue: hash(artifact[1]) },
    resolutionNs, budgetEvaluation, operationStats, pairs,
  }, null, 2) + '\n', { flag: 'wx' });
  assert.ok(policy === undefined || budgetEvaluation.verdict === 'pass', 'Browser 性能预算未通过');
} finally {
  try {
    if (browser) await browser.close();
  } finally {
    try {
      if (server) await server.close();
    } finally {
      await unlink(link);
    }
  }
}
