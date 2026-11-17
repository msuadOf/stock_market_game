// 仅 diagnostics 调用；真实 Rust authority、IndexedDB Worker 与 Outbound 的异步验收。
import { normalizeSerdeValue } from "./protocol/guards.ts";
import type { WasmCheckpointClient } from "./wasm-checkpoint-client.ts";

type Key = { generation: string; writerEpoch: string; seq: string };
type Row = { id: string; generation: string; tick: string; seq: string; civilReady: boolean; archive: { queued: Key[]; durable: Key | null; blocked: boolean }; observer: { lease: string; accepted: string }; [key: string]: unknown };
export interface ContractBindings {
  market_runner_contract_snapshot(handle: number, id: string): unknown;
  market_runner_contract_fail(handle: number): void;
  market_runner_contract_queue_full(handle: number): void;
  market_runner_step_once(handle: number): void;
  market_runner_end_civil(handle: number): void;
  market_publisher_take(handle: number, lease: bigint): unknown;
  market_publisher_flushed(handle: number, lease: bigint, ordinal: bigint): void;
  market_publisher_bind(handle: number, generation: bigint): bigint;
  market_publisher_hold(handle: number, lease: bigint): bigint;
  market_publisher_resume(handle: number, lease: bigint, ordinal: bigint): boolean;
  market_publisher_start(handle: number, lease: bigint): void;
  market_publisher_ack(handle: number, lease: bigint, ordinal: bigint): void;
  market_publisher_release(handle: number, lease: bigint): void;
  market_runner_publication_ordinal(handle: number): string;
  market_archive_receipt(handle: number, generation: bigint, epoch: bigint, seq: bigint): string;
  save(handle: number): unknown;
}
export interface BrowserContractRuntime {
  current(): { handle: number; generation: number; wasm: ContractBindings };
  checkpoint: WasmCheckpointClient;
  restore(slot: unknown): Promise<void>;
  rejectRestore(slot: unknown): void;
  rejectGeneration(generation: number): void;
}
const IDS = ["initial", "manual", "without-observer", "slow-observer", "archive-full", "blocked", "old-receipt", "bad-receipt", "duplicate-receipt", "prepare-rejected", "replaced", "old-generation", "new-lease", "old-lease", "prefix-failure", "terminal-repeat", "shutdown"] as const;
function check(condition: unknown, message: string): asserts condition { if (!condition) throw new Error(message); }
function rejects(action: () => unknown, message: string): void {
  let rejected = false;
  try { action(); } catch { rejected = true; }
  check(rejected, message);
}

/** 仅验证读取点适配serde Map，生产发布路径不新增复制。 */
export function readAuthorityContractSnapshot(wasm: Pick<ContractBindings, "market_runner_contract_snapshot">, handle: number, id: string): Row {
  const normalized = normalizeSerdeValue(wasm.market_runner_contract_snapshot(handle, id), "authority.contract.snapshot");
  check(normalized !== null && typeof normalized === "object" && !Array.isArray(normalized), "验证snapshot必须为对象");
  const row = normalized as Row;
  check(row.id === id && typeof row.civilReady === "boolean" && row.archive !== null && typeof row.archive === "object" && row.observer !== null && typeof row.observer === "object", "验证snapshot字段或步骤不匹配");
  return row;
}

/** MessagePort收到真实值后才标准化，不更改生产发布编码。 */
export function normalizeAuthorityContractPacket(packet: unknown): Record<string, unknown> {
  const value = normalizeSerdeValue(packet, "authority.contract.publication");
  check(value !== null && typeof value === "object" && !Array.isArray(value), "发布包不是对象");
  const result = value as Record<string, unknown>;
  check(["update", "failure", "gap", "closed"].includes(String(result.kind)), "发布包kind无效");
  return result;
}

/** 只在MessagePort真实接收完成后调用；停收与排空场景的应用接纳分开。 */
export function confirmAuthorityContractPacket(wasm: Pick<ContractBindings, "market_publisher_flushed" | "market_publisher_ack">, handle: number, lease: bigint, value: Record<string, unknown>, accept: boolean): void {
  if (typeof value.publicationOrdinal !== "string") return;
  const ordinal = BigInt(value.publicationOrdinal);
  wasm.market_publisher_flushed(handle, lease, ordinal);
  if (accept) wasm.market_publisher_ack(handle, lease, ordinal);
}

export async function runBrowserAuthorityContract(runtime: BrowserContractRuntime): Promise<Row[]> {
  const rows: Row[] = [];
  let saved: unknown;
  let failed: Key | undefined;
  let durable: Key | undefined;
  let oldLease = 0n;
  let lease = 0n;
  let terminalDeliveries = 0;
  const packets: unknown[] = [];
  let retired: { handle: number; generation: number; lease: bigint } | undefined;
  // 独占真实Outbound后经MessagePort接收；慢观察者停ACK，前缀排空恢复接纳。
  const transport = async (accept = false) => {
    const { handle, wasm } = runtime.current();
    for (let index = 0; index < 64; index++) {
      const packet = wasm.market_publisher_take(handle, lease);
      if (packet === null) return;
      const channel = new MessageChannel();
      let received: unknown;
      try {
        received = await new Promise<unknown>((resolve, reject) => {
          channel.port2.onmessage = event => resolve(event.data);
          channel.port2.onmessageerror = () => reject(new Error("验收MessagePort解码失败"));
          channel.port1.postMessage(packet);
        });
      } finally { channel.port1.close(); channel.port2.close(); }
      const value = normalizeAuthorityContractPacket(received);
      packets.push({ kind: value.kind, ordinal: value.publicationOrdinal, failure: value.failure });
      confirmAuthorityContractPacket(wasm, handle, lease, value, accept);
      if (value.kind === "failure" && !Object.hasOwn(value, "publicationOrdinal")) terminalDeliveries++;
      if (value.kind === "closed" || value.kind === "gap") return;
    }
    throw new Error("验收发布未在有界读取内停止");
  };
  const state = (id = "probe") => { const { handle, wasm } = runtime.current(); return readAuthorityContractSnapshot(wasm, handle, id); };
  const step = () => { const { handle, wasm } = runtime.current(); wasm.market_runner_step_once(handle); };
  const civil = () => { const { handle, wasm } = runtime.current(); wasm.market_runner_end_civil(handle); };
  const boundary = () => { for (let i = 0; !state().civilReady; i++) { check(i < 1000, "验收日过长"); step(); } };
  const drain = async () => { const { handle, wasm } = runtime.current(); await runtime.checkpoint.waitIdle(handle, wasm); };
  const receipt = (key: Key) => { const { handle, wasm } = runtime.current(); return wasm.market_archive_receipt(handle, BigInt(key.generation), BigInt(key.writerEpoch), BigInt(key.seq)); };
  for (const id of IDS) {
    const { handle, generation, wasm } = runtime.current();
    switch (id) {
      case "initial": lease = wasm.market_publisher_bind(handle, BigInt(generation)); break;
      case "manual": step(); break;
      case "without-observer":
      case "slow-observer": {
        // publisher 已由实际副实例 quiesce；没有应用 ACK，真实归档仍必须完成。
        boundary(); civil(); await drain();
        const actual = state();
        check(actual.archive.durable !== null && actual.archive.queued.length === 0, "IndexedDB Worker 收据未确认 durable");
        check(actual.observer.accepted === "0", "停收时不应产生应用 ACK");
        durable = actual.archive.durable;
        if (id === "slow-observer") await transport();
        break;
      }
      case "archive-full":
        boundary(); wasm.market_runner_contract_queue_full(handle); civil();
        check(state().archive.blocked, "入队拒绝未阻断归档");
        failed = state().archive.queued[0]; check(failed, "失败候选未保留"); saved = wasm.save(handle); break;
      case "blocked": {
        const before = state(); rejects(step, "blocked 仍允许推进");
        check(state().seq === before.seq && state().tick === before.tick, "拒绝后发生了市场提交"); break;
      }
      case "old-receipt": check(failed && receipt({ ...failed, writerEpoch: "0" }) === "stale", "旧writer收据被接受"); break;
      case "bad-receipt": check(failed && receipt({ ...failed, seq: (BigInt(failed.seq) + 1n).toString() }) === "failed", "乱序收据没有显式诊断"); break;
      case "duplicate-receipt": check(durable && receipt(durable) === "stale", "重复收据被再次确认"); break;
      case "prepare-rejected": rejects(() => runtime.rejectRestore(saved), "旧generation的prepare未拒绝"); check(state().generation === String(generation), "prepare失败改变generation"); break;
      case "replaced":
        retired = { handle, generation, lease };
        await runtime.restore(saved);
        { const current = runtime.current(); lease = current.wasm.market_publisher_bind(current.handle, BigInt(current.generation)); check(current.generation === generation + 1, "恢复未换代"); }
        break;
      case "old-generation":
        check(failed && receipt(failed) === "stale", "旧generation收据被接受");
        check(retired, "缺少旧owner身份");
        rejects(() => runtime.rejectGeneration(retired!.generation), "实际命令gate接受旧generation");
        rejects(() => wasm.market_publisher_take(retired!.handle, retired!.lease), "旧owner发布包仍能消费");
        break;
      case "new-lease":
        oldLease = lease; lease = wasm.market_publisher_hold(handle, lease);
        check(wasm.market_publisher_resume(handle, lease, BigInt(wasm.market_runner_publication_ordinal(handle))), "新lease基线未安装");
        wasm.market_publisher_start(handle, lease); break;
      case "old-lease":
        rejects(() => wasm.market_publisher_ack(handle, oldLease, 0n), "旧ACK被接受");
        rejects(() => wasm.market_publisher_resume(handle, oldLease, 0n), "旧cut被接受");
        rejects(() => wasm.market_publisher_release(handle, oldLease), "旧release删除新流"); break;
      case "prefix-failure":
        while (state().civilReady) civil();
        await drain(); step(); wasm.market_runner_contract_fail(handle); await transport(true);
        check(terminalDeliveries === 1, `成功前缀后terminal未恰好交付一次：${JSON.stringify({ terminalDeliveries, packets, state: state() })}`); break;
      case "terminal-repeat": wasm.market_runner_contract_fail(handle); rejects(step, "terminal后仍可推进"); await transport(true); check(terminalDeliveries === 1, "terminal被重复交付"); break;
      case "shutdown": {
        await drain(); const final = state(id);
        check(final.archive.queued.length === 0, "shutdown仍有未确认归档");
        await runtime.checkpoint.close();
        // finish 已验证真实归档排空后才移除 ArchivePort；保存的是移除前最后一次真实采样。
        rows.push({ ...final, productPackets: structuredClone(packets) }); continue;
      }
    }
    rows.push({ ...state(id), productPackets: structuredClone(packets) });
  }
  return rows;
}
