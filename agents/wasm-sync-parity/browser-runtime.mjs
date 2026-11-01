// 在无 query 参数的真实浏览器中验证默认 Worker/WASM runtime。
import assert from 'node:assert/strict';
import { availableParallelism } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from '../../apps/web/node_modules/@playwright/test/index.mjs';
import { createServer } from '../../apps/web/node_modules/vite/dist/node/index.js';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const server = await createServer({ root: path.join(root, 'apps/web'), mode: 'e2e', appType: 'custom', server: { host: '127.0.0.1', port: 0, fs: { allow: [root, '/tmp/wasm-sync-verified/wasm/pkg'] } } });
server.middlewares.use((request, response, next) => {
  if (request.url !== '/runtime-check') return next();
  response.setHeader('Content-Type', 'text/html');
  response.end('<!doctype html><title>Runtime 验收</title>');
});
let browser;
try {
  await server.listen();
  browser = await chromium.launch({ headless: true });
  const modes = ['every_tick', 'day_end', 'on_demand'];
  console.log(JSON.stringify({ classification: '真实 Browser 长验收', parallelCases: modes.length, availableCpus: availableParallelism(), sharedDeadlineMs: 300000 }));
  const outcomes = await Promise.allSettled(modes.map(async (policy) => {
    const page = await browser.newPage();
    page.setDefaultTimeout(10000);
    try {
      await page.goto(`${server.resolvedUrls.local[0]}runtime-check`);
      return await page.evaluate(async (policy) => {
        if (!crossOriginIsolated || location.search !== '') throw new Error('浏览器隔离或无 query 前提不成立');
        const { createWorkerHost } = await import('/src/host/worker-host.ts');
        const { DEFAULT_SETUP } = await import('/src/config/defaults.ts');
        const { ProtocolCoordinator } = await import('/src/host/protocol-coordinator.ts');
        const { prepareDocumentWasmRestoreCapability } = await import('/src/host/wasm-restore-capability-source.ts');
        const { getDocumentWasmMemoryOwner } = await import('/src/host/wasm-document-memory-owner.ts');
        const setup = { ...DEFAULT_SETUP, stocks: [DEFAULT_SETUP.stocks[0]], npcs: { ...DEFAULT_SETUP.npcs, retail_count: 0, inst_count: 0, hot_count: 0 }, float_allocation: { between_kinds: 'Random', within_kind: 'Random' }, ticks_per_day: 4, auction_ticks: 1, closing_auction_ticks: 0, start_date: '2030-01-02', runtime_sync_policy: policy };
        const host = await createWorkerHost(setup, 42n, { enableE2EStepping: true, onSpawnFailure: (failure) => { throw new Error(JSON.stringify(failure)); } });
        const check = (condition, message) => { if (!condition) throw new Error(`${policy}: ${message}`); };
        try {
          let fatal;
          const coordinator = new ProtocolCoordinator({ onBaseline() {}, onApplied() {}, onFailure(failure) { fatal = failure; } }, () => {});
          await host.start((update) => coordinator.accept(update), (failure) => { fatal = failure; });
          await host.stop();
          const current = () => { const status = coordinator.status(); if (fatal || status.kind !== 'ready') throw new Error(JSON.stringify(fatal || status)); return status.state.snapshot; };
          const waitTick = async (tick) => { const deadline = performance.now() + 3000; while (current().tick < tick && performance.now() < deadline) await new Promise((resolve) => setTimeout(resolve, 1)); check(current().tick === tick, `等待 tick ${tick}，实际 ${current().tick}`); };
          const initial = current();
          const initialOrigin = initial.mirror_origin.source_tick;
          let rejected = false;
          if (policy === 'day_end') {
            try { await host.submitIntent({ PlaceLimit: { code: '600101', side: 'Buy', price: { Fixed: '1120' }, qty: 100 } }); } catch (error) { rejected = String(error).includes('runtime sync policy violation'); if (!rejected) throw error; }
            check(rejected, 'DayEnd 必须拒绝玩家输入');
          }
          await host.stepOnceForE2E();
          await waitTick(1);
          const first = current();
          check(first.tick === 1, `首拍 ${first.tick}`);
          check(first.mirror_origin.source_tick === (policy === 'every_tick' ? 1 : initialOrigin), `首拍 mirror ${JSON.stringify(first.mirror_origin)}`);
          if (policy === 'on_demand') {
            await host.flushRuntimeSync();
            const flushed = current();
            check(flushed.tick === first.tick && flushed.seq === first.seq && flushed.mirror_origin.source_tick === first.tick, 'flush 必须同步来源且不推进 tick/seq');
            await host.submitIntent({ PlaceLimit: { code: '600101', side: 'Buy', price: { Fixed: '1120' }, qty: 100 } });
            await host.stepOnceForE2E();
            await waitTick(2);
            const next = current();
            check(next.tick === 2 && next.mirror_origin.source_tick === 1, `flush 后下一增量 ${JSON.stringify(next.mirror_origin)}`);
            await host.flushRuntimeSync();
            check((await host.playerWorkingOrders()).length === 1, '刷新后真实委托必须进入账簿');
          }
          while (current().tick < 4) { const tick = await host.stepOnceForE2E(); await waitTick(tick); }
          const day = current();
          check(day.mirror_origin.source_tick === 4, `日终 mirror ${JSON.stringify(day.mirror_origin)}`);
          await host.endCivilDay();
          if (policy === 'on_demand') await host.flushRuntimeSync();
          const saved = await host.save();
          const selected = await host.save({ seq: saved.snapshot.seq, settledDate: saved.civil_clock.settled_through });
          check(selected.snapshot.seq === saved.snapshot.seq && selected.setup.runtime_sync_policy === policy, '日终候选及策略必须一致');
          await host.load(saved);
          check(host.snapshot().tick === saved.snapshot.tick, '恢复 tick 必须一致');
          await host.stepOnceForE2E();
          await waitTick(saved.snapshot.tick + 1);
          check(current().tick === saved.snapshot.tick + 1, '恢复后必须继续运行');
          const restoredTick = current().tick;
          await host.dispose();
          await prepareDocumentWasmRestoreCapability(document, { baseUrl: '/wasm-pkg/', runtimeOverride: true });
          const owner = getDocumentWasmMemoryOwner(document);
          const admission = owner.admit({ kind: 'load', archive: saved });
          check(admission.kind === 'admitted', '初始存档必须获真实 memory admission');
          const initialHost = await createWorkerHost(saved.setup, 42n, { enableE2EStepping: true, onSpawnFailure: (failure) => { throw new Error(JSON.stringify(failure)); }, initialArchive: admission.intent.archive, memoryAdmissionTicket: admission.ticket });
          try {
            check(initialHost.snapshot().tick === saved.snapshot.tick, '初始存档启动 tick 必须一致');
            const resumedTick = await initialHost.stepOnceForE2E();
            check(resumedTick === saved.snapshot.tick + 1, '初始存档启动后必须继续推进');
          } finally { await initialHost.dispose(); await owner.finish(admission.ticket); }
          return { policy, initialOrigin, firstOrigin: first.mirror_origin.source_tick, dayEndOrigin: day.mirror_origin.source_tick, candidateSeq: saved.snapshot.seq, restoredTick, initialRestoreTick: saved.snapshot.tick, rejected };
        } finally { await host.dispose(); }
      }, policy);
    } finally { await page.close(); }
  }));
  const failures = outcomes.filter((outcome) => outcome.status === 'rejected');
  if (failures.length) throw new AggregateError(failures.map((failure) => failure.reason), '浏览器验收失败');
  const results = outcomes.map((outcome) => outcome.value);
  assert.equal(results.length, 3);
  console.log(JSON.stringify(results, null, 2));
} finally {
  if (browser) await browser.close();
  await server.close();
}
