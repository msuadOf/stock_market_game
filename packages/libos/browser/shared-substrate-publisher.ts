/** 共享 Memory 的独立输出通道；payload 类型和是否继续读取由上层契约决定。 */
export interface SharedSubstratePublisherPorts<T> {
  readonly memory: WebAssembly.Memory;
  readonly signal: (token: number) => number;
  readonly take: (token: number) => T | null;
  readonly deliver: (value: T, generation: number) => "continue" | "stop";
  readonly failure: (error: unknown) => void;
  readonly wait?: (view: Int32Array, observed: number) => Promise<void>;
  readonly notify?: (view: Int32Array) => void;
  readonly yieldTask: () => Promise<void>;
}

type WaitAsyncResult =
  | { readonly async: false; readonly value: "not-equal" }
  | { readonly async: true; readonly value: Promise<"ok"> };

function readWaitAsync(): (array: Int32Array, index: number, value: number) => WaitAsyncResult {
  const waitAsync: unknown = Reflect.get(Atomics, "waitAsync");
  if (typeof waitAsync !== "function") throw new Error("共享发布 Worker 需要 Atomics.waitAsync");
  return (array, index, value) => Reflect.apply(waitAsync, Atomics, [array, index, value]) as WaitAsyncResult;
}

type Reader = {
  readonly generation: number;
  readonly signal: Int32Array;
  readonly token: number;
  running: boolean;
  stopped: boolean;
  failed: boolean;
  completion: Promise<void> | null;
};

export class SharedSubstratePublisher<T> {
  private current: Reader | null = null;
  private generation = 0;
  private closed = false;
  private readonly ports: SharedSubstratePublisherPorts<T>;
  private readonly wait: (view: Int32Array, observed: number) => Promise<void>;
  private readonly notify: (view: Int32Array) => void;

  constructor(ports: SharedSubstratePublisherPorts<T>) {
    this.ports = ports;
    if (!(ports.memory instanceof WebAssembly.Memory) || !(ports.memory.buffer instanceof SharedArrayBuffer)) throw new Error("共享发布通道必须使用同一份 shared WebAssembly.Memory");
    if (ports.wait !== undefined) this.wait = ports.wait;
    else {
      const atomicWait = readWaitAsync();
      this.wait = async (view, observed) => {
        const waiter = atomicWait(view, 0, observed);
        if (waiter.async) await waiter.value;
      };
    }
    this.notify = ports.notify ?? ((view) => { Atomics.notify(view, 0); });
  }

  async bind(generation: number, token: number, bound: () => void): Promise<void> {
    if (this.closed) throw new Error("共享发布通道已关闭");
    if (!Number.isSafeInteger(generation) || generation <= this.generation) throw new Error("generation 必须为严格递增的正安全整数");
    if (!Number.isSafeInteger(token) || token < 1 || token > 0xffffffff) throw new Error("发布 token 必须为正 u32");
    const previous = this.current;
    this.generation = generation;
    await this.stopReader(previous);
    if (this.closed || generation !== this.generation) throw new Error("共享发布绑定已经失效");
    const address = this.ports.signal(token);
    const buffer = this.ports.memory.buffer;
    if (!Number.isSafeInteger(address) || address <= 0 || address % 4 !== 0 || address + 4 > buffer.byteLength) throw new Error("共享发布信号地址未对齐或超出内存边界");
    const reader: Reader = { generation, token, signal: new Int32Array(buffer, address, 1), stopped: false, running: false, failed: false, completion: null };
    this.current = reader;
    bound();
    this.startReader(reader);
  }

  async pause(generation: number): Promise<void> {
    const reader = this.requireReader(generation);
    reader.stopped = true;
    this.notify(reader.signal);
    await reader.completion;
    if (this.closed || this.current !== reader || reader.running) throw new Error("共享发布暂停已失效");
  }

  async resume(generation: number, resetCursor: (token: number) => boolean, bound: (accepted: boolean) => void, restart = true): Promise<void> {
    const reader = this.requireReader(generation);
    await this.pause(generation);
    const accepted = resetCursor(reader.token);
    bound(accepted);
    if (restart && accepted) this.start(generation, () => undefined);
  }

  start(generation: number, bound: () => void): void {
    const reader = this.requireReader(generation);
    if (reader.running || reader.failed) throw new Error("共享发布读者尚未暂停或已经失败");
    bound();
    reader.stopped = false;
    this.startReader(reader);
  }

  private requireReader(generation: number): Reader {
    const reader = this.current;
    if (this.closed || reader === null || reader.generation !== generation) throw new Error("共享发布通道 generation 已失效");
    return reader;
  }

  private startReader(reader: Reader): void {
    reader.running = true;
    reader.completion = this.run(reader, reader.token).catch((error: unknown) => {
      // 关闭期间不吞真实的 payload/发送错误；发生后由 owner 统一回收。
      reader.stopped = true;
      reader.failed = true;
      this.ports.failure(error);
    }).finally(() => { reader.running = false; });
  }

  async close(): Promise<void> {
    this.closed = true;
    await this.stopReader(this.current);
  }

  private async stopReader(reader: Reader | null): Promise<void> {
    if (reader === null) return;
    reader.stopped = true;
    // notify 与实际 waitAsync 的登记都同步发起，同一 Worker 内不存在交错窗口。
    this.notify(reader.signal);
    try { await reader.completion; }
    finally { if (this.current === reader) this.current = null; }
  }

  private async run(reader: Reader, token: number): Promise<void> {
    let sent = 0;
    while (!reader.stopped) {
      const observed = Atomics.load(reader.signal, 0);
      const value = this.ports.take(token);
      if (value !== null) {
        if (this.ports.deliver(value, reader.generation) === "stop") return;
        // 仅让出控制任务；不在这里划分市场批次或等待 UI ACK。
        if (++sent === 64) { sent = 0; await this.ports.yieldTask(); }
      } else {
        sent = 0;
        if (Atomics.load(reader.signal, 0) === observed && !reader.stopped) await this.wait(reader.signal, observed);
      }
    }
  }
}

/** 控制前哨只暴露同一共享 Memory 中经过校验的一个对齐 i32 字。 */
export function sharedControlWord(memory: WebAssembly.Memory, address: number): Int32Array {
  const buffer: unknown = memory.buffer;
  if (!(buffer instanceof SharedArrayBuffer) || !Number.isSafeInteger(address) || address <= 0 || address % 4 !== 0 || address + 4 > buffer.byteLength) {
    throw new Error("共享控制地址未对齐或超出内存边界");
  }
  return new Int32Array(buffer, address, 1);
}

export function requireSharedControlWord(value: unknown): Int32Array {
  if (!(value instanceof Int32Array) || !(value.buffer instanceof SharedArrayBuffer) || value.length !== 1 || value.byteOffset <= 0) throw new Error("共享控制回复必须是一个对齐 i32 字");
  return value;
}

export function markSharedControl(word: Int32Array): void { Atomics.add(word, 0, 1); }
export function retractSharedControl(word: Int32Array): void { Atomics.sub(word, 0, 1); }
