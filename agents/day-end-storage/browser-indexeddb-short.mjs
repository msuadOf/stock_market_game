import assert from "node:assert/strict";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "../../apps/web/node_modules/@playwright/test/index.mjs";
import { createServer } from "../../apps/web/node_modules/vite/dist/node/index.js";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const selectedCases = process.argv.slice(2);
const cases = selectedCases.length > 0 ? selectedCases : ["transactions", "structure", "ui", "selection", "connection"];
console.log(JSON.stringify({ classification: "真实 IndexedDB 代表性短测", workers: cases.length, caseTimeoutMs: 10000 }));
const server = await createServer({ root: path.join(root, "apps/web"), server: { host: "127.0.0.1", port: 0 } });
server.middlewares.use((request, response, next) => {
  if (request.url !== "/archive-test") return next();
  response.setHeader("Content-Type", "text/html");
  response.end("<!doctype html><title>IndexedDB 短测</title>");
});
let browser;
try {
  await server.listen();
  browser = await chromium.launch();
  await Promise.all(cases.map(async (testCase) => {
    const started = performance.now();
    const page = await browser.newPage();
    page.setDefaultTimeout(10000);
    await page.goto(`${server.resolvedUrls.local[0]}archive-test`);
    const result = await page.evaluate(async (kind) => {
      const { IndexedDbSaveRepository } = await import("/src/save/indexeddb-save-repository.ts");
      const databaseName = `archive-${kind}-${crypto.randomUUID()}`;
      if (kind === "connection") {
        const repository = new IndexedDbSaveRepository(indexedDB, databaseName);
        await repository.list();
        const database = await repository.opening;
        database.close();
        database.dispatchEvent(new Event("close"));
        await repository.list();
        await repository.close();
        return "数据库连接关闭后明确操作重新开库";
      }
      if (kind === "structure") {
        await new Promise((resolve, reject) => {
          const request = indexedDB.open(databaseName);
          request.onupgradeneeded = () => request.result.createObjectStore("legacy");
          request.onsuccess = () => { request.result.close(); resolve(); };
          request.onerror = () => reject(request.error);
        });
        const repository = new IndexedDbSaveRepository(indexedDB, databaseName);
        try { await repository.list(); throw new Error("旧结构未拒绝"); }
        catch (error) { if (!String(error).includes("数据库结构")) throw error; }
        return "旧结构显式拒绝";
      }
      const { currentSaveFixture } = await import("/src/save/current-save-fixture.ts");
      const fixture = currentSaveFixture();
      fixture.report_correction_operations = {};
      fixture.runtime_state.personal_trade_confirmations = {};
      fixture.civil_clock.settled_through = "2029-12-31";
      const repository = new IndexedDbSaveRepository(indexedDB, databaseName);
      const equal = (actual, expected) => { if (JSON.stringify(actual) !== JSON.stringify(expected)) throw new Error(`真实 IndexedDB 内容不一致：actual=${JSON.stringify(actual)} expected=${JSON.stringify(expected)}`); };
      equal(await repository.list(), []);
      equal(await repository.load(), null);
      equal(await repository.save(fixture), true);
      if (kind === "selection") {
        const original = (await repository.list())[0];
        const copied = await repository.copy(original.slot_id, "明确选择的档案");
        await repository.select(copied.slot_id);
        await repository.close();
        const reopened = new IndexedDbSaveRepository(indexedDB, databaseName);
        equal((await reopened.load()).snapshot.tick, fixture.snapshot.tick);
        const advanced = { ...fixture, snapshot: { ...fixture.snapshot, tick: fixture.snapshot.tick + fixture.setup.ticks_per_day } };
        await reopened.save(advanced);
        equal((await reopened.load(copied.slot_id)).snapshot.tick, advanced.snapshot.tick);
        equal((await reopened.load(original.slot_id)).snapshot.tick, fixture.snapshot.tick);
        const originalPut = IDBObjectStore.prototype.put;
        IDBObjectStore.prototype.put = function (...args) {
          if (this.name === "selection") throw new DOMException("测试选槽事务失败", "QuotaExceededError");
          return originalPut.apply(this, args);
        };
        try { await reopened.select(original.slot_id); throw new Error("选槽故障未显示"); }
        catch (error) { if (!String(error).includes("选槽事务失败")) throw error; }
        finally { IDBObjectStore.prototype.put = originalPut; }
        const pointerReader = new IndexedDbSaveRepository(indexedDB, databaseName);
        equal((await pointerReader.load()).snapshot.tick, advanced.snapshot.tick);
        await pointerReader.close();
        await reopened.save(advanced);
        equal((await reopened.load(original.slot_id)).snapshot.tick, advanced.snapshot.tick);
        let current = true;
        const retired = reopened.select(copied.slot_id, () => current);
        current = false;
        equal(await retired, false);
        await reopened.close();
        const finalReader = new IndexedDbSaveRepository(indexedDB, databaseName);
        const next = { ...advanced, snapshot: { ...advanced.snapshot, tick: advanced.snapshot.tick + fixture.setup.ticks_per_day } };
        await finalReader.load();
        await finalReader.save(next);
        equal((await finalReader.load(original.slot_id)).snapshot.tick, next.snapshot.tick);
        equal((await finalReader.load(copied.slot_id)).snapshot.tick, advanced.snapshot.tick);
        await finalReader.delete(original.slot_id);
        equal(await finalReader.load(), null);
        await finalReader.close();
        return "物理重开选槽、失败保留启动指针但保留内存目标、旧代选择拒绝";
      }
      if (kind === "ui") {
        const { ArchiveManager } = await import("/src/components/ArchiveManager.tsx");
        const { default: React } = await import("/node_modules/.vite/deps/react.js");
        const { default: ReactDOM } = await import("/node_modules/.vite/deps/react-dom_client.js");
        const element = document.createElement("main");
        document.body.append(element);
        window.archiveLoads = 0;
        ReactDOM.createRoot(element).render(React.createElement(ArchiveManager, { repository,
          onLoad: async (slotId) => { await repository.load(slotId); window.archiveLoads += 1; }, onClose: () => {} }));
        return "真实槽位 UI";
      }
      const first = (await repository.list())[0];
      equal(first.civil_date, fixture.civil_clock.settled_through);
      const original = await repository.load();
      equal(original.snapshot.tick, fixture.snapshot.tick);
      await repository.rename(first.slot_id, "测试档");
      const copied = await repository.copy(first.slot_id, "副本");
      equal((await repository.list()).length, 2);
      equal((await repository.load(copied.slot_id)).snapshot.tick, fixture.snapshot.tick);
      equal(await repository.save(fixture, () => false), false);
      const stale = { ...fixture, snapshot: { ...fixture.snapshot, tick: fixture.snapshot.tick + fixture.setup.ticks_per_day } };
      let current = true;
      const pending = repository.save(stale, () => current);
      current = false;
      equal(await pending, false);
      equal((await repository.load()).snapshot.tick, original.snapshot.tick);
      const { DayEndPersistence } = await import("/src/save/day-end-persistence.ts");
      const queue = new DayEndPersistence();
      queue.install("交易日");
      const tradingDay = { ...fixture, civil_clock: { ...fixture.civil_clock, current_date: "2030-01-12", settled_through: "2030-01-11" } };
      equal(await queue.completed("交易日", Promise.resolve(tradingDay), (slot, isCurrent) => repository.save(slot, isCurrent)), true);
      const closedDay = { ...tradingDay, civil_clock: { ...tradingDay.civil_clock, current_date: "2030-01-13", settled_through: "2030-01-12" } };
      equal(await queue.completed("交易日", Promise.resolve(closedDay), (slot, isCurrent) => repository.save(slot, isCurrent)), true);
      equal((await repository.load()).civil_clock.settled_through, "2030-01-12");
      let release;
      const delayed = new Promise((resolve) => { release = resolve; });
      const oldWrite = queue.completed("交易日", delayed, (slot, isCurrent) => repository.save(slot, isCurrent));
      queue.invalidate();
      repository.newSlot();
      release(stale);
      equal(await oldWrite, false);
      equal((await repository.load(first.slot_id)).civil_clock.settled_through, "2030-01-12");
      await repository.select(first.slot_id);
      const retired = repository.save(stale);
      repository.newSlot();
      equal(await retired, false);
      equal((await repository.load(first.slot_id)).snapshot.tick, original.snapshot.tick);
      await repository.select(first.slot_id);
      const beforeFailure = await repository.load();
      const beforeMetadata = await repository.list();
      const originalPut = IDBObjectStore.prototype.put;
      IDBObjectStore.prototype.put = function (...args) {
        if (this.name === "metadata") throw new DOMException("测试写入配额失败", "QuotaExceededError");
        return originalPut.apply(this, args);
      };
      try { await repository.save(stale); throw new Error("写入故障未显示"); }
      catch (error) { if (!String(error).includes("配额")) throw error; }
      finally { IDBObjectStore.prototype.put = originalPut; }
      equal(await repository.load(), beforeFailure);
      equal(await repository.list(), beforeMetadata);
      try { await repository.save({ ...fixture, schema_version: 1 }); throw new Error("旧档未拒绝"); }
      catch (error) { if (!String(error).includes("schema_version")) throw error; }
      equal(await repository.load(), beforeFailure);
      try { await repository.save({ ...fixture, civil_clock: { ...fixture.civil_clock, settled_through: null } }); throw new Error("首日日结前不应保存"); }
      catch (error) { if (!String(error).includes("日终结算")) throw error; }
      equal(await repository.load(), beforeFailure);
      await repository.delete(copied.slot_id);
      equal((await repository.list()).length, 1);
      await repository.close();
      const reopened = new IndexedDbSaveRepository(indexedDB, databaseName);
      equal((await reopened.load()).snapshot.tick, original.snapshot.tick);
      await reopened.delete(first.slot_id);
      equal(await reopened.load(), null);
      await reopened.close();
      return "事务、管理、旧代隔离、严格当前档、重开恢复";
    }, testCase);
    assert.ok(result);
    if (testCase === "ui") {
      await page.getByRole("button", { name: "读档", exact: true }).waitFor();
      assert.equal(await page.evaluate(() => window.archiveLoads), 0);
      await page.getByRole("textbox", { name: "存档新名称" }).fill("界面重命名");
      await page.getByRole("button", { name: "重命名", exact: true }).click();
      await page.getByText("界面重命名", { exact: false }).waitFor();
      assert.equal(await page.evaluate(() => window.archiveLoads), 0);
      await page.getByRole("textbox", { name: "存档新名称" }).fill("界面副本");
      await page.getByRole("button", { name: "复制", exact: true }).click();
      await page.getByText("界面副本", { exact: false }).waitFor();
      assert.equal(await page.evaluate(() => window.archiveLoads), 0);
      await page.getByRole("button", { name: "读档", exact: true }).first().click();
      await page.waitForFunction(() => window.archiveLoads === 1);
      page.once("dialog", (dialog) => dialog.accept());
      await page.getByRole("button", { name: "删除", exact: true }).first().click();
      await page.waitForFunction(() => document.querySelectorAll(".archive-manager li").length === 1);
    }
    console.log(`${testCase}: PASS ${result} wallMs=${Math.round(performance.now() - started)}`);
    await page.close();
  }));
} finally {
  if (browser) await browser.close();
  await server.close();
}
