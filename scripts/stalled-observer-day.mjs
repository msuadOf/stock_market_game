#!/usr/bin/env node
// 真实 Chromium + 正式共享内存 WASM；自动推进，不调用单步或手工日结。
import assert from "node:assert/strict";
import { verifySharedMemoryArtifact } from "./check-web-release-wasm.mjs";
import { makeSetup, SCENARIO } from "./fixtures/libos-product-scenario.mjs";
import { createServer } from "../apps/web/node_modules/vite/dist/node/index.js";
import { chromium } from "../apps/web/node_modules/@playwright/test/index.mjs";

const root = import.meta.dirname + "/..";
const argv = process.argv.slice(2);
assert.ok(argv.length === 0 || (argv.length === 2 && argv[0] === "--browser"), "用法：stalled-observer-day.mjs [--browser <Chromium>]");
await verifySharedMemoryArtifact(root + "/apps/web/wasm-pkg");
const vite = await createServer({
  root: root + "/apps/web", appType: "custom", mode: "e2e",
  server: { host: "127.0.0.1", port: 0, fs: { allow: [root] } },
});
let browser;
try {
  vite.middlewares.use((req, res, next) => {
    if (req.url !== "/stalled-observer-day") return next();
    res.setHeader("Content-Type", "text/html; charset=utf-8");
    res.end("<!doctype html><title>观察端停收跨天回归</title>");
  });
  await vite.listen();
  browser = await chromium.launch({ headless: true, executablePath: argv[1] });
  const page = await browser.newPage();
  page.on("pageerror", error => console.error("页面错误", error));
  await page.goto(new URL("stalled-observer-day", vite.resolvedUrls.local[0]).href);
  const defaults = await page.evaluate(async () => (await import("/src/config/defaults.ts")).DEFAULT_SETUP);
  const result = await page.evaluate(async ({ setup, seed, startDate, codes }) => {
    if (!crossOriginIsolated) throw new Error("测试需要真实隔离 Worker");
    const { createWorkerHost } = await import("/src/host/worker-host.ts");
    const { normalizeSerdeValue } = await import("/src/host/protocol/guards.ts");
    const post = MessagePort.prototype.postMessage;
    let withheld = 0, lastObserverTick = 0, baseline, publisherFailure;
    let hold = true;
    MessagePort.prototype.postMessage = function(message, ...args) {
      if (hold && message?.type === "ackPublisher") {
        withheld += 1;
        return;
      }
      return post.call(this, message, ...args);
    };
    let host, unlockStorage;
    try {
      host = await createWorkerHost(setup, BigInt(seed), { onSpawnFailure(failure) { publisherFailure = failure; } });
      const { SAVE_DATABASE, SAVE_OBJECT_STORE } = await import("/src/save/save-storage-keys.ts");
      if ((await indexedDB.databases()).some(database => database.name === SAVE_DATABASE)) {
        throw new Error("首次日终前归档副实例不应创建数据库");
      }
      // fixture 创建空库并占住真实事务；不阻塞 JS，也不让存档中的 put 提前提交。
      const slowDatabase = await new Promise((resolve, reject) => {
        const request = indexedDB.open(SAVE_DATABASE, 1);
        request.onupgradeneeded = () => { request.result.createObjectStore(SAVE_OBJECT_STORE); };
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
      });
      let slow = true;
      unlockStorage = () => { slow = false; };
      await new Promise((resolve, reject) => {
        const transaction = slowDatabase.transaction(SAVE_OBJECT_STORE, "readwrite");
        const store = transaction.objectStore(SAVE_OBJECT_STORE);
        transaction.oncomplete = () => slowDatabase.close();
        transaction.onabort = () => { slowDatabase.close(); reject(transaction.error); };
        const keep = () => {
          if (!slow) return;
          const request = store.get("__regression_lock__");
          request.onsuccess = () => { resolve(); keep(); };
        };
        keep();
      });
      let readFence, fenceComplete = false, fenceError;
      await host.setPausePreferences({pause_after_close: false, pause_before_open: false});
      // 有足够观察窗口验证前一日真实产生分时，而不是对空数据断言。
      await host.setSpeed(12);
      await host.start(update => {
        if (update.type === "baseline") {
          baseline = update;
          lastObserverTick = update.snapshot.tick;
          return;
        }
        const value = normalizeSerdeValue(update.update, "stalled-observer.publication");
        lastObserverTick = value.TickBatch?.frames.at(-1)?.tick ?? value.CivilUpdate?.tick ?? lastObserverTick;
      }, failure => { publisherFailure = failure; });
      const previousLiveBars = Object.fromEntries(codes.map(code => [code, []]));
      let newIntraday;
      const deadline = performance.now() + 26_000;
      while (performance.now() < deadline) {
        if (publisherFailure) throw new Error("发布故障：" + JSON.stringify(publisherFailure));
        for (const code of codes) {
          const current = await host.queryCurrentMinuteHistory({ code });
          if (current.date === startDate && current.bars.length >= previousLiveBars[code].length) {
            previousLiveBars[code] = current.bars.map(bar => bar.minute_of_day);
          }
          if (current.date > startDate && current.trading && current.phase === "Continuous") {
            newIntraday = { code, date: current.date, minutes: current.bars.map(bar => bar.minute_of_day), phase: current.phase };
            break;
          }
        }
        if (readFence === undefined && previousLiveBars[codes[0]].length !== 0) {
          const close = await host.queryMarketHistory({ code: codes[0], date_from: startDate, date_to: startDate, after: null, page_size: 1 });
          if (close.entries[0]?.availability === "Traded") {
            readFence = host.waitCheckpointIdle().then(() => { fenceComplete = true; }, error => { fenceError = error; });
          }
        }
        if (newIntraday) break;
        await new Promise(resolve => setTimeout(resolve, 35));
      }
      if (!newIntraday) throw new Error("UI ACK 停发时，权威未进入下一交易日连续竞价");
      if (readFence === undefined || fenceComplete || fenceError !== undefined) {
        throw new Error("市场跨日时，必须确实有独立的归档读围栏仍在等真实 IndexedDB 提交");
      }
      const observerBeforeRecovery = lastObserverTick;
      unlockStorage();
      await readFence;
      if (fenceError !== undefined || !fenceComplete) throw new Error("归档 read fence 未跟随真实提交", { cause: fenceError });
      await host.stop();
      // 第一次围栏只保证发出时已入队的日界，随后市场可能结算了周末。
      // 停止后再取最终围栏，才能与重开时的最新持久化序号做精确对照。
      await host.waitCheckpointIdle();
      const previous = [];
      for (const code of codes) {
        const history = await host.queryMarketHistory({ code, date_from: startDate, date_to: startDate, after: null, page_size: 1 });
        const closed = history.entries[0];
        if (!closed || closed.date !== startDate) throw new Error("已完成交易日缺失");
        previous.push({
          code, minutes: previousLiveBars[code], retainedBars: closed.bars.length,
          availability: closed.availability,
          trades: closed.daily_candle?.trade_stats?.trade_count ?? 0,
          volume: closed.daily_candle?.volume ?? 0,
          day: closed.daily_candle?.time ?? null,
        });
      }
      // 不放回 ACK：等待独立存储提交，直接从产品使用的 IndexedDB 读取，不能以 publish 或同页内存冒充。
      const { CompressedIndexedDbSaveRepository } = await import("/src/save/indexed-db-save-repository.ts");
      const quick = new CompressedIndexedDbSaveRepository(indexedDB, { getItem: () => null });
      let archive = await quick.load();
      const storageDeadline = performance.now() + 6_000;
      while ((!archive || archive.civil_clock.settled_through < startDate) && performance.now() < storageDeadline) {
        if (publisherFailure) throw new Error("存档回归期间 authority 错误：" + JSON.stringify(publisherFailure));
        await new Promise(resolve => setTimeout(resolve, 35));
        archive = await quick.load();
      }
      if (!archive) throw new Error("UI 停收包时 IndexedDB 没有合法日终快速档");
      const dateTag = Date.parse(startDate) / 1_000;
      const stored = {
        date: archive.civil_clock.settled_through, tick: archive.snapshot.tick, seq: archive.snapshot.seq,
        tradedCandles: Object.values(archive.snapshot.daily_candles).flat().filter(candle => candle.time === dateTag && candle.volume > 0).length,
        retainedMinutes: archive.retained_market_days.flatMap(day => Object.values(day.securities)).reduce((count, item) => count + item.bars.length, 0),
        observerTickWhileStored: lastObserverTick,
      };
      hold = false;
      await host.refreshBaseline();
      return { withheld, observerBeforeRecovery, authorityAfterRecovery: baseline?.snapshot.tick, previous, newIntraday, stored };
    } finally {
      unlockStorage?.();
      try { await host?.dispose(); }
      finally { MessagePort.prototype.postMessage = post; }
    }
  }, {setup: makeSetup(defaults), seed: SCENARIO.seed, startDate: SCENARIO.startDate, codes: SCENARIO.codes});
  console.log(JSON.stringify(result));
  assert.equal(result.withheld, 2, "必须真正耗尽观察者发送窗口");
  assert.ok(result.observerBeforeRecovery < SCENARIO.ticksPerDay, "观察者不应随权威跨越日界");
  assert.ok(result.authorityAfterRecovery > SCENARIO.ticksPerDay, "恢复基线必须包含下一交易日新提交的 tick");
  assert.ok(result.previous.some(item => item.minutes.length > 0 && item.availability === "Traded" && item.trades > 0
    && item.volume > 0 && item.day === Date.parse(SCENARIO.startDate) / 1000), "前一日必须有真实成交与对应日期的日 K");
  const old = result.previous.find(item => item.code === result.newIntraday.code);
  // 在次日刚开盘时核对当日分钟坐标，不能以“条数较少”代替实际清空。
  assert.ok(old && old.minutes.some(minute => minute >= 13 * 60)
    && result.newIntraday.minutes.every(minute => minute < 12 * 60),
    "新一日早盘分钟桶不得残留上一日的下午分钟");
  assert.ok(result.previous.every(item => item.retainedBars === 0), "不得保留昨天的历史分钟K，仅保留日K");
  assert.ok(result.stored.date >= SCENARIO.startDate && result.stored.tick >= SCENARIO.ticksPerDay);
  assert.equal(result.stored.observerTickWhileStored, result.observerBeforeRecovery, "实际落盘前不得放回 ACK");
  assert.equal(result.stored.tradedCandles, 2, "两个真实成交日 K 必须进入压缩存档");
  assert.equal(result.stored.retainedMinutes, 0, "实际存档不得包含上一日分钟行情");
  await page.reload();
  const reopened = await page.evaluate(async () => {
    const { CompressedIndexedDbSaveRepository } = await import("/src/save/indexed-db-save-repository.ts");
    const archive = await new CompressedIndexedDbSaveRepository(indexedDB, localStorage).load();
    return archive && { date: archive.civil_clock.settled_through, seq: archive.snapshot.seq };
  });
  assert.deepEqual(reopened, { date: result.stored.date, seq: result.stored.seq }, "刷新后从老格式快速槽读取相同世代的持久化正文");
} finally {
  await browser?.close();
  await vite.close();
}
