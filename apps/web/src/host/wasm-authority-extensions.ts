// 仅独立diagnostics产品实例使用；每个破坏性边界必须新建真实Worker owner。
import { normalizeAuthorityContractPacket, readAuthorityContractSnapshot, type ContractBindings } from "./wasm-authority-contract.ts";
import { normalizeSerdeValue } from "./protocol/guards.ts";
import type { WasmCheckpointClient } from "./wasm-checkpoint-client.ts";
export interface BrowserExtensionRuntime {
  current(): { handle: number; generation: number; wasm: ContractBindings & { market_runner_set_running(handle: number, running: boolean): void } };
  checkpoint: WasmCheckpointClient;
  trackObserver(handle: number, lease: bigint): void;
  prepareInvalid(slot: unknown): void;
  restore(slot: unknown, fault?: "postcommit-archive-attach"): Promise<void>;
  shutdown(): Promise<{ report: unknown; archiveFailure?: unknown }>;
  forceArchiveWorkerExit(): void;
  tryReuseAfterUnknownExit(): Promise<void>;
}
function require(condition: unknown, message: string): asserts condition { if (!condition) throw new Error(message); }
async function rejection(action: () => unknown | Promise<unknown>, message: string): Promise<string> {
  try { await action(); } catch (error) { return String(error); }
  throw new Error(message);
}
export async function runBrowserAuthorityExtension(runtime: BrowserExtensionRuntime, id: string): Promise<unknown> {
  const initial = runtime.current();
  let lease = initial.wasm.market_publisher_bind(initial.handle, BigInt(initial.generation));
  runtime.trackObserver(initial.handle, lease);
  const state = (label: string) => { const { handle, wasm } = runtime.current(); return readAuthorityContractSnapshot(wasm, handle, label); };
  const step = () => { const { handle, wasm } = runtime.current(); wasm.market_runner_step_once(handle); };
  const civil = () => { const { handle, wasm } = runtime.current(); wasm.market_runner_end_civil(handle); };
  const boundary = () => { for (let n = 0; !state(id).civilReady; n++) { require(n < 1000, "短日超限"); step(); } };
  const drain = async () => { const { handle, wasm } = runtime.current(); await runtime.checkpoint.waitIdle(handle, wasm); };
  const day = async () => { boundary(); civil(); await drain(); };
  const receive = async () => {
    const { handle, wasm } = runtime.current(), packets = [];
    for (let n = 0; n < 2048; n++) {
      const raw = wasm.market_publisher_take(handle, lease); if (raw === null) return packets;
      const channel = new MessageChannel();
      let actual;
      try { actual = await new Promise(resolve => { channel.port2.onmessage = event => resolve(event.data); channel.port1.postMessage(raw); }); }
      finally { channel.port1.close(); channel.port2.close(); }
      const packet = normalizeAuthorityContractPacket(actual); packets.push(packet);
      if (typeof packet.publicationOrdinal === "string") {
        wasm.market_publisher_flushed(handle, lease, BigInt(packet.publicationOrdinal));
        wasm.market_publisher_ack(handle, lease, BigInt(packet.publicationOrdinal));
      }
      if (packet.kind === "closed") return packets;
    }
    throw new Error("实际观察流未在有界收包内排空");
  };
  const before = state("extension-before");
  if (id === "prepare-rejected-running-preserved") {
    await day(); const { handle, wasm } = runtime.current();
    wasm.market_runner_set_running(handle, true); const running = state(id);
    const invalid = normalizeSerdeValue(wasm.save(handle)) as { setup: { ticks_per_day: number } };
    invalid.setup.ticks_per_day = 0;
    const failure = await rejection(() => runtime.prepareInvalid(invalid), "非法prepare没有拒绝");
    const after = state(id);
    require(after.generation === running.generation && after.tick === running.tick && after.seq === running.seq && after.paused === false, "prepare失败改变运行意图或已提交事实");
    wasm.market_runner_set_running(handle, false);
    return { before: running, after, failure, assertions: { runningPreserved: true } };
  }
  if (id === "shutdown-known-archive-failure-cleanup") {
    boundary(); initial.wasm.market_runner_contract_queue_full(initial.handle); civil();
    const failed = state(id); require(failed.archive.blocked && failed.archive.queued.length > 0, "真实queuefull未保留候选");
    const shutdown = await runtime.shutdown();
    require(shutdown.archiveFailure !== undefined && shutdown.report !== undefined, "已知归档失败必须可见且实际owner完成关闭");
    return { before, failed, shutdown, assertions: { archiveFailureVisible: true, queuedNotDurable: true, ownerCleanupReported: true } };
  }
  if (id === "shutdown-unknown-worker-exit-no-reuse") {
    runtime.forceArchiveWorkerExit();
    const failure = await rejection(() => runtime.shutdown(), "未知Worker退出被误报已释放");
    const reuse = await rejection(() => runtime.tryReuseAfterUnknownExit(), "未知退出后重用了owner");
    return { before, failure, reuse, assertions: { quiescenceUnknown: true, reuseRejected: true } };
  }
  if (id === "shutdown-pending") {
    boundary(); civil(); const queued = state(id);
    require(queued.archive.queued.length > 0, "shutdown前没有实际queued候选");
    const shutdown = await runtime.shutdown();
    require(shutdown.archiveFailure === undefined && shutdown.report !== undefined, "pending shutdown没有成功完成实际归档与owner关闭");
    return { before: queued, shutdown, assertions: { queuedDrainedByRealShutdown: true } };
  }
  if (id === "restore-postcommit-io") {
    await day(); const current = runtime.current(), sealed = current.wasm.save(current.handle);
    const old = state(id);
    const failure = await rejection(() => runtime.restore(sealed, "postcommit-archive-attach"), "新代归档attach故障未显式失败");
    const next = runtime.current(); require(next.generation === Number(old.generation) + 1, "postcommit错误伪回滚generation");
    return { before: old, nextGeneration: next.generation, failure, phase: "archive-attach", assertions: { committedGenerationRetained: true } };
  }
  if (id === "stale-release-same-generation" || id === "disconnect-resync-stale") {
    await day(); const { handle, wasm } = runtime.current(); const oldLease = lease;
    lease = wasm.market_publisher_hold(handle, oldLease);
    runtime.trackObserver(handle, lease);
    require(wasm.market_publisher_resume(handle, lease, BigInt(wasm.market_runner_publication_ordinal(handle))), "真实重同步cut未安装");
    wasm.market_publisher_start(handle, lease); const installed = state(id);
    const stale = await rejection(() => wasm.market_publisher_release(handle, oldLease), "旧release删除了新流");
    const after = state(id);
    require(after.observer.lease === installed.observer.lease && after.observer.accepted === installed.observer.accepted, "旧release改变新观察者");
    return { before, installed, after, stale, ...(id === "disconnect-resync-stale" ? { transportStageRequired: "disconnect-resync-stale" } : {}), assertions: { staleReleaseRejected: true } };
  }
  if (id === "terminal-queue-full-prefix-or-gap") {
    // 实际持有lease，1025次正常发布超过共享1024包容量；不替换queue、不推造gap。
    lease = initial.wasm.market_publisher_hold(initial.handle, lease);
    runtime.trackObserver(initial.handle, lease);
    for (let n = 0; n < 1025; n++) { if (state(id).civilReady) { civil(); await drain(); } else step(); }
    initial.wasm.market_runner_contract_fail(initial.handle);
    const packets = await receive();
    const terminal = packets.filter(packet => packet.kind === "failure" && !Object.hasOwn(packet, "publicationOrdinal"));
    require(terminal.length === 1, "真实满队列terminal必须恰好一次");
    const fault = terminal[0]!.failure as { message?: string };
    require(typeof fault?.message === "string" && fault.message.includes("观察流缺口"), "满队列未交付前缀时必须明确报告缺口");
    require((await receive()).filter(packet => packet.kind === "failure" && !Object.hasOwn(packet, "publicationOrdinal")).length === 0, "terminal重复");
    return { before, after: state(id), packets, assertions: { actualQueueOverflowGap: true, terminalOnce: true } };
  }
  throw new Error(`Browser扩展尚未接线：${id}`);
}
