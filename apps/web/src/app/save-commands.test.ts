import assert from "node:assert/strict";
import test from "node:test";
import { createSaveCommands, type SaveCommandPorts } from "./useSaveCommands.ts";
import { InitialSaveSource, SessionReplacementGate } from "../save/session-replacement.ts";
import { DayEndPersistence } from "../save/day-end-persistence.ts";
import { PlayerOrderRefreshGate } from "../components/player-orders.ts";
import { SpeedMetricsRequestGate } from "../host/speed.ts";
import { DEFAULT_SETUP } from "../config/defaults.ts";
import { commandHostFixture, commandDayEndArchiveFixture } from "./command-host-test-fixture.ts";
import type { StrictSaveEnvelope } from "../save/schema/root.ts";
import type { SessionSetup } from "../types/engine.ts";

function deferred<Value>() { let resolve!: (value: Value) => void; const promise = new Promise<Value>((done) => { resolve = done; }); return { promise, resolve }; }
function fixture() {
  const calls: string[] = [], notices: string[] = [];
  const archive = commandDayEndArchiveFixture();
  const host = commandHostFixture({ load: async () => { calls.push("load"); }, refreshBaseline: async () => { calls.push("baseline"); },
    day: () => 3 });
  let setup: SessionSetup | null = null;
  const ports: SaveCommandPorts = {
    hostRef: { current: host }, initialSaveSourceRef: { current: new InitialSaveSource<StrictSaveEnvelope>() }, dayEndPersistenceRef: { current: new DayEndPersistence() },
    autoOrderMgrRef: { current: null }, sessionReplacementGateRef: { current: new SessionReplacementGate() }, saveSelectionGenerationRef: { current: 0 },
    dayEndFileTargetRef: { current: null }, playerOrderRefreshGateRef: { current: new PlayerOrderRefreshGate() },
    speedMetricsLoadInProgressRef: { current: false }, speedMetricsRequestGateRef: { current: new SpeedMetricsRequestGate() },
    fatalHostErrorRef: { current: (value) => { calls.push(`fatal:${String(value)}`); } },
    activeSetup: DEFAULT_SETUP, startDateDraft: "2031-02-03", priceCageEnabledDraft: false, floatAllocationDraft: DEFAULT_SETUP.float_allocation,
    loadFromFile: async () => archive, selectDayEndFileTarget: async () => ({ write: async () => { calls.push("file-write"); } }),
    getBrowserSaveRepository: () => ({ load: async () => archive }), resetMarketHistory: () => { calls.push("history"); },
    refreshPlayerOrders: async () => { calls.push("orders-refresh"); }, clearPlayerOrders: () => { calls.push("orders-clear"); },
    setNotice: (value) => { notices.push(value); }, setError: (value) => { calls.push(`error:${String(value)}`); }, setReady: (value) => { calls.push(`ready:${value}`); },
    setSessionSetup: (value) => { assert.equal(typeof value, "object"); setup = typeof value === "function" ? value(DEFAULT_SETUP) : value; calls.push("session-setup"); },
    setActiveSetup: () => { calls.push("active-setup"); }, setStartDateDraft: () => { calls.push("date-draft"); }, setPriceCageEnabledDraft: () => { calls.push("cage-draft"); }, setFloatAllocationDraft: () => { calls.push("allocation-draft"); },
    setStartDateError: (value) => { calls.push(`date-error:${String(value)}`); }, setSpeedMetricsPollingGeneration: (value) => { calls.push(`poll:${value}`); },
    setSpeedMetrics: (value) => { calls.push(`metrics:${value}`); }, setSpeedMetricsError: (value) => { calls.push(`metrics-error:${value}`); },
  };
  return { calls, notices, archive, host, ports, commands: createSaveCommands(ports), setup: () => setup };
}

test("保存仅提示日终政策；文件入口仅授权目标，不生成或写入日内存档", async () => {
  const f = fixture(); await f.commands.noticeSavePolicy(); await f.commands.selectFile();
  assert.match(f.notices[0], /日内不写档/); assert.match(f.notices[1], /已授权.*日终/);
  assert.deepEqual(f.calls, []); assert.ok(f.ports.dayEndFileTargetRef.current); assert.equal(f.ports.saveSelectionGenerationRef.current, 1);
});
test("旧文件选择响应不能替换共享目标；取消明确提示且不清除既有目标", async () => {
  const f = fixture(), selection = deferred<Awaited<ReturnType<SaveCommandPorts["selectDayEndFileTarget"]>>>();
  const original = { write: async () => {} }; f.ports.dayEndFileTargetRef.current = original;
  f.ports.selectDayEndFileTarget = () => selection.promise;
  const selecting = createSaveCommands(f.ports).selectFile(); f.ports.saveSelectionGenerationRef.current++; selection.resolve({ write: async () => {} }); await selecting;
  assert.equal(f.ports.dayEndFileTargetRef.current, original); assert.deepEqual(f.notices, []);
  f.ports.selectDayEndFileTarget = async () => null; await createSaveCommands(f.ports).selectFile();
  assert.equal(f.ports.dayEndFileTargetRef.current, original); assert.match(f.notices.at(-1)!, /已取消选择/);
});
test("快速槽与文件 load 共用宿主/metrics 令牌；成功只更新当前 Shell setup 和历史", async () => {
  for (const kind of ["load", "loadFile"] as const) {
    const f = fixture(); const currentMetrics = f.ports.speedMetricsRequestGateRef.current.capture();
    const refresh = f.ports.playerOrderRefreshGateRef.current.next();
    f.host.load = async (slot) => {
      assert.equal(slot, f.archive); assert.equal(f.ports.speedMetricsLoadInProgressRef.current, true);
      assert.equal(f.ports.speedMetricsRequestGateRef.current.isCurrent(currentMetrics), false);
      assert.equal(f.ports.playerOrderRefreshGateRef.current.isCurrent(refresh), false); f.calls.push("load");
    };
    await f.commands[kind]();
    assert.deepEqual(f.calls, ["poll:1", "metrics:null", "metrics-error:null", "load", "poll:2", "history", "orders-clear", "active-setup", "date-draft", "cage-draft", "allocation-draft", "orders-refresh"]);
    assert.equal(f.setup(), null); assert.equal(f.ports.speedMetricsLoadInProgressRef.current, false);
    assert.match(f.notices.at(-1)!, /第 4 个交易日/); assert.ok(f.ports.sessionReplacementGateRef.current.begin() !== null);
  }
});
test("load 失败先同步权威基线；同步失败升级 fatal；旧宿主响应不能回写 setup", async () => {
  const f = fixture(); f.host.load = async () => { throw new Error("恢复失败"); }; await f.commands.load();
  assert.equal(f.calls.includes("baseline"), true); assert.match(f.notices.at(-1)!, /读档失败.*恢复失败/);
  f.host.refreshBaseline = async () => { throw new Error("同步失败"); }; await f.commands.loadFile();
  assert.ok(f.calls.some((value) => /fatal:文件读档后权威基线同步失败.*同步失败/.test(value)));
  const g = fixture(), pending = deferred<StrictSaveEnvelope | null>();
  g.ports.getBrowserSaveRepository = () => ({ load: () => pending.promise });
  const loading = createSaveCommands(g.ports).load(); g.ports.hostRef.current = null; pending.resolve(g.archive); await loading;
  assert.deepEqual(g.calls, []); assert.deepEqual(g.notices, []);
});
test("恢复入口等待旧日终写入屏障，选择首次读档源并请求重建；不调用当前 host.load", async () => {
  const f = fixture(), committed = deferred<void>(), entered = deferred<void>();
  const queue = f.ports.dayEndPersistenceRef.current; queue.install("1");
  const writing = queue.completed("1", Promise.resolve(f.archive), async () => { entered.resolve(); await committed.promise; }); await entered.promise;
  const recovering = f.commands.recoverFromFile(); await Promise.resolve(); await Promise.resolve();
  assert.equal(f.setup(), null); assert.deepEqual(f.calls, []);
  committed.resolve(); await recovering; await writing;
  assert.deepEqual(f.calls, ["orders-clear", "error:null", "ready:false", "session-setup"]);
  assert.deepEqual(f.setup(), f.archive.setup); assert.equal(await f.ports.initialSaveSourceRef.current.read(async () => null), f.archive);
});
test("新局日期错误不进入替换；合法新局只请求重建并保留价格笼子草稿", async () => {
  const f = fixture(); f.ports.startDateDraft = "不存在日期"; await createSaveCommands(f.ports).newGame();
  assert.match(f.calls[0], /date-error:/); assert.equal(f.setup(), null); assert.equal(f.ports.saveSelectionGenerationRef.current, 0);
  f.calls.length = 0; f.ports.startDateDraft = "2031-02-03"; await createSaveCommands(f.ports).newGame();
  assert.deepEqual(f.calls, ["date-error:null", "orders-clear", "error:null", "ready:false", "session-setup"]);
  assert.equal(f.setup()?.start_date, "2031-02-03"); assert.equal(f.setup()?.config.price_cage_enabled, false);
  assert.deepEqual(f.setup()?.float_allocation, f.ports.floatAllocationDraft);
  let reads = 0; assert.equal(await f.ports.initialSaveSourceRef.current.read(async () => { reads++; return f.archive; }), null); assert.equal(reads, 0);
});

test("快速槽与文件 load 失败释放替换屏障和 metrics 状态，同一入口可以重新读档", async () => {
  for (const kind of ["load", "loadFile"] as const) {
    const f = fixture();
    const previousMetrics = f.ports.speedMetricsRequestGateRef.current.capture();
    f.host.load = async () => { f.calls.push("load-failed"); throw new Error("日终存档恢复失败"); };

    await f.commands[kind]();

    assert.deepEqual(f.calls, ["poll:1", "metrics:null", "metrics-error:null", "load-failed", "poll:2", "baseline"]);
    assert.equal(f.ports.speedMetricsLoadInProgressRef.current, false);
    assert.equal(f.ports.speedMetricsRequestGateRef.current.isCurrent(previousMetrics), false);
    assert.match(f.notices.at(-1)!, /读档失败.*日终存档恢复失败/);
    assert.equal(f.setup(), null);

    f.calls.length = 0;
    f.host.load = async (slot) => { assert.equal(slot, f.archive); f.calls.push("load-retried"); };
    await f.commands[kind]();
    assert.deepEqual(f.calls, ["poll:3", "metrics:null", "metrics-error:null", "load-retried", "poll:4", "history", "orders-clear", "active-setup", "date-draft", "cage-draft", "allocation-draft", "orders-refresh"]);
    assert.match(f.notices.at(-1)!, /第 4 个交易日/);
  }
});

test("读档失败后的 baseline 同步期间更换宿主，晚到的同步错误不影响新会话", async () => {
  for (const kind of ["load", "loadFile"] as const) {
    const f = fixture();
    const entered = deferred<void>(), baseline = deferred<void>();
    f.host.load = async () => { throw new Error("旧会话恢复失败"); };
    f.host.refreshBaseline = async () => {
      f.calls.push("baseline-pending");
      entered.resolve();
      await baseline.promise;
      throw new Error("旧会话同步失败");
    };

    const loading = f.commands[kind]();
    await entered.promise;
    const replacementHost = commandHostFixture();
    f.ports.hostRef.current = replacementHost;
    f.ports.sessionReplacementGateRef.current.invalidate();
    const replacementGeneration = f.ports.sessionReplacementGateRef.current.begin();
    assert.notEqual(replacementGeneration, null);
    baseline.resolve();
    await loading;

    assert.deepEqual(f.calls, ["poll:1", "metrics:null", "metrics-error:null", "poll:2", "baseline-pending"]);
    assert.deepEqual(f.notices, []);
    assert.equal(f.ports.hostRef.current, replacementHost);
    assert.equal(f.ports.sessionReplacementGateRef.current.isCurrent(replacementGeneration!), true);
    assert.equal(f.ports.speedMetricsLoadInProgressRef.current, false);
  }
});
