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
function fixture(archive = commandDayEndArchiveFixture()) {
  const calls: string[] = [], notices: string[] = [];
  const host = commandHostFixture({ load: async () => { calls.push("load"); }, refreshBaseline: async () => { calls.push("baseline"); },
    day: () => 3 });
  let setup: SessionSetup | null = null;
  const ports: SaveCommandPorts = {
    browserLocal: true,
    hostRef: { current: host }, initialSaveSourceRef: { current: new InitialSaveSource<StrictSaveEnvelope>() }, dayEndPersistenceRef: { current: new DayEndPersistence() },
    autoOrderMgrRef: { current: null }, sessionReplacementGateRef: { current: new SessionReplacementGate() }, saveSelectionGenerationRef: { current: 0 },
    dayEndFileTargetRef: { current: null }, playerOrderRefreshGateRef: { current: new PlayerOrderRefreshGate() },
    speedMetricsLoadInProgressRef: { current: false }, speedMetricsRequestGateRef: { current: new SpeedMetricsRequestGate() },
    fatalHostErrorRef: { current: (value) => { calls.push(`fatal:${String(value)}`); } },
    activeSetup: DEFAULT_SETUP, startDateDraft: "2031-02-03", priceCageEnabledDraft: false, floatAllocationDraft: DEFAULT_SETUP.float_allocation, reportFrequencyDraft: { Monthly: { schedule: { Preset: { preset: "FirstDayEvening", delay: "None" } } } }, dividendTaxModeDraft: DEFAULT_SETUP.dividend_tax_mode, flatWithholdingBpDraft: DEFAULT_SETUP.flat_withholding_bp ?? 1000, rightsOfferingEnabledDraft: DEFAULT_SETUP.rights_offering_enabled, issuerRepurchaseEnabledDraft: DEFAULT_SETUP.issuer_repurchase_enabled, setRightsOfferingEnabledDraft: () => {}, setIssuerRepurchaseEnabledDraft: () => {}, parValuePerShareDraft: "100", setParValuePerShareDraft: () => {},
    companySystemDraft: JSON.stringify({ mode: "Simple", config: { environment: { initial_change_bp: 0, persistence_bp: 0, noise: { monthly_bp: 0, quarterly_bp: 0, half_year_bp: 0, annual_bp: 0 } }, settlement_cycle: "Monthly", companies: [], prehistory_periods: 0 } }), seedDraft: "123456789",
    loadFromFile: async beforeRead => { await beforeRead?.(); return archive; }, selectDayEndFileTarget: async () => ({ write: async () => { calls.push("file-write"); } }),
    getBrowserSaveRepository: () => ({ load: async () => archive, select: async () => true, cancelPending: () => {}, newSlot: () => {} }), resetMarketHistory: () => { calls.push("history"); },
    configureMarketTiming: () => { calls.push("timing"); },
    refreshPlayerOrders: async () => { calls.push("orders-refresh"); }, clearPlayerOrders: () => { calls.push("orders-clear"); },
    setNotice: (value) => { notices.push(value); }, setError: (value) => { calls.push(`error:${String(value)}`); }, setReady: (value) => { calls.push(`ready:${value}`); },
    setSessionCreation: (value, seed) => { assert.equal(typeof value, "object"); assert.match(seed, /^(0|[1-9]\d*)$/); setup = value; calls.push("session-setup"); }, setSeedDraft: () => {},
    setActiveSetup: () => { calls.push("active-setup"); }, setStartDateDraft: () => { calls.push("date-draft"); }, setPriceCageEnabledDraft: () => { calls.push("cage-draft"); }, setFloatAllocationDraft: () => { calls.push("allocation-draft"); }, setReportFrequencyDraft: () => { calls.push("frequency-draft"); }, setCompanySystemDraft: () => { calls.push("company-draft"); }, setDividendTaxModeDraft: () => { calls.push("tax-mode-draft"); }, setFlatWithholdingBpDraft: () => { calls.push("flat-rate-draft"); }, setInitialAllocation: () => {},
    setStartDateError: (value) => { calls.push(`date-error:${String(value)}`); }, setSpeedMetricsPollingGeneration: (value) => { calls.push(`poll:${value}`); },
    setSpeedMetrics: (value) => { calls.push(`metrics:${value}`); }, setSpeedMetricsError: (value) => { calls.push(`metrics-error:${value}`); },
  };
  return { calls, notices, archive, host, ports, commands: createSaveCommands(ports), setup: () => setup };
}

test("快速槽读档等待点击前的日终写入，首次档案不会误报无存档", { timeout: 10000 }, async () => {
  for (const previous of [null, commandDayEndArchiveFixture()]) {
    const f = fixture(), commit = deferred<void>(), entered = deferred<void>();
    let saved = previous;
    let reads = 0;
    f.ports.getBrowserSaveRepository = () => ({ load: async () => { reads++; return saved; }, select: async () => true, cancelPending: () => {}, newSlot: () => {} });
    const queue = f.ports.dayEndPersistenceRef.current;
    queue.install("current");
    const writing = queue.completed("current", Promise.resolve(f.archive), async (_slot, current) => {
      entered.resolve();
      await commit.promise;
      assert.equal(current(), true, "读取快速槽前不能取消已经提交的当日日终写入");
      saved = f.archive;
    });
    await entered.promise;
    let loaded: StrictSaveEnvelope | null = null;
    f.host.load = async slot => { loaded = slot as StrictSaveEnvelope; };
    const loading = createSaveCommands(f.ports).load();
    await Promise.resolve();
    await Promise.resolve();
    assert.equal(reads, 0);
    assert.equal(loaded, null);
    assert.match(f.notices[0], /正在读取.*等待.*写入/);
    commit.resolve();
    await Promise.all([writing, loading]);
    assert.equal(reads, 1);
    assert.equal(loaded, f.archive);
    assert.match(f.notices.at(-1)!, /已读档/);
  }
});

test("读档等待的写入失败不会载入旧快速槽，错误明确展示且允许重试", { timeout: 10000 }, async () => {
  const f = fixture(), commit = deferred<void>(), entered = deferred<void>();
  let reads = 0;
  f.ports.getBrowserSaveRepository = () => ({ load: async () => { reads++; return f.archive; }, select: async () => true, cancelPending: () => {}, newSlot: () => {} });
  const queue = f.ports.dayEndPersistenceRef.current;
  queue.install("current");
  const writing = queue.completed("current", Promise.resolve(f.archive), async () => {
    entered.resolve();
    await commit.promise;
    throw new Error("磁盘写入失败");
  });
  const rejected = assert.rejects(writing, /磁盘写入失败/);
  await entered.promise;
  const commands = createSaveCommands(f.ports);
  const loading = commands.load();
  await Promise.resolve();
  commit.resolve();
  await Promise.all([rejected, loading]);
  assert.equal(reads, 0);
  assert.equal(f.calls.includes("load"), false);
  assert.match(f.notices.at(-1)!, /读档失败.*磁盘写入失败/);
  await commands.load();
  assert.equal(reads, 1, "显式重试可以读取上次有效档案");
  assert.match(f.notices.at(-1)!, /已读档/);
});

test("等待写档时拒绝重复读档，替换宿主后旧操作不读取或取消新局队列", { timeout: 10000 }, async () => {
  const f = fixture(), commit = deferred<void>(), entered = deferred<void>();
  let reads = 0;
  f.ports.getBrowserSaveRepository = () => ({ load: async () => { reads++; return f.archive; }, select: async () => true, cancelPending: () => {}, newSlot: () => {} });
  const queue = f.ports.dayEndPersistenceRef.current;
  queue.install("old");
  const writing = queue.completed("old", Promise.resolve(f.archive), async () => { entered.resolve(); await commit.promise; });
  await entered.promise;
  const commands = createSaveCommands(f.ports);
  const loading = commands.load();
  await commands.load();
  assert.match(f.notices.at(-1)!, /上一项读档.*尚未结束/);
  f.ports.hostRef.current = commandHostFixture();
  f.ports.sessionReplacementGateRef.current.invalidate();
  queue.install("new");
  const noticesBeforeReplacement = [...f.notices];
  commit.resolve();
  await Promise.all([writing, loading]);
  assert.equal(reads, 0);
  assert.deepEqual(f.calls, []);
  assert.deepEqual(f.notices, noticesBeforeReplacement);
  assert.equal(await queue.completed("new", Promise.resolve(f.archive), async () => {}), true);
});

test("远程新游戏明确重置共享市场，保持host不另建私局也不访问客户端档", { timeout: 10000 }, async () => {
  const fixtureState = fixture();
  fixtureState.ports.browserLocal = false;
  let resets = 0;
  fixtureState.ports.hostRef.current = { ...fixtureState.host, capabilities: { ...fixtureState.host.capabilities, persistence: "remote" },
    resetMarket: async (setup, seed) => { resets += 1; assert.equal(setup.start_date, "2031-02-03"); assert.ok(seed >= 0n); },
  };
  fixtureState.ports.getBrowserSaveRepository = () => { throw new Error("Remote不能访问客户端游戏档"); };
  await createSaveCommands(fixtureState.ports).newGame();
  assert.equal(resets, 1);
  assert.equal(fixtureState.calls.includes("session-setup"), false);
  assert.equal(fixtureState.calls.includes("ready:false"), false);
  assert.match(fixtureState.notices.at(-1)!, /全市场/);
});

test("启动失败尚无host时明确新局仍取消旧事务并分配新浏览器槽", { timeout: 10000 }, async () => {
  const f = fixture();
  f.ports.hostRef.current = null;
  const storageCalls: string[] = [];
  f.ports.getBrowserSaveRepository = () => ({
    load: async () => { throw new Error("明确新局不能重读旧档"); },
    select: async () => { throw new Error("明确新局不能选择旧槽"); },
    cancelPending: () => { storageCalls.push("cancel"); },
    newSlot: () => { storageCalls.push("new-slot"); },
  });
  await createSaveCommands(f.ports).newGame();
  assert.deepEqual(storageCalls, ["cancel", "new-slot"]);
  assert.ok(f.calls.includes("session-setup"));
});

test("Native明确选槽走host archiveStore，客户端不写IndexedDB", { timeout: 10000 }, async () => {
  const f = fixture();
  f.ports.browserLocal = false;
  const nativeHost = { ...f.host, capabilities: { ...f.host.capabilities, persistence: "native" as const } };
  f.ports.hostRef.current = nativeHost;
  f.ports.getBrowserSaveRepository = () => { throw new Error("Native不得读写IndexedDB"); };
  const calls: unknown[][] = [];
  nativeHost.archiveStore = {
    select: async (slotId) => { calls.push(["select", slotId]); return true; },
    load: async (slotId) => { calls.push(["read", slotId]); return f.archive; },
    list: async () => { throw new Error("读档不应加载列表"); },
    rename: async () => { throw new Error("读档不应重命名"); },
    delete: async () => { throw new Error("读档不应删除"); },
    copy: async () => { throw new Error("读档不应复制"); },
  };
  nativeHost.load = async (slot, slotId) => { assert.equal(slot, f.archive); calls.push(["restore", slotId]); };
  await createSaveCommands(f.ports).load("selected-slot");
  assert.deepEqual(calls, [["read", "selected-slot"], ["restore", "selected-slot"], ["select", "selected-slot"]]);
});

test("Engine成功restore后选槽写入失败应承认市场已加载，保留当前setup且不假整体回滚", { timeout: 10000 }, async () => {
  const f = fixture();
  f.ports.getBrowserSaveRepository = () => ({
    load: async () => f.archive,
    select: async () => { throw new Error("启动槽事务配额不足"); },
    cancelPending: () => {}, newSlot: () => {},
  });
  await createSaveCommands(f.ports).load("明确档案");
  assert.ok(f.calls.includes("load"));
  assert.ok(f.calls.includes("active-setup"));
  assert.equal(f.calls.includes("baseline"), false);
  assert.match(f.notices.at(-1)!, /市场已加载.*启动槽选择保存失败.*事务配额不足/);
});

test("旧读档选择元数据的晚响应不能写入新局通知，也不触发假回滚", { timeout: 10000 }, async () => {
  const f = fixture();
  const pending = deferred<boolean>();
  const entered = deferred<void>();
  let remainsCurrent!: () => boolean;
  f.ports.getBrowserSaveRepository = () => ({
    load: async () => f.archive,
    select: async (_slotId, isCurrent) => { remainsCurrent = isCurrent!; entered.resolve(); return pending.promise; },
    cancelPending: () => {}, newSlot: () => {},
  });
  const loading = createSaveCommands(f.ports).load("旧选择");
  await entered.promise;
  f.ports.sessionReplacementGateRef.current.invalidate();
  assert.equal(remainsCurrent(), false);
  pending.resolve(true);
  await loading;
  assert.equal(f.notices.length, 1);
  assert.match(f.notices[0], /^正在读取/);
  assert.equal(f.calls.includes("baseline"), false);
});

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
    assert.deepEqual(f.calls, ["poll:1", "metrics:null", "metrics-error:null", "load", "poll:2", "timing", "active-setup", "date-draft", "cage-draft", "allocation-draft", "frequency-draft", "company-draft", "tax-mode-draft", "flat-rate-draft", "history", "orders-clear", "orders-refresh"]);
    assert.equal(f.setup(), null); assert.equal(f.ports.speedMetricsLoadInProgressRef.current, false);
    assert.match(f.notices.at(-1)!, /第 4 个交易日/); assert.ok(f.ports.sessionReplacementGateRef.current.begin() !== null);
  }
});
test("日终读档成功清除旧局初始分配，不拿读档后的持仓重算开局", async () => {
  for (const kind of ["load", "loadFile"] as const) {
    const f = fixture();
    const clears: unknown[] = [];
    f.ports.setInitialAllocation = (value) => { clears.push(value); };
    f.host.initialAllocation = async () => { throw new Error("读档不查询开局分配"); };
    await createSaveCommands(f.ports)[kind]();
    assert.deepEqual(clears, [null]);
  }
});
test("load 失败先同步权威基线；同步失败升级 fatal；旧宿主响应不能回写 setup", async () => {
  const f = fixture(); f.host.load = async () => { throw new Error("恢复失败"); }; await f.commands.load();
  assert.equal(f.calls.includes("baseline"), true); assert.match(f.notices.at(-1)!, /读档失败.*恢复失败/);
  f.host.refreshBaseline = async () => { throw new Error("同步失败"); }; await f.commands.loadFile();
  assert.ok(f.calls.some((value) => /fatal:文件读档后权威基线同步失败.*同步失败/.test(value)));
  const g = fixture(), pending = deferred<StrictSaveEnvelope | null>(), entered = deferred<void>();
  g.ports.getBrowserSaveRepository = () => ({ load: () => { entered.resolve(); return pending.promise; }, select: async () => true, cancelPending: () => {}, newSlot: () => {} });
  const loading = createSaveCommands(g.ports).load();
  await entered.promise;
  g.ports.hostRef.current = null; pending.resolve(g.archive); await loading;
  assert.deepEqual(g.calls, []); assert.equal(g.notices.length, 1); assert.match(g.notices[0], /^正在读取/);
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

test("文件读档与错误恢复传入提交屏障，读取前不取消当前日终写入", { timeout: 10000 }, async () => {
  for (const kind of ["loadFile", "recoverFromFile"] as const) {
    const f = fixture(), commit = deferred<void>(), entered = deferred<void>();
    const queue = f.ports.dayEndPersistenceRef.current; queue.install("current");
    let reads = 0;
    const writing = queue.completed("current", Promise.resolve(f.archive), async (_slot, current) => {
      entered.resolve(); await commit.promise;
      assert.equal(current(), true);
    });
    await entered.promise;
    f.ports.loadFromFile = async beforeRead => {
      assert.equal(typeof beforeRead, "function");
      await beforeRead!(); reads++; return f.archive;
    };
    const loading = createSaveCommands(f.ports)[kind]();
    await Promise.resolve(); await Promise.resolve();
    assert.equal(reads, 0); assert.deepEqual(f.calls, []);
    commit.resolve(); await Promise.all([writing, loading]);
    assert.equal(reads, 1);
    assert.equal(f.calls.some(call => call === (kind === "loadFile" ? "load" : "session-setup")), true);
  }
});
test("新局日期错误不进入替换；合法新局只请求重建并保留价格笼子草稿", async () => {
  const f = fixture(); f.ports.startDateDraft = "不存在日期"; await createSaveCommands(f.ports).newGame();
  assert.match(f.calls[0], /date-error:/); assert.equal(f.setup(), null); assert.equal(f.ports.saveSelectionGenerationRef.current, 0);
  f.calls.length = 0; f.ports.startDateDraft = "2031-02-03"; await createSaveCommands(f.ports).newGame();
  assert.deepEqual(f.calls, ["date-error:null", "orders-clear", "error:null", "ready:false", "session-setup"]);
  assert.equal(f.setup()?.start_date, "2031-02-03"); assert.equal(f.setup()?.config.price_cage_enabled, false);
  assert.deepEqual(f.setup()?.float_allocation, f.ports.floatAllocationDraft);
  assert.deepEqual(f.setup()?.report_frequency, f.ports.reportFrequencyDraft);
  assert.equal(f.setup()?.dividend_tax_mode, DEFAULT_SETUP.dividend_tax_mode);
  assert.equal(f.setup()?.flat_withholding_bp, DEFAULT_SETUP.flat_withholding_bp, "默认新局为简税并携带默认比例");
  assert.equal(f.setup()?.config.stamp_tax_rate, DEFAULT_SETUP.config.stamp_tax_rate, "简税默认新局保持正式印花税基线");
  f.ports.dividendTaxModeDraft = "Exempt"; await createSaveCommands(f.ports).newGame();
  assert.equal(f.setup()?.dividend_tax_mode, "Exempt", "新局税务模式草稿须进入重建 setup");
  assert.equal(f.setup()?.config.stamp_tax_rate, 0, "不扣税模式新局连印花税也免（2026-10-08 决策）");
  assert.equal(f.setup()?.flat_withholding_bp, null, "非 Flat 模式不携带简税比例（三态契约）");
  let reads = 0; assert.equal(await f.ports.initialSaveSourceRef.current.read(async () => { reads++; return f.archive; }), null); assert.equal(reads, 0);
});

test("新局使旧日终写入失效，并等待写入退出后才请求重建", { timeout: 10000 }, async () => {
  const f = fixture(), commit = deferred<void>(), entered = deferred<void>();
  const queue = f.ports.dayEndPersistenceRef.current; queue.install("old");
  let written = false;
  const writing = queue.completed("old", Promise.resolve(f.archive), async (_slot, current) => {
    entered.resolve(); await commit.promise;
    if (!current()) return false;
    written = true;
  });
  await entered.promise;
  const replacing = f.commands.newGame();
  await Promise.resolve(); await Promise.resolve();
  assert.equal(f.setup(), null);
  assert.deepEqual(f.calls, ["date-error:null"]);
  commit.resolve();
  assert.equal(await writing, false); await replacing;
  assert.equal(written, false);
  assert.equal(f.setup()?.start_date, "2031-02-03");
  assert.equal(await f.ports.initialSaveSourceRef.current.read(async () => f.archive), null);
});

test("Monthly尚未明确排期的新局不能推进替换或静默使用默认日期", { timeout: 10000 }, async () => {
  const f = fixture();
  f.ports.reportFrequencyDraft = "Monthly";
  await createSaveCommands(f.ports).newGame();
  assert.equal(f.setup(), null);
  assert.equal(f.ports.saveSelectionGenerationRef.current, 0);
  assert.match(f.notices.join("\n"), /请明确选择月报排期/);
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
    assert.deepEqual(f.calls, ["poll:3", "metrics:null", "metrics-error:null", "load-retried", "poll:4", "timing", "active-setup", "date-draft", "cage-draft", "allocation-draft", "frequency-draft", "company-draft", "tax-mode-draft", "flat-rate-draft", "history", "orders-clear", "orders-refresh"]);
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
    const noticesBeforeReplacement = [...f.notices];
    const replacementHost = commandHostFixture();
    f.ports.hostRef.current = replacementHost;
    f.ports.sessionReplacementGateRef.current.invalidate();
    const replacementGeneration = f.ports.sessionReplacementGateRef.current.begin();
    assert.notEqual(replacementGeneration, null);
    baseline.resolve();
    await loading;

    assert.deepEqual(f.calls, ["poll:1", "metrics:null", "metrics-error:null", "poll:2", "baseline-pending"]);
    assert.deepEqual(f.notices, noticesBeforeReplacement);
    assert.equal(f.ports.hostRef.current, replacementHost);
    assert.equal(f.ports.sessionReplacementGateRef.current.isCurrent(replacementGeneration!), true);
    assert.equal(f.ports.speedMetricsLoadInProgressRef.current, false);
  }
});


test("读档提交后恢复运行失败仍安装新时间配置，提交前失败保持旧配置", { timeout: 10000 }, async () => {
  for (const kind of ["load", "loadFile"] as const) {
    const f = fixture({ ...commandDayEndArchiveFixture(), setup: { ...DEFAULT_SETUP, ticks_per_day: 30, auction_ticks: 9, closing_auction_ticks: 3 } });
    let installed: SessionSetup | null = null;
    f.ports.configureMarketTiming = setup => { installed = setup; f.calls.push("timing"); };
    f.host.load = async (_slot, _slotId, onRestored) => { onRestored?.(); throw new Error("提交后的恢复运行失败"); };
    await createSaveCommands(f.ports)[kind]();
    assert.equal(installed, f.archive.setup);
    assert.equal(f.calls.includes("active-setup"), true);
    assert.equal(f.calls.filter(call => call === "timing").length, 1);
    assert.equal(f.calls.includes("baseline"), true);
    assert.match(f.notices.at(-1)!, /读档失败.*提交后的恢复运行失败/);
  }
});
