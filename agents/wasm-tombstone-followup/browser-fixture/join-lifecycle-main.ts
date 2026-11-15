import { createEngineSupervisor } from "../../../apps/web/src/host/engine-supervisor.ts";
import { decodeGen, decodeJoinState, joinWordOffset } from "../../../apps/web/src/host/facade-slot-protocol.ts";
import type { HostFailure } from "../../../apps/web/src/host/host-update.ts";
import type { WasmLifecycleObservation } from "../../../apps/web/src/host/wasm-lifecycle-observation.ts";

const mode = new URLSearchParams(location.search).get("mode");
const trace: { stage: string; result?: unknown; error?: string } = { stage: "startup" };
Object.assign(window, { joinLifecycle: trace });

function require(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function observeTermination(worker: Worker) {
  const terminate = worker.terminate.bind(worker);
  let count = 0;
  worker.terminate = () => {
    terminate();
    count += 1;
  };
  return { worker, get terminations() { return count; } };
}

type EngineReply = { type: "ready" } | { type: "quiescent" }
  | { type: "spawned"; joinIndex: number }
  | { type: "joined"; value: number; duplicate: string }
  | { type: "fixture-error"; message: string };

async function observe() {
  require(mode === "normal" || mode === "cleanup-failure" || mode === "forced", "未知 join 生命周期验收模式");
  require(crossOriginIsolated, "必须启用真实共享内存浏览器隔离");
  const words = new Int32Array(new SharedArrayBuffer(16));
  const witness = () => Array.from(words, (_, index) => Atomics.load(words, index));
  const engineHandle = observeTermination(new Worker(new URL("./join-lifecycle-engine.ts", import.meta.url), { type: "module" }));
  const engine = engineHandle.worker;
  const children: ReturnType<typeof observeTermination>[] = [];
  const failures: HostFailure[] = [];
  const anomalies: unknown[] = [];
  const events: WasmLifecycleObservation[] = [];
  let reportGate!: () => void;
  const gateEntered = new Promise<void>((resolve) => { reportGate = resolve; });
  const supervisor = createEngineSupervisor({
    engine,
    createChildWorker() {
      const child = new Worker(new URL("./join-lifecycle-child.ts", import.meta.url), { type: "module" });
      children.push(observeTermination(child));
      child.addEventListener("message", (event) => {
        if (event.data?.type === "join-lifecycle-gate-entered") reportGate();
      });
      child.postMessage({ type: "join-lifecycle-gate", signal: words.buffer });
      return child;
    },
    onFatalFailure(failure) { failures.push(failure); },
    onSpawnFailure(failure) { anomalies.push(failure); },
    onLifecycleObservation(event) { events.push(event); },
    reportError(error) { anomalies.push(String(error)); },
  });

  function exchange<T extends EngineReply["type"]>(reply: T, send: () => void) {
    return new Promise<Extract<EngineReply, { type: T }>>((resolve, reject) => {
      const receive = (event: MessageEvent<EngineReply>) => {
        engine.removeEventListener("message", receive);
        const value = event.data;
        if (value.type === "fixture-error") reject(new Error(value.message));
        else if (value.type !== reply) reject(new Error(`engine 回执类型不符，预期 ${reply}，实际 ${value.type}`));
        else resolve(value as Extract<EngineReply, { type: T }>);
      };
      engine.addEventListener("message", receive);
      try { send(); }
      catch (error) { engine.removeEventListener("message", receive); reject(error); }
    });
  }

  try {
    await exchange("ready", () => supervisor.bootstrapEngine({ type: "start" }));
    trace.stage = "configured";
    const { memory, layout } = supervisor.getSubstrate();
    const input = mode === "normal" ? 0xA701 : mode === "cleanup-failure" ? 0xA702 : 0xA703;
    const { joinIndex } = await exchange("spawned", () => engine.postMessage({ type: "spawn", input }));
    await gateEntered;
    const identity = events.find((event) => event.type === "childSpawn");
    require(identity?.type === "childSpawn", "必须观察到生产 supervisor 的真实 childSpawn");
    require(children.length === 1 && identity.slot === joinIndex, "夹具需要真实的单 child 同槽 join");
    const readJoin = () => {
      const arena = new Uint32Array(memory.buffer, layout.baseAddr);
      const joinWord = Atomics.load(arena, joinWordOffset(joinIndex, layout.capacity));
      return { gen: decodeGen(joinWord, layout.genBits), state: decodeJoinState(joinWord, layout.genBits) };
    };
    const beforeJoin = { gate: witness(), join: readJoin(), counts: supervisor.snapshot() };
    trace.stage = "destroy-gated-before-join";
    const joined = await exchange("joined", () => engine.postMessage({ type: "join" }));
    await exchange("quiescent", () => engine.postMessage({ type: "quiesce" }));
    supervisor.markQuiescent(engine);
    supervisor.beginShutdown(performance.now() + 4000);
    const drain = supervisor.waitForChildrenExit(performance.now() + 4000);
    let drainState = "pending";
    const drained = drain.then(() => { drainState = "drained"; }, () => { drainState = "rejected"; });
    supervisor.checkpoint();
    await Promise.resolve();
    const afterJoin = { gate: witness(), join: readJoin(), counts: supervisor.snapshot(), drainState, events: [...events] };
    trace.stage = "destroy-gated-after-join";

    if (mode !== "forced") {
      Atomics.store(words, 0, mode === "normal" ? 1 : 2);
      Atomics.notify(words, 0);
    } else {
      // 不打开闸门，不把真实 Worker.terminate 冒充 raw destroy 或正常 exit。
      supervisor.discard({ code: "JOIN_LIFECYCLE_FIXTURE_FORCED_STOP", where: "join-lifecycle-check", message: "测试端要求强制停止仍被闸门阻塞的 Worker" });
    }
    await drained;
    const sampledBytes = memory.buffer.byteLength;
    const receipt = supervisor.dispose();
    return {
      mode, identity, beforeJoin, afterJoin, joined, gate: witness(), drainState, sampledBytes, events,
      counts: supervisor.snapshot(),
      receipt: receipt instanceof Error ? { kind: "unknown", message: receipt.message } : receipt,
      childTerminations: children[0].terminations, engineTerminations: engineHandle.terminations, failures, anomalies,
    };
  } finally {
    supervisor.dispose();
  }
}

void observe().then((result) => { trace.result = result; }, (error) => { trace.error = String(error); });
