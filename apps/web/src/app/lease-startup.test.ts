import assert from "node:assert/strict"
import { gzipSync } from "node:zlib"
import test from "node:test"
import { createSessionHostLifecycle, type SessionHostLifecyclePorts } from "./useSessionHostLifecycle.ts"
import { createSaveCommands, type SaveCommandPorts } from "./useSaveCommands.ts"
import { commandHostFixture } from "./command-host-test-fixture.ts"
import { DEFAULT_SETUP } from "../config/defaults.ts"
import { PlayerOrderRefreshGate } from "../components/player-orders.ts"
import { SpeedMetricsRequestGate } from "../host/speed.ts"
import { WasmDocumentMemoryOwner } from "../host/wasm-document-memory-owner.ts"
import { WasmMemoryStorage } from "../host/wasm-memory-storage.ts"
import { fixture as memoryFixture, MiB } from "../host/wasm-memory-storage.test-support.ts"
import { CHUNKED_RESTORE_EXPORTS } from "../host/wasm-restore-package-capability.ts"
import { InitialSaveSource, SessionReplacementGate } from "../save/session-replacement.ts"
import { DayEndPersistence } from "../save/day-end-persistence.ts"
import { currentSaveFixture, withSettledDay } from "../save/current-save-fixture.ts"
import { parseSaveJson } from "../save/save-schema.ts"
import { captureGzipArchiveSource } from "../save/archive-source.ts"
import { createValidatedArchiveLease, inspectValidatedArchiveLease, type ValidatedArchiveLease } from "../save/validated-archive-lease.ts"
import type { InitialSaveValue } from "../save/initial-save-selection.ts"
import type { ChunkedArchiveRestore } from "./chunked-archive-restore.ts"
import type { SessionSetup } from "../types/engine.ts"
import { createChunkedHandoverSources } from "../save/chunked-handover-source.ts"

function deferred<Value>() { let resolve!: (value: Value) => void; const promise = new Promise<Value>(done => { resolve = done }); return { promise, resolve } }
async function fixture() {
  const value = { ...withSettledDay(currentSaveFixture(), "2030-01-01", "2030-01-02"), seed: "18446744073709551615" }, text = JSON.stringify(value)
  const archive = parseSaveJson(text), lease = await createValidatedArchiveLease(captureGzipArchiveSource(`gzip:${gzipSync(text).toString("base64")}`))
  const calls: string[] = [], chosenSetups: SessionSetup[] = [], source = new InitialSaveSource<InitialSaveValue>(), memory = memoryFixture()
  const capability = { kind: "verified-package" as const, wasmSha256: "a".repeat(64), glueSha256: "b".repeat(64), sourceSha256: "c".repeat(64), buildManifestSha256: "d".repeat(64), exports: ["restore_json", "facade_slot_init_for_restore_json", "restore_prepared", "clear_prepared_restore", "wasm_tombstone_init_primary", "wasm_tombstone_bind_ingress", ...CHUNKED_RESTORE_EXPORTS] }
  const owner = new WasmDocumentMemoryOwner({ maximumBytes: memory.options.maximumBytes, reclaimBytes: memory.options.reclaimBytes,
    readCapacity: () => ({ kind: "enforced-capacity", initialPages: 1024, maximumPages: 4096, liveUpperBoundBytes: 256 * MiB }),
    restoreCapabilities: { forArchive: () => capability, forLease: () => capability }, createStorage: liveUpperBoundBytes => new WasmMemoryStorage({ ...memory.options, liveUpperBoundBytes }) })
  const host = commandHostFixture({ start: async () => { calls.push("start") }, stop: async () => { calls.push("stop") }, dispose: async () => { calls.push("dispose") },
    setSpeed: async () => {}, setPausePreferences: async () => {}, tick: () => 0, day: () => 1, load: async () => { calls.push("host.load") }, refreshBaseline: async () => {}, initialAllocation: async () => { calls.push("allocation"); return { stocks: [] } } })
  const chunkedRestore: ChunkedArchiveRestore = {
    prepareCapability: async selected => { assert.equal(selected, lease); calls.push("prepare-lease") },
    createHost: async input => {
      assert.equal(input.lease, lease); assert.equal(input.seed, 18446744073709551615n); assert.equal(input.setup, inspectValidatedArchiveLease(lease).setup)
      input.ticket.registerRelease(Promise.resolve({ kind: "not-allocated" })); calls.push("chunked-create"); return host
    },
  }
  const ports: SessionHostLifecyclePorts = {
    hostRef: { current: null }, hostTransitionRef: owner.hostTransitionRef, wasmMemoryOwner: owner,
    initialSaveSourceRef: { current: source }, dayEndPersistenceRef: { current: new DayEndPersistence() }, autoOrderMgrRef: { current: null },
    sessionReplacementGateRef: { current: new SessionReplacementGate() }, saveSelectionGenerationRef: { current: 0 }, playerOrderRefreshGateRef: { current: new PlayerOrderRefreshGate() },
    hostUpdateRef: { current: () => true }, fatalHostErrorRef: { current: () => {} }, startupTarget: { kind: "wasm" }, sessionSetup: DEFAULT_SETUP,
    speed: 1, pauseAfterClose: false, pauseBeforeOpen: false, TRADING_E2E_MODE: false, malformedProtocolFixture: () => false,
    setIndicatorCalculator: () => () => {}, connectProtocol: () => {}, disconnectProtocol: () => {},
    createHost: async (_setup, _seed, _target, _failure, _archive, ticket) => { ticket?.registerRelease(Promise.resolve({ kind: "not-allocated" })); calls.push("string-create"); return host },
    createSeed: () => { calls.push("entropy"); return 42n }, checkWasmEnvironment: () => {}, isDocumentHidden: () => false,
    getBrowserSaveRepository: () => ({ load: async () => { calls.push("read-ordinary"); return archive }, loadFrozen: async () => { calls.push("read-string"); return archive }, loadValidatedLease: async () => { calls.push("read-lease"); return lease } }),
    setActiveSetup: setup => { assert.ok(setup); calls.push("setup") }, configureMarketTiming: () => {}, setStartDateDraft: () => {}, setPriceCageEnabledDraft: () => {}, setFloatAllocationDraft: () => {}, setInitialAllocation: () => {},
    setDeliveryModes: () => {}, setDeliveryModeState: () => {}, setNotice: value => { calls.push(value) }, setReady: value => { calls.push(`ready:${value}`) },
    onSpawnFailure: () => {}, setError: error => { calls.push(typeof error === "string" ? error : error.message) }, onRunning: () => {}, onAutoTriggered: () => {},
  }
  return { ports, source, calls, lease, archive, host, chunkedRestore, chosenSetups, memory, capability }
}
function savePorts(f: Awaited<ReturnType<typeof fixture>>): SaveCommandPorts {
  f.ports.hostRef.current = f.host
  return { ...f.ports, startupTarget: f.ports.startupTarget, activeSetup: f.archive.setup, startDateDraft: "2030-01-03", priceCageEnabledDraft: true, floatAllocationDraft: f.archive.setup.float_allocation,
    dayEndFileTargetRef: { current: null }, speedMetricsLoadInProgressRef: { current: false }, speedMetricsRequestGateRef: { current: new SpeedMetricsRequestGate() },
    loadFromFile: async beforeRead => { await beforeRead?.(); f.calls.push("read-file"); return f.archive }, selectDayEndFileTarget: async () => null,
    resetMarketHistory: () => {}, refreshPlayerOrders: () => Promise.resolve(), clearPlayerOrders: () => {}, setError: () => {}, setSessionSetup: value => { if (typeof value === "function") throw new Error("此测试不预期函数式配置"); f.chosenSetups.push(value); f.calls.push("session-setup") }, setStartDateError: () => {},
    setSpeedMetricsPollingGeneration: () => {}, setSpeedMetrics: () => {}, setSpeedMetricsError: () => {},
  }
}

test("分块自动读档达到阈值时固定源并交接重载", { timeout: 10000 }, async () => {
  const f = await fixture(), owner = f.ports.wasmMemoryOwner!, data = new Map<string, string>();
  const sources = createChunkedHandoverSources({ async getItem(key) { return data.get(key) ?? null }, async addItem(key, value, current) { if (!current()) return false; data.set(key, value); return true }, async removeItem(key) { data.delete(key) } }, () => "cd".repeat(16));
  for (let i = 0; i < 2; i++) {
    const ticket = owner.admitChunked(f.lease).ticket;
    owner.recordCommittedBoundary(ticket, { tick: 1, day: 1 });
    ticket.registerRelease(Promise.resolve({ kind: "released", finalBytes: 256 * MiB, growthStopped: true, referencesDropped: true }));
    await owner.finish(ticket);
  }
  f.ports.chunkedRestore = { ...f.chunkedRestore, handoverSources: sources };
  f.ports.reloadAfterMemoryNotice = async () => { f.calls.push("reload") };
  await createSessionHostLifecycle(f.ports).start();
  assert.ok(f.calls.includes("reload"));
  assert.equal(f.calls.includes("chunked-create"), false);
  assert.equal(data.size, 1);
  const originalKey = [...data.keys()][0]!;
  data.set("quick-save", "其他标签的新档");
  const after = new WasmDocumentMemoryOwner({ maximumBytes: f.memory.options.maximumBytes, reclaimBytes: f.memory.options.reclaimBytes,
    readCapacity: () => ({ kind: "enforced-capacity", initialPages: 1024, maximumPages: 4096, liveUpperBoundBytes: 256 * MiB }),
    restoreCapabilities: { forArchive: () => f.capability, forLease: () => f.capability }, createStorage: liveUpperBoundBytes => new WasmMemoryStorage({ ...f.memory.options, liveUpperBoundBytes }) });
  let restored = false, attempts = 0;
  const nextPorts: SessionHostLifecyclePorts = { ...f.ports, hostTransitionRef: after.hostTransitionRef, wasmMemoryOwner: after,
    initialSaveSourceRef: { current: new InitialSaveSource<InitialSaveValue>() },
    confirmMemoryHandover: async handover => handover.kind === "frozen" ? handover.pendingIntent : null,
    chunkedRestore: { ...f.chunkedRestore, handoverSources: sources, prepareCapability: async () => {}, createHost: async input => {
      assert.equal(inspectValidatedArchiveLease(input.lease).seed, inspectValidatedArchiveLease(f.lease).seed);
      input.ticket.registerRelease(Promise.resolve({ kind: "not-allocated" }));
      if (attempts++ === 0) throw new Error("首次恢复失败");
      restored = true; return f.host;
    } },
    getBrowserSaveRepository: () => { throw new Error("重载确认不得重读快速槽") },
  };
  const first = createSessionHostLifecycle(nextPorts);
  await first.start();
  assert.equal(restored, false);
  assert.ok(nextPorts.initialSaveSourceRef.current.hasExplicitSelection(), "失败后须保留已确认租约");
  const next = createSessionHostLifecycle(nextPorts);
  await next.start();
  assert.equal(restored, true, f.calls.join(" | "));
  assert.ok(data.has(originalKey));
  await next.dispose();
});

test("正常分块读档不写入无用固定源", { timeout: 10000 }, async () => {
  const f = await fixture(); let writes = 0;
  const sources = createChunkedHandoverSources({ async getItem() { return null }, async addItem() { writes++; return true }, async removeItem() {} });
  f.ports.chunkedRestore = { ...f.chunkedRestore, handoverSources: sources };
  const lifecycle = createSessionHostLifecycle(f.ports);
  await lifecycle.start();
  assert.equal(writes, 0);
  assert.ok(f.calls.includes("chunked-create"));
  await lifecycle.dispose();
});

test("发布重载目标期间拒绝更换选择，原交接继续", { timeout: 10000 }, async () => {
  const f = await fixture(), owner = f.ports.wasmMemoryOwner!, data = new Map<string, string>();
  for (let i = 0; i < 2; i++) {
    const ticket = owner.admitChunked(f.lease).ticket;
    owner.recordCommittedBoundary(ticket, { tick: 1, day: 1 });
    ticket.registerRelease(Promise.resolve({ kind: "released", finalBytes: 256 * MiB, growthStopped: true, referencesDropped: true }));
    await owner.finish(ticket);
  }
  const sources = createChunkedHandoverSources({ async getItem(key) { return data.get(key) ?? null }, async addItem(key, value) { data.set(key, value); return true }, async removeItem(key) { data.delete(key) } }, () => "de".repeat(16), async (_id, _signal, action) => {
    const result = await action(); assert.throws(() => f.source.reset(), /重载交接/); return result;
  });
  f.ports.chunkedRestore = { ...f.chunkedRestore, handoverSources: sources };
  f.ports.reloadAfterMemoryNotice = async () => { f.calls.push("reload") };
  await createSessionHostLifecycle(f.ports).start();
  assert.equal(f.calls.includes("reload"), true);
  assert.equal(owner.pendingHandover()?.kind, "frozen");
  assert.equal(f.source.isSelectionFrozen(), true);
});

test("等待重载提示绘制时拒绝更换选择", { timeout: 10000 }, async () => {
  const f = await fixture(), owner = f.ports.wasmMemoryOwner!, data = new Map<string, string>();
  for (let i = 0; i < 2; i++) {
    const ticket = owner.admitChunked(f.lease).ticket; owner.recordCommittedBoundary(ticket, { tick: 1, day: 1 });
    ticket.registerRelease(Promise.resolve({ kind: "released", finalBytes: 256 * MiB, growthStopped: true, referencesDropped: true })); await owner.finish(ticket);
  }
  const sources = createChunkedHandoverSources({ async getItem(key) { return data.get(key) ?? null }, async addItem(key, value) { data.set(key, value); return true }, async removeItem(key) { data.delete(key) } }, () => "ef".repeat(16));
  f.ports.chunkedRestore = { ...f.chunkedRestore, handoverSources: sources };
  const entered = deferred<void>(), proceed = deferred<void>();
  f.ports.reloadAfterMemoryNotice = async (_degraded, isCurrent) => { entered.resolve(); await proceed.promise; if (isCurrent()) f.calls.push("reload"); };
  const lifecycle = createSessionHostLifecycle(f.ports), start = lifecycle.start();
  await entered.promise; assert.throws(() => f.source.reset(), /重载交接/); proceed.resolve(); await start;
  assert.equal(f.calls.includes("reload"), true); assert.equal(owner.pendingHandover()?.kind, "frozen");
});

test("冻结发布后 StrictMode 重挂继续原重载，不进入新目标准入", { timeout: 10000 }, async () => {
  const f = await fixture(), owner = f.ports.wasmMemoryOwner!, data = new Map<string, string>();
  for (let i = 0; i < 2; i++) {
    const ticket = owner.admitChunked(f.lease).ticket; owner.recordCommittedBoundary(ticket, { tick: 1, day: 1 });
    ticket.registerRelease(Promise.resolve({ kind: "released", finalBytes: 256 * MiB, growthStopped: true, referencesDropped: true })); await owner.finish(ticket);
  }
  const sources = createChunkedHandoverSources({ async getItem(key) { return data.get(key) ?? null }, async addItem(key, value) { data.set(key, value); return true }, async removeItem(key) { data.delete(key) } }, () => "fe".repeat(16));
  f.ports.chunkedRestore = { ...f.chunkedRestore, handoverSources: sources };
  const entered = deferred<void>(), proceed = deferred<void>(); let reloads = 0, calls = 0;
  f.ports.reloadAfterMemoryNotice = async (_degraded, isCurrent) => { if (calls++ === 0) { entered.resolve(); await proceed.promise; } if (isCurrent()) reloads++; };
  const first = createSessionHostLifecycle(f.ports), firstStart = first.start(); await entered.promise;
  await first.dispose(); const next = createSessionHostLifecycle(f.ports), nextStart = next.start();
  proceed.resolve(); await Promise.all([firstStart, nextStart]);
  assert.equal(reloads, 1); assert.equal(calls, 2); assert.equal(f.calls.includes("chunked-create"), false);
  assert.equal(f.source.isSelectionFrozen(), true); assert.equal(owner.pendingHandover()?.kind, "frozen");
});

test("默认不读租约且String启动保持；注入真实可选factory才选择lease并精确传seed/setup", { timeout: 10000 }, async () => {
  const ordinary = await fixture(), defaultRuntime = createSessionHostLifecycle(ordinary.ports); await defaultRuntime.start()
  assert.ok(ordinary.calls.includes("read-string")); assert.ok(ordinary.calls.includes("string-create")); assert.equal(ordinary.calls.includes("read-lease"), false)
  await defaultRuntime.dispose()
  const f = await fixture(); f.ports.chunkedRestore = f.chunkedRestore
  const runtime = createSessionHostLifecycle(f.ports); await runtime.start()
  assert.ok(f.calls.includes("read-lease")); assert.ok(f.calls.includes("chunked-create")); assert.equal(f.calls.includes("read-string"), false)
  assert.equal(f.calls.includes("string-create"), false); assert.equal(f.calls.includes("entropy"), false); assert.equal(f.calls.includes("host.load"), false); assert.equal(f.calls.includes("allocation"), false)
  assert.ok(f.calls.some(value => /已读档/.test(value)), JSON.stringify(f.calls)); await runtime.dispose()
})

test("StrictMode取消effect不取消选择读取，后继复用同一lease Promise和signal不重读", { timeout: 10000 }, async () => {
  const f = await fixture(), entered = deferred<void>(), read = deferred<ValidatedArchiveLease | null>()
  f.ports.chunkedRestore = f.chunkedRestore
  let reads = 0, signal: AbortSignal | undefined
  f.ports.getBrowserSaveRepository = () => ({ load: async () => { entered.resolve(); throw new Error("不得读ordinary") }, loadFrozen: async () => { entered.resolve(); throw new Error("不得读String") }, loadValidatedLease: input => { reads++; signal = input; entered.resolve(); return read.promise } })
  const first = createSessionHostLifecycle(f.ports), starting = first.start(); await entered.promise; await first.dispose()
  assert.ok(signal); assert.equal(signal.aborted, false)
  const second = createSessionHostLifecycle(f.ports), next = second.start()
  read.resolve(f.lease); await Promise.all([starting, next])
  assert.equal(reads, 1); assert.equal(f.calls.filter(value => value === "chunked-create").length, 1); assert.ok(f.calls.includes("ready:true"), JSON.stringify(f.calls)); await second.dispose()
})

test("显式lease无factory或跨远程目标时显式拒绝，不读槽、不回退String", { timeout: 10000 }, async () => {
  for (const remote of [false, true]) {
    const f = await fixture(); f.source.select({ kind: "lease", lease: f.lease })
    if (remote) { f.ports.startupTarget = { kind: "remote", baseUrl: "http://localhost" }; f.ports.chunkedRestore = f.chunkedRestore }
    await createSessionHostLifecycle(f.ports).start()
    assert.equal(f.calls.some(value => value.startsWith("read-")), false); assert.equal(f.calls.includes("string-create"), false); assert.equal(f.calls.includes("chunked-create"), false)
    assert.ok(f.calls.some(value => /租约|分块/.test(value)))
  }
})

test("明确新局不读取快速槽，远程即使配置chunked能力仍走ordinary/String", { timeout: 10000 }, async () => {
  const fresh = await fixture(); fresh.source.reset(); fresh.ports.chunkedRestore = fresh.chunkedRestore
  const runtime = createSessionHostLifecycle(fresh.ports); await runtime.start()
  assert.equal(fresh.calls.some(value => value.startsWith("read-")), false); assert.ok(fresh.calls.includes("string-create")); await runtime.dispose()
  const remote = await fixture(); remote.ports.startupTarget = { kind: "remote", baseUrl: "http://localhost" }; remote.ports.chunkedRestore = remote.chunkedRestore
  await createSessionHostLifecycle(remote.ports).start()
  assert.ok(remote.calls.includes("read-ordinary")); assert.ok(remote.calls.includes("host.load")); assert.equal(remote.calls.includes("read-lease"), false)
})

test("手动快速槽通过lease选择复用启动源；文件仍保存原archive选择；明确新局清除选择", { timeout: 10000 }, async () => {
  const f = await fixture(), ports = savePorts(f); ports.chunkedRestore = f.chunkedRestore
  const commands = createSaveCommands(ports); await commands.load()
  const selected = await f.source.read(async () => { throw new Error("明确快速槽选择不可重读") })
  assert.deepEqual(selected, { kind: "lease", lease: f.lease }); assert.deepEqual(f.chosenSetups.at(-1), f.archive.setup); assert.ok(f.calls.includes("session-setup")); assert.equal(f.calls.filter(value => value === "read-lease").length, 1)
  await commands.loadFile(); assert.equal(await f.source.read(async () => { throw new Error("明确文件不可重读") }), f.archive)
  await commands.newGame(); assert.equal(await f.source.read(async () => { throw new Error("明确新局不可读槽") }), null)
})

test("换选择取消pending lease读取，旧错误和配置不回写，新局不重读", { timeout: 10000 }, async () => {
  const f = await fixture(), entered = deferred<void>()
  f.ports.chunkedRestore = f.chunkedRestore
  let reads = 0
  f.ports.getBrowserSaveRepository = () => ({ load: async () => { throw new Error("不得回退") }, loadFrozen: async () => { throw new Error("不得回退") }, loadValidatedLease: signal => {
    reads++; entered.resolve(); assert.ok(signal)
    return new Promise((_resolve, reject) => { signal.addEventListener("abort", () => { reject(signal.reason) }, { once: true }) })
  } })
  const old = createSessionHostLifecycle(f.ports), starting = old.start(); await entered.promise
  f.source.reset(); await starting
  assert.equal(f.calls.includes("setup"), false); assert.equal(f.calls.includes("ready:true"), false)
  assert.equal(f.calls.some(value => /初始化失败/.test(value)), false)
  const next = createSessionHostLifecycle(f.ports); await next.start()
  assert.equal(reads, 1); assert.ok(f.calls.includes("string-create")); await next.dispose()
})

test("取消chunked构造仅取消尝试signal，后继仍等旧构造和真实release，不重新读取源", { timeout: 10000 }, async () => {
  const f = await fixture(), entered = deferred<void>(), created = deferred<typeof f.host>(), released = deferred<import("../host/wasm-document-memory-owner.ts").WasmFinalRelease>()
  f.ports.chunkedRestore = { ...f.chunkedRestore, createHost: async input => { input.ticket.registerRelease(released.promise); attempt = input.signal; entered.resolve(); return created.promise } }
  let attempt: AbortSignal | undefined, nextAttempt: AbortSignal | undefined, nextCreates = 0
  const first = createSessionHostLifecycle(f.ports), starting = first.start(); await entered.promise; await first.dispose()
  assert.ok(attempt); assert.equal(attempt.aborted, true); assert.equal(f.source.selectionSignal().aborted, false)
  const second = createSessionHostLifecycle({ ...f.ports, chunkedRestore: { ...f.chunkedRestore, createHost: async input => { nextCreates++; nextAttempt = input.signal; input.ticket.registerRelease(Promise.resolve({ kind: "not-allocated" })); return commandHostFixture({ ...f.host }) } } })
  const next = second.start(); await Promise.resolve(); assert.equal(nextCreates, 0)
  created.resolve(f.host); await new Promise<void>(resolve => setImmediate(resolve)); assert.equal(nextCreates, 0)
  released.resolve({ kind: "not-allocated" }); await Promise.all([starting, next])
  assert.equal(nextCreates, 1); assert.equal(f.calls.filter(value => value === "read-lease").length, 1)
  assert.ok(nextAttempt); assert.equal(nextAttempt.aborted, false, "完成选择读取不能取消已就绪host的尝试signal")
  await second.dispose(); assert.equal(nextAttempt.aborted, true)
})

test("换选择时仅忽略自身取消，reader清理故障仍显示且不覆盖新选择", { timeout: 10000 }, async () => {
  const f = await fixture(), entered = deferred<void>(), cleanup = new Error("旧reader取消失败")
  f.ports.chunkedRestore = f.chunkedRestore
  f.ports.getBrowserSaveRepository = () => ({ load: async () => { throw new Error("不重读") }, loadFrozen: async () => { throw new Error("不重读") }, loadValidatedLease: signal => {
    assert.ok(signal); entered.resolve()
    return new Promise((_resolve, reject) => { signal.addEventListener("abort", () => { reject(new AggregateError([signal.reason, cleanup], "存档读取清理失败")) }, { once: true }) })
  } })
  const runtime = createSessionHostLifecycle(f.ports), starting = runtime.start(); await entered.promise
  f.source.select(f.archive); await starting
  assert.ok(f.calls.some(value => value.includes("旧reader取消失败")), JSON.stringify(f.calls))
  assert.equal(await f.source.read(async () => { throw new Error("新选择不读槽") }), f.archive)
  assert.equal(f.calls.includes("chunked-create"), false)
})

test("旧manual租约清理未完成时并发新会话零create，清理失败锁共享transition且不调用过期fatal", { timeout: 10000 }, async () => {
  const f = await fixture(), entered = deferred<void>(), cleanupStarted = deferred<void>(), cleanup = new Error("manual reader取消失败")
  const ports = savePorts(f); ports.chunkedRestore = f.chunkedRestore
  let finishCleanup: () => void = () => { throw new Error("尚未进入清理") }, oldFatal = 0
  ports.fatalHostErrorRef = { current: () => { oldFatal++ } }
  ports.getBrowserSaveRepository = () => ({ load: async () => { throw new Error("不回退") }, loadFrozen: async () => { throw new Error("不回退") }, loadValidatedLease: signal => {
    assert.ok(signal); entered.resolve()
    return new Promise((_resolve, reject) => { signal.addEventListener("abort", () => {
      finishCleanup = () => { reject(new AggregateError([signal.reason, cleanup], "旧手动读取清理失败")) }; cleanupStarted.resolve()
    }, { once: true }) })
  } })
  const loading = createSaveCommands(ports).load(); await entered.promise
  f.ports.hostRef.current = null; f.ports.sessionReplacementGateRef.current.invalidate(); f.source.reset(); await cleanupStarted.promise
  const next = createSessionHostLifecycle(f.ports), starting = next.start()
  await new Promise<void>(resolve => setImmediate(resolve))
  const beforeCleanup = f.calls.filter(value => value === "string-create" || value === "chunked-create").length
  finishCleanup(); await Promise.all([loading, starting])
  assert.equal(beforeCleanup, 0, "新会话不得越过旧读取清理")
  assert.equal(f.calls.some(value => value === "string-create" || value === "chunked-create"), false)
  assert.equal(oldFatal, 0, "过期命令不能调用UI fatal污染新会话")
  assert.ok(f.calls.some(value => value.includes("manual reader取消失败")), JSON.stringify(f.calls))
  assert.equal((await f.ports.hostTransitionRef.current).kind, "failed")
  assert.deepEqual(f.chosenSetups, [])
})

test("默认String旧读取pending时明确新局不等待decoder屏障，旧结果不回写", { timeout: 10000 }, async () => {
  const f = await fixture(), entered = deferred<void>(), oldRead = deferred<typeof f.archive | null>()
  let reads = 0
  f.ports.getBrowserSaveRepository = () => ({ load: async () => { throw new Error("不走remote") }, loadFrozen: async () => { reads++; entered.resolve(); return oldRead.promise } })
  const first = createSessionHostLifecycle(f.ports), starting = first.start(); await entered.promise
  f.source.reset(); await first.dispose()
  const second = createSessionHostLifecycle(f.ports), next = second.start()
  await new Promise<void>(resolve => setImmediate(resolve))
  const createdBeforeOldRead = f.calls.filter(value => value === "string-create").length
  oldRead.resolve(f.archive); await Promise.all([starting, next])
  assert.equal(createdBeforeOldRead, 1, "String读取不持有lease decoder，不应阻止明确新局")
  assert.equal(reads, 1)
  assert.equal(f.calls.filter(value => value === "setup").length, 1)
  assert.equal(f.calls.filter(value => value === "ready:true").length, 1)
  await second.dispose()
})

test("前序宿主与lease reader同时清理失败时共享transition保留两者且重挂不重复聚合", { timeout: 10000 }, async () => {
  const f = await fixture(), previous = new Error("前序host释放失败"), cleanup = new AggregateError([new Error("reader解锁失败")], "reader释放失败")
  await assert.rejects(f.source.runSelectionRead(async () => { throw cleanup }), error => error === cleanup)
  f.ports.hostTransitionRef.current = Promise.resolve({ kind: "failed", error: previous })
  const first = createSessionHostLifecycle(f.ports); await first.start()
  const outcome = await f.ports.hostTransitionRef.current
  assert.equal(outcome.kind, "failed")
  if (outcome.kind !== "failed") throw new Error("失败transition缺失")
  assert.ok(outcome.error instanceof AggregateError)
  assert.deepEqual(outcome.error.errors, [previous, cleanup])
  const second = createSessionHostLifecycle(f.ports); await second.start()
  const remounted = await f.ports.hostTransitionRef.current
  assert.equal(remounted.kind, "failed")
  if (remounted.kind !== "failed") throw new Error("重挂失败transition缺失")
  assert.equal(remounted.error, outcome.error, "同一sticky错误不应重挂一层又一层")
  assert.equal(f.calls.some(value => value === "string-create" || value === "chunked-create"), false)
})
