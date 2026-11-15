// 持久化副实例：不消费观察流，序列化和存储都在此执行。
import { sharedControlWord } from "../../../../packages/libos/browser/shared-substrate-publisher.ts";
import { bindFacadeThreadEnvironment, getArenaView, installFacadeSlotLayout } from "./engine-clock-env.ts";
import { instantiateFacadeModule, type FacadeSyncInitializer } from "./facade-wasm-module.ts";
import { parseFacadeEngineSubstrate } from "./facade-engine-protocol.ts";
import { normalizeSerdeMaps } from "./serde-normalize.ts";
import { WorkerCheckpointRepository } from "../save/worker-checkpoint-repository.ts";
import { validateDayEndArchive } from "../save/day-end-candidate.ts";
import { parseSaveSlot } from "../save/save-schema.ts";

type Bindings = FacadeSyncInitializer & {
  market_archive_take(): unknown;
  market_archive_signal(): number;
  market_archive_bind_tombstone(address: number): void;
  market_archive_release(): void;
};

const scope = self as unknown as {
  postMessage(value: unknown): void;
  addEventListener(type: "message", listener: (event: MessageEvent<unknown>) => void): void;
  close(): void;
  readonly location: Location;
  readonly indexedDB: IDBFactory;
};

function record(value: unknown): Readonly<Record<string, unknown>> {
  if (value == null || typeof value !== "object" || Array.isArray(value)) throw new Error("归档消息必须为对象");
  return value as Readonly<Record<string, unknown>>;
}

function seq(value: unknown): bigint {
  if (typeof value !== "string" || !/^(0|[1-9][0-9]*)$/.test(value) || BigInt(value) > 0xffffffffffffffffn) throw new Error("归档序号无效");
  return BigInt(value);
}

let booted = false;
let clean = false;
async function bootstrap(raw: unknown): Promise<void> {
  if (booted) throw new Error("后台归档副实例只能装配一次");
  booted = true;
  const message = record(raw);
  if (message.type !== "bindCheckpoint" || typeof message.lease !== "string" || !/^[0-9a-f-]{36}$/.test(message.lease)
    || typeof message.address !== "number") throw new Error("后台归档启动身份无效");
  const generation = seq(message.generation);
  const writerEpoch = seq(message.writerEpoch);
  const substrate = parseFacadeEngineSubstrate({ type: "facadeEngineSubstrate", module: message.module, memory: message.memory, layout: message.layout, diagnostics: message.diagnostics });
  bindFacadeThreadEnvironment({ memory: substrate.memory, role: "ingress", postControl() { throw new Error("归档 Worker 不得创建计算线程"); } });
  let bindings: Bindings;
  if (substrate.diagnostics) {
    if (!import.meta.env.DEV) throw new Error("正式构建禁止 diagnostics WASM");
    const url = new URL("/wasm-diagnostics-pkg/web_wasm.js", scope.location.origin);
    bindings = await import(/* @vite-ignore */ url.href) as Bindings;
  } else bindings = await import("../../wasm-pkg/web_wasm.js") as unknown as Bindings;
  const exports = instantiateFacadeModule(bindings, substrate.module, substrate.memory);
  let repo: WorkerCheckpointRepository | undefined;
  try {
    bindings.market_archive_bind_tombstone(message.address);
    installFacadeSlotLayout(substrate.layout);
    getArenaView();
    const signal = sharedControlWord(substrate.memory, bindings.market_archive_signal());
    repo = new WorkerCheckpointRepository(scope.indexedDB, message.lease);
    await repo.activate();

    const waitAsync = Reflect.get(Atomics, "waitAsync") as ((array: Int32Array, index: number, value: number) =>
      { async: false; value: "not-equal" } | { async: true; value: Promise<"ok"> }) | undefined;
    if (typeof waitAsync !== "function") throw new Error("存档 Worker 需要 Atomics.waitAsync");
    scope.postMessage({ type: "ready" });
    while (true) {
      const before = Atomics.load(signal, 0);
      const data = bindings.market_archive_take();
      if (data === null) {
        const waiting = waitAsync.call(Atomics, signal, 0, before);
        if (waiting.async) await waiting.value;
        continue;
      }
      const packet = record(data);
      if (packet.kind === "closed") break;
      if (packet.kind !== "checkpoint" || seq(packet.generation) !== generation || seq(packet.writerEpoch) !== writerEpoch) {
        throw new Error("归档任务收到不匹配的世代或租约");
      }
      const sequence = seq(packet.seq);
      let applied: boolean;
      try {
        const archive = validateDayEndArchive(parseSaveSlot(normalizeSerdeMaps(packet.slot)));
        if (BigInt(archive.snapshot.seq) !== sequence) throw new Error("归档候选与市场序号不一致");
        applied = await repo.write(sequence, archive);
      } catch (error) {
        // 已知身份的单份归档失败只结束当前候选；后续日终仍可保存。
        scope.postMessage({ type: "writeFailed", seq: sequence.toString(), message: error instanceof Error ? error.message : String(error) });
        continue;
      }
      if (!applied) throw new Error("快速槽写入租约已被替换");
      scope.postMessage({ type: "stored", seq: sequence.toString() });
    }
  } finally {
    try { await repo?.close(); }
    finally {
      bindings.market_archive_release();
      exports.__wbindgen_thread_destroy(0, 0, 0);
      clean = true;
    }
  }
  scope.postMessage({ type: "closed" });
  scope.close();
}

scope.addEventListener("message", (event: MessageEvent<unknown>) => {
  void bootstrap(event.data).catch((error: unknown) => {
    scope.postMessage({ type: "failed", clean, message: error instanceof Error ? error.message : String(error) });
    scope.close();
  });
});
