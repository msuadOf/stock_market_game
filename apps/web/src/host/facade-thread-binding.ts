import { CAPACITY, GEN_BITS, GEN_BITS_DEFAULT, HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE } from "./facade-slot-protocol.ts";
import type { FacadeArenaView } from "./facade-supervisor.ts";

export type FacadeThreadRole = "engine" | "child" | "ingress";
export interface FacadeThreadIdentity {
  readonly workerId: number;
  readonly slot: number;
  readonly gen: number;
}
export type FacadeControlMessage =
  | { readonly type: "facadeSpawn"; readonly slot: number; readonly gen: number; readonly name: string }
  | { readonly type: "facadeTerminateChild"; readonly workerId: number };
export interface FacadeThreadConfiguration {
  readonly memory: WebAssembly.Memory;
  readonly role: FacadeThreadRole;
  readonly postControl: (message: FacadeControlMessage) => void;
  readonly bootstrap?: FacadeThreadIdentity;
}
export interface FacadeSlotLayout {
  readonly baseAddr: number;
  readonly headerWords: number;
  readonly slotStride: number;
  readonly joinStride: number;
  readonly capacity: number;
  readonly genBits: number;
}

function fail(message: string): never {
  throw new Error(`facade_thread_env：${message}`);
}

function unsigned(value: unknown, field: string): number {
  if (typeof value !== "number" || !Number.isInteger(value) || value < 0 || value > 0xffffffff) {
    fail(`${field} 必须为 u32 整数，收到 ${String(value)}`);
  }
  return value;
}

// extern i32 进入 JS 可为负值；校验后才还原位型，禁止截断小数或域外数。
function rawUnsigned(value: number, field: string): number {
  if (!Number.isInteger(value) || value < -0x80000000 || value > 0xffffffff) {
    fail(`${field} 必须为完整 32 位整数，收到 ${String(value)}`);
  }
  return value >>> 0;
}

function dedicatedWorker(): boolean {
  const scope = globalThis as typeof globalThis & {
    DedicatedWorkerGlobalScope?: new (...args: never[]) => object;
  };
  return typeof scope.DedicatedWorkerGlobalScope === "function" && globalThis instanceof scope.DedicatedWorkerGlobalScope;
}

/** 每个 Worker 的独立绑定；isWorker 仅为隔离短 fixture 注入，生产单例使用真实检测。 */
export function createFacadeThreadBinding(options: { readonly isWorker?: () => boolean } = {}) {
  const isWorker = options.isWorker === undefined ? dedicatedWorker : options.isWorker;
  let configuration: FacadeThreadConfiguration | undefined;
  let layout: FacadeSlotLayout | undefined;
  let exitIntent: FacadeThreadIdentity | undefined;

  function bound(): FacadeThreadConfiguration {
    if (configuration === undefined) fail("未绑定 memory/角色；须先 bindFacadeThreadEnvironment 再实例化 WASM");
    return configuration;
  }

  function currentLayout(): FacadeSlotLayout {
    bound();
    if (layout === undefined) fail("layout 尚未装入；须使用 facade_slot_layout() 的实际导出");
    return layout;
  }

  function identityInLayout(slot: number, gen: number, current: FacadeSlotLayout): void {
    if (slot >= current.capacity) fail(`slot ${slot} 超出 layout capacity ${current.capacity}`);
    if (gen > 2 ** current.genBits - 1) fail(`gen ${gen} 超出 layout genBits ${current.genBits}`);
  }

  function bindFacadeThreadEnvironment(input: FacadeThreadConfiguration): void {
    if (configuration !== undefined) fail("重复绑定；每个 Worker 只允许一次绑定");
    const { memory, role, postControl, bootstrap } = input;
    if (!(memory instanceof WebAssembly.Memory) || !(memory.buffer instanceof SharedArrayBuffer)) {
      fail("memory 必须为 shared WebAssembly.Memory（共享内存）");
    }
    if (role !== "engine" && role !== "child" && role !== "ingress") fail(`未知角色 ${String(role)}`);
    if (typeof postControl !== "function") fail("postControl 必须为控制消息发送函数");
    let identity: FacadeThreadIdentity | undefined;
    if (role === "child") {
      if (bootstrap === undefined || bootstrap === null) fail("child 缺少 bootstrap 身份");
      identity = Object.freeze({
        workerId: unsigned(bootstrap.workerId, "bootstrap.workerId"),
        slot: unsigned(bootstrap.slot, "bootstrap.slot"),
        gen: unsigned(bootstrap.gen, "bootstrap.gen"),
      });
    } else if (bootstrap !== undefined) {
      fail(`${role} 角色不接受 child bootstrap 身份`);
    }
    configuration = Object.freeze({ memory, role, postControl, bootstrap: identity });
  }

  function installFacadeSlotLayout(input: FacadeSlotLayout): void {
    const config = bound();
    if (layout !== undefined) fail("重复装入 layout");
    const { baseAddr, headerWords, slotStride, joinStride, capacity, genBits } = input;
    const candidate = Object.freeze({
      baseAddr: unsigned(baseAddr, "layout.baseAddr"),
      headerWords: unsigned(headerWords, "layout.headerWords"),
      slotStride: unsigned(slotStride, "layout.slotStride"),
      joinStride: unsigned(joinStride, "layout.joinStride"),
      capacity: unsigned(capacity, "layout.capacity"),
      genBits: unsigned(genBits, "layout.genBits"),
    });
    if (candidate.headerWords !== HEADER_WORDS || candidate.slotStride !== SLOT_STRIDE || candidate.joinStride !== JOIN_STRIDE) {
      fail("layout header/stride 与 Rust/TS 跨语言布局不匹配");
    }
    if (candidate.baseAddr % 4 !== 0 || capacity === 0 || genBits < 1 || genBits > GEN_BITS_DEFAULT) {
      fail("layout 地址必须对齐，capacity 必须为正，genBits 必须在 1..29");
    }
    const wordLength = headerWords + capacity * (slotStride + joinStride);
    if (baseAddr + wordLength * 4 > config.memory.buffer.byteLength) fail("layout 超出当前 memory.buffer 范围");
    const words = new Int32Array(config.memory.buffer, baseAddr, wordLength);
    if ((Atomics.load(words, CAPACITY) >>> 0) !== capacity || (Atomics.load(words, GEN_BITS) >>> 0) !== genBits) {
      fail("layout capacity/genBits 与共享内存 header 不一致");
    }
    if (config.bootstrap !== undefined) identityInLayout(config.bootstrap.slot, config.bootstrap.gen, candidate);
    layout = candidate;
  }

  function getArenaView(): FacadeArenaView {
    const config = bound();
    const current = currentLayout();
    return {
      words: new Int32Array(config.memory.buffer, current.baseAddr,
        current.headerWords + current.capacity * (current.slotStride + current.joinStride)),
      capacity: current.capacity,
      genBits: current.genBits,
    };
  }

  function wordView(address: number): { words: Int32Array; index: number } {
    const config = bound();
    const normalized = rawUnsigned(address, "wordAddr 地址");
    const buffer = config.memory.buffer;
    if (normalized % 4 !== 0 || normalized + 4 > buffer.byteLength) fail(`wordAddr 地址 ${normalized} 未对齐或超出当前 buffer`);
    return { words: new Int32Array(buffer), index: normalized / 4 };
  }

  function facade_futex_wait(wordAddr: number, expected: number, timeoutMs: bigint): number {
    bound();
    if (!isWorker()) fail("主线程不可 Atomics.wait；仅允许真实 DedicatedWorker");
    const { words, index } = wordView(wordAddr);
    const expectedWord = rawUnsigned(expected, "expected 完整 W") | 0;
    if (typeof timeoutMs !== "bigint" || timeoutMs < -1n || timeoutMs > BigInt(Number.MAX_SAFE_INTEGER)) {
      fail("timeoutMs 必须为 i64 BigInt：-1n 表无限，其余为非负安全整数超时");
    }
    const result = Atomics.wait(words, index, expectedWord, timeoutMs === -1n ? Infinity : Number(timeoutMs));
    switch (result) {
      case "ok": return 0;
      case "not-equal": return 1;
      case "timed-out": return 2;
      default: return fail(`Atomics.wait 返回未知结果 ${String(result)}`);
    }
  }

  function facade_futex_notify(wordAddr: number): void {
    const { words, index } = wordView(wordAddr);
    Atomics.notify(words, index);
  }

  function controlRole(): FacadeThreadConfiguration {
    const config = bound();
    if (config.role === "ingress") fail("ingress 角色不得发 spawn/terminate 线程控制");
    return config;
  }

  function facade_spawn_notify(slot: number, gen: number, namePtr: number, nameLen: number): void {
    const config = controlRole();
    const current = currentLayout();
    const messageSlot = rawUnsigned(slot, "slot");
    const messageGen = rawUnsigned(gen, "gen");
    identityInLayout(messageSlot, messageGen, current);
    const pointer = rawUnsigned(namePtr, "namePtr");
    const length = rawUnsigned(nameLen, "nameLen");
    const buffer = config.memory.buffer;
    if (pointer + length > buffer.byteLength) fail("namePtr/nameLen 超出当前 memory.buffer");
    // H5 调用返回前复制，原共享内存可随即复用；保留 U+FEFF，不把它当传输 BOM 丢弃。
    const bytes = new Uint8Array(length);
    bytes.set(new Uint8Array(buffer, pointer, length));
    let name: string;
    try {
      name = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes);
    } catch (cause) {
      throw new Error("facade_thread_env：name 不是有效 UTF-8", { cause });
    }
    config.postControl({ type: "facadeSpawn", slot: messageSlot, gen: messageGen, name });
  }

  function facade_worker_exit_notify(workerId: number, slot: number, gen: number): void {
    const config = bound();
    if (config.role !== "child") fail(`${config.role} 角色不可记录 child exit`);
    if (exitIntent !== undefined) fail("重复记录 child exit 意图");
    const intent = Object.freeze({ workerId: rawUnsigned(workerId, "workerId"),
      slot: rawUnsigned(slot, "slot"), gen: rawUnsigned(gen, "gen") });
    identityInLayout(intent.slot, intent.gen, currentLayout());
    const bootstrap = config.bootstrap;
    if (bootstrap === undefined || intent.workerId !== bootstrap.workerId || intent.slot !== bootstrap.slot || intent.gen !== bootstrap.gen) {
      fail("exit 身份与 bootstrap 不匹配");
    }
    // Rust 此时尚未返回；这里只记录意图，不 post/close/destroy。
    exitIntent = intent;
  }

  function requireExitIntent(): FacadeThreadIdentity {
    bound();
    if (exitIntent === undefined) fail("缺失 child exit 意图");
    return exitIntent;
  }

  function facade_terminate_child(workerId: number): void {
    const config = controlRole();
    config.postControl({ type: "facadeTerminateChild", workerId: rawUnsigned(workerId, "workerId") });
  }

  return { bindFacadeThreadEnvironment, installFacadeSlotLayout, getArenaView, requireExitIntent,
    facade_futex_wait, facade_futex_notify, facade_spawn_notify, facade_worker_exit_notify, facade_terminate_child };
}
