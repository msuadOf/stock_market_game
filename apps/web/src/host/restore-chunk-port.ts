const CHUNK_BYTES = 64 * 1024;

/** 超时只表示停止等待；调用方必须保留源占用，不能据此启动新实例。 */
export class RestoreSourceCleanupTimeoutError extends AggregateError {
  readonly sourceMayStillBeRunning = true;
  constructor(errors: unknown[]) { super(errors, "分块源清理超时，源可能仍在运行，不能视为已释放"); this.name = "RestoreSourceCleanupTimeoutError"; }
}

/** 超时后 consumer 可能仍运行；调用方不得认定资源已释放或启动新实例。 */
export class RestoreConsumerCleanupTimeoutError extends AggregateError {
  readonly consumerMayStillBeRunning = true;
  constructor(errors: unknown[]) { super(errors, "分块 consumer 清理超时，状态未知，不能视为已释放"); this.name = "RestoreConsumerCleanupTimeoutError"; }
}

type Wire = { type: string; generation: number; seq?: number; length?: number; bytes?: Uint8Array; reason?: string };

function checkIdentity(generation: number, length: number): void {
  if (!Number.isSafeInteger(generation) || generation < 1 || !Number.isSafeInteger(length) || length < 0) throw new Error("分块传输 generation 或声明长度无效");
}

function message(value: unknown, generation: number): Wire | null {
  if (value === null || typeof value !== "object") throw new Error("分块传输消息无效");
  const wire = value as Wire;
  if (!Number.isSafeInteger(wire.generation) || wire.generation < 1) throw new Error("分块传输 generation 无效");
  if (wire.generation !== generation) return null;
  return wire;
}

function failure(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** 仅传输未验证字节；调用方仍须独立完成存档验证。端口由本函数独占并关闭。 */
export async function sendRestoreChunks(port: MessagePort, generation: number, length: number, source: AsyncIterable<Uint8Array>, signal?: AbortSignal, cancelSource?: (reason: Error) => void | Promise<void>): Promise<void> {
  try { checkIdentity(generation, length); } catch (error) { port.close(); throw error; }
  let rejectPending: ((error: Error) => void) | undefined;
  let resolvePending: (() => void) | undefined;
  let expectedType = "";
  let expectedSeq = 0;
  let stopped: Error | undefined;
  let interrupt!: (error: Error) => void;
  const interrupted = new Promise<never>((_, reject) => { interrupt = reject; });
  void interrupted.catch(() => {});
  const fail = (error: Error, notify = true) => {
    if (stopped) return;
    stopped = error;
    interrupt(error);
    if (notify) { try { port.postMessage({ type: "error", generation, reason: error.message }); } catch { /* 端口已不可用，原错误仍会返回调用方。 */ } }
    rejectPending?.(error);
  };
  const onMessage = (event: MessageEvent) => {
    try {
      const item = message(event.data, generation);
      if (!item) return;
      if (item.type === "error") { fail(new Error(`对端分块传输失败：${String(item.reason)}`), false); return; }
      if (!resolvePending || item.type !== expectedType || item.seq !== expectedSeq) throw new Error("分块确认消息或序号无效");
      const resolve = resolvePending; resolvePending = undefined; rejectPending = undefined; resolve();
    } catch (error) { fail(new Error(failure(error))); }
  };
  const onAbort = () => fail(new Error("分块传输已取消"));
  const onError = () => fail(new Error("分块端口消息无法反序列化"));
  const onClose = () => fail(new Error("分块端口意外关闭"), false);
  port.addEventListener("message", onMessage); port.addEventListener("messageerror", onError); port.addEventListener("close", onClose); signal?.addEventListener("abort", onAbort); port.start();
  const exchange = async (wire: Wire, response: string) => {
    if (stopped) throw stopped;
    await new Promise<void>((resolve, reject) => {
      resolvePending = resolve; rejectPending = reject; expectedType = response; expectedSeq = wire.seq!;
      try { port.postMessage(wire); } catch (error) { fail(new Error(failure(error)), false); }
    });
  };
  let iterator: AsyncIterator<Uint8Array> | undefined;
  let ended = false;
  let operationError: Error | undefined;
  const cleanupErrors: unknown[] = [];
  let cleanupTimedOut = false;
  try {
    if (signal?.aborted) onAbort();
    if (stopped) throw stopped;
    iterator = source[Symbol.asyncIterator]();
    let seq = 0, total = 0;
    while (true) {
      const result = await Promise.race([iterator.next(), interrupted]);
      if (result.done) { ended = true; break; }
      if (!(result.value instanceof Uint8Array) || result.value.byteLength === 0) throw new Error("分块源必须产生非空 Uint8Array");
      if (result.value.byteLength > length - total) throw new Error("分块源超出声明长度");
      total += result.value.byteLength;
      for (let offset = 0; offset < result.value.byteLength; offset += CHUNK_BYTES) {
        await exchange({ type: "chunk", generation, seq, bytes: result.value.slice(offset, offset + CHUNK_BYTES) }, "ack");
        seq++;
      }
    }
    if (total !== length) throw new Error("分块源短于声明长度");
    await exchange({ type: "eof", generation, seq, length }, "done");
  } catch (error) {
    fail(error instanceof Error ? error : new Error(failure(error)));
    operationError = stopped;
  } finally {
    signal?.removeEventListener("abort", onAbort); port.removeEventListener("message", onMessage); port.removeEventListener("messageerror", onError); port.removeEventListener("close", onClose); port.close();
    if (!ended) {
      // 等待中的源 I/O 必须响应 cancelSource；不合作的源不能阻塞调用方。
      const cleanup = async () => {
        if (stopped && cancelSource) {
          try { await cancelSource(stopped); } catch (error) { cleanupErrors.push(error); }
        }
        if (iterator?.return) {
          try { await iterator.return(); } catch (error) { cleanupErrors.push(error); }
        }
      };
      let timer: ReturnType<typeof setTimeout> | undefined;
      const deadline = new Promise<void>((resolve) => { timer = setTimeout(() => { cleanupTimedOut = true; cleanupErrors.push(new Error("分块源清理超时，源可能仍在运行")); resolve(); }, 1000); });
      await Promise.race([cleanup(), deadline]);
      if (timer !== undefined) clearTimeout(timer);
    }
  }
  if (cleanupTimedOut) throw new RestoreSourceCleanupTimeoutError([operationError, ...cleanupErrors]);
  if (cleanupErrors.length) throw new AggregateError([operationError, ...cleanupErrors], "分块传输及源清理失败");
  if (operationError) throw operationError;
}

/** consumer 接收的是未验证字节；完成传输不代表存档可信。端口由本函数独占并关闭。 */
export function receiveRestoreChunks(port: MessagePort, generation: number, length: number, consumer: (bytes: Uint8Array, isActive: () => boolean) => Promise<void>, signal?: AbortSignal, onComplete?: (isActive: () => boolean) => Promise<void>): Promise<void> {
  try { checkIdentity(generation, length); } catch (error) { port.close(); throw error; }
  return new Promise<void>((resolve, reject) => {
    let seq = 0, received = 0, busy = false, settled = false;
    let inFlight: Promise<void> | undefined;
    const cleanup = () => { signal?.removeEventListener("abort", onAbort); port.removeEventListener("message", onMessage); port.removeEventListener("messageerror", onError); port.removeEventListener("close", onClose); port.close(); };
    const fail = (error: Error, notify = true) => {
      if (settled) return;
      settled = true;
      if (notify) { try { port.postMessage({ type: "error", generation, reason: error.message }); } catch { /* 端口已不可用，原错误仍会返回调用方。 */ } }
      cleanup();
      const running = inFlight;
      if (running === undefined) { reject(error); return; }
      void (async () => {
        let timer: ReturnType<typeof setTimeout> | undefined;
        const timeout = new Promise<"timeout">((resolve) => { timer = setTimeout(() => resolve("timeout"), 1000); });
        const outcome = await Promise.race([running.then(() => ({ kind: "done" as const }), (cause: unknown) => ({ kind: "failed" as const, cause })), timeout]);
        if (timer !== undefined) clearTimeout(timer);
        if (outcome === "timeout") reject(new RestoreConsumerCleanupTimeoutError([error]));
        else if (outcome.kind === "failed" && outcome.cause !== error) reject(new AggregateError([error, outcome.cause], "分块传输及 consumer 均失败"));
        else reject(error);
      })();
    };
    const onAbort = () => fail(new Error("分块传输已取消"));
    const onError = () => fail(new Error("分块端口消息无法反序列化"));
    const onClose = () => fail(new Error("分块端口意外关闭"), false);
    const onMessage = (event: MessageEvent) => {
      void (async () => {
        try {
          const item = message(event.data, generation);
          if (!item || settled) return;
          if (item.type === "error") { fail(new Error(`对端分块传输失败：${String(item.reason)}`), false); return; }
          if (busy || item.seq !== seq) throw new Error("分块序号或 credit 无效");
          if (item.type === "eof") {
            if (item.length !== length || received !== length) throw new Error("分块 EOF 长度不符");
            if (onComplete !== undefined) {
              busy = true;
              inFlight = Promise.resolve().then(() => onComplete(() => !settled));
              await inFlight;
              inFlight = undefined;
              if (settled) return;
            }
            port.postMessage({ type: "done", generation, seq }); settled = true; cleanup(); resolve(); return;
          }
          if (item.type !== "chunk" || !(item.bytes instanceof Uint8Array) || item.bytes.byteLength < 1 || item.bytes.byteLength > CHUNK_BYTES || received + item.bytes.byteLength > length) throw new Error("分块内容或声明长度无效");
          const chunkLength = item.bytes.byteLength;
          busy = true;
          const chunk = item.bytes;
          inFlight = Promise.resolve().then(() => consumer(chunk, () => !settled));
          await inFlight;
          inFlight = undefined;
          if (settled) return;
          received += chunkLength; port.postMessage({ type: "ack", generation, seq }); seq++; busy = false;
        } catch (error) { fail(error instanceof Error ? error : new Error(failure(error))); }
      })();
    };
    port.addEventListener("message", onMessage); port.addEventListener("messageerror", onError); port.addEventListener("close", onClose); signal?.addEventListener("abort", onAbort); port.start();
    if (signal?.aborted) onAbort();
  });
}
