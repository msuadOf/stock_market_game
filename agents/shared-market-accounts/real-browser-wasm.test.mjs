import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { before, after, test } from "node:test";
import { createHash } from "node:crypto";
import { verifyWebReleaseWasm } from "../../scripts/check-web-release-wasm.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const web = path.join(root, "apps/web");
const require = createRequire(path.join(web, "package.json"));
const { createServer } = await import(require.resolve("vite"));
const { chromium } = require("@playwright/test");
const fixturePath = path.join(web, "src/save/fixtures/current-schema-save.json");
const wasmPath = path.join(web, "wasm-pkg/web_wasm_bg.wasm");
let server;
let browser;
let address;
let fixtureHash;
let wasmHash;

before(async () => {
  fixtureHash = createHash("sha256").update(await fs.readFile(fixturePath)).digest("hex");
  wasmHash = createHash("sha256").update(await fs.readFile(wasmPath)).digest("hex");
  await verifyWebReleaseWasm(path.dirname(wasmPath));
  server = await createServer({ root: web, configFile: false, appType: "custom", server: { host: "127.0.0.1", port: 0, fs: { allow: [root] } }, optimizeDeps: { noDiscovery: true }, plugins: [{ name: "真实隔离浏览器验证", configureServer(instance) { instance.middlewares.use((request, response, next) => {
    response.setHeader("Cross-Origin-Opener-Policy", "same-origin");
    response.setHeader("Cross-Origin-Embedder-Policy", "require-corp");
    if (request.url === "/validation") { response.setHeader("Content-Type", "text/html"); response.end("<!doctype html><html><body>真实 WASM 验证</body></html>"); return; }
    next();
  }); } }] });
  await server.listen();
  address = server.resolvedUrls.local[0];
  browser = await chromium.launch({ headless: true });
});

after(async () => {
  const cleanup = await Promise.allSettled([
    browser?.close(), server?.close(),
    fs.readFile(fixturePath).then((bytes) => assert.equal(createHash("sha256").update(bytes).digest("hex"), fixtureHash, "验证中真实 fixture 不得漂移")),
    fs.readFile(wasmPath).then((bytes) => assert.equal(createHash("sha256").update(bytes).digest("hex"), wasmHash, "验证中真实 normal WASM 不得漂移")),
  ]);
  const failures = cleanup.filter((result) => result.status === "rejected").map((result) => result.reason);
  if (failures.length > 0) throw new AggregateError(failures, "真实浏览器验证清理或制品稳定性检查失败");
});

test("真实 Worker 与 IndexedDB 并行短验证", { timeout: 10000, concurrency: 2 }, async (suite) => {
  await Promise.all(["隔离分片一", "隔离分片二"].map((mode) => suite.test(`真实 normal WASM Worker／IndexedDB ${mode}`, { timeout: 10000 }, async () => {
    const context = await browser.newContext();
    try {
      const page = await context.newPage();
      await page.goto(`${address}validation`);
      const evidence = await page.evaluate(async ({ mode, fixtureUrl }) => {
        if (!crossOriginIsolated || typeof SharedArrayBuffer !== "function") throw new Error("真实浏览器没有多线程隔离条件");
        const { parseSaveSlot } = await import("/src/save/save-schema.ts");
        const { validateDayEndArchive } = await import("/src/save/day-end-candidate.ts");
        const { IndexedDbSaveRepository } = await import("/src/save/indexeddb-save-repository.ts");
        const raw = await (await fetch(fixtureUrl)).text();
        const slot = validateDayEndArchive(parseSaveSlot(JSON.parse(raw)));
        const origin = location.origin;
        const source = `import init, * as wasm from ${JSON.stringify(`${origin}/wasm-pkg/web_wasm.js`)};
          import { normalizeSerdeMaps } from ${JSON.stringify(`${origin}/src/host/serde-normalize.ts`)};
          await init(); await wasm.initThreadPool(2); if(wasm.host_capabilities().npcDecisionDiagnostics!==false) throw new Error("真实 normal WASM 不能声明 NPC 私有诊断");
          onmessage = ({data}) => { let handle; try { handle=wasm.restore_json(data); wasm.prepare_public_baseline(handle); postMessage({snapshot:normalizeSerdeMaps(wasm.snapshot(handle)),saved:normalizeSerdeMaps(wasm.save(handle))}); }
          catch(error) { postMessage({error:error instanceof Error?error.message:String(error)}); } finally { if(handle!==undefined)wasm.drop_session(handle); } }; postMessage({ready:true});`;
        const workerUrl = URL.createObjectURL(new Blob([source], { type: "text/javascript" }));
        const worker = new Worker(workerUrl, { type: "module" });
        const ready = new Promise((resolve, reject) => {
          worker.onmessage = ({ data }) => data.ready === true ? resolve() : reject(new Error("真实 Worker 初始化消息无效"));
          worker.onerror = (error) => reject(new Error(error.message));
        });
        const restore = (json) => new Promise((resolve, reject) => {
          worker.onmessage = ({ data }) => data.error === undefined ? resolve(data) : reject(new Error(data.error));
          worker.onerror = (error) => reject(new Error(error.message));
          worker.postMessage(json);
        });
        try {
          await ready;
          const restored = await restore(raw);
          const actual = restored.snapshot;
          const candidate = validateDayEndArchive(parseSaveSlot(restored.saved));
          const repository = new IndexedDbSaveRepository(indexedDB, `真实验收-${mode}`);
          const committed = await repository.save(candidate);
          const metadata = await repository.list();
          const copied = await repository.copy(metadata[0].slot_id, "真实复制");
          const selected = await repository.select(copied.slot_id);
          const saved = await repository.load();
          if (saved === null) throw new Error("真实 IndexedDB 选槽后没有存档");
          const reloaded = await restore(JSON.stringify(saved));
          return { committed, selected, sameSnapshot: JSON.stringify(actual) === JSON.stringify(reloaded.snapshot), tick: actual.tick, expectedTick: slot.snapshot.tick, seq: actual.seq, expectedSeq: slot.snapshot.seq, accountIds: Object.keys(actual.accounts), expectedAccountIds: Object.values(slot.market_memberships.members).map((member) => member.account_id), settledDate: candidate.civil_clock.settled_through, slots: (await repository.list()).length };
        } finally { worker.terminate(); URL.revokeObjectURL(workerUrl); }
      }, { mode, fixtureUrl: `/@fs${fixturePath}` });
      assert.equal(evidence.committed, true);
      assert.equal(evidence.selected, true);
      assert.equal(evidence.tick, evidence.expectedTick);
      assert.equal(evidence.seq, evidence.expectedSeq);
      assert.deepEqual(evidence.accountIds.sort(), evidence.expectedAccountIds.sort());
      assert.equal(evidence.sameSnapshot, true);
      assert.equal(evidence.slots, 2);
      assert.ok(evidence.settledDate);
      console.log(JSON.stringify({ mode, fixtureHash, wasmHash, rayonThreads: 2, ...evidence }));
    } finally { await context.close(); }
  })));
});
