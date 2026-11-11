export interface DecodedTextReader {
  read(): Promise<ReadableStreamReadResult<string>>;
  cancel(reason?: unknown): Promise<void>;
  releaseLock(): void;
}

/** 在 EOF 外退出时等待取消；读取期间可由 AbortSignal 打断并保留所有错误。 */
export async function* readDecodedText(reader: DecodedTextReader, signal?: AbortSignal): AsyncGenerator<string> {
  let eof = false, failed = false, primary: unknown;
  let cancellation: Promise<{ ok: true } | { ok: false; error: unknown }> | undefined;
  function cancel(reason?: unknown) {
    cancellation ??= Promise.resolve().then(() => reader.cancel(reason)).then(
      () => ({ ok: true as const }), error => ({ ok: false as const, error }),
    );
    return cancellation;
  }
  let rejectAbort: ((reason: unknown) => void) | undefined;
  const aborted = new Promise<never>((_resolve, reject) => { rejectAbort = reject; });
  const onAbort = () => { void cancel(signal?.reason); rejectAbort?.(signal?.reason); };
  // 即使在两次 read 之间取消，rejection 也必须有消费者。
  void aborted.catch(() => {});
  signal?.addEventListener("abort", onAbort, { once: true });
  async function cleanup(): Promise<void> {
  signal?.removeEventListener("abort", onAbort);
  const errors: unknown[] = [];
  // 下游在 yield 暂停时响应 abort 并 return，也必须把取消列为清理失败的原始原因。
  if (!eof && !failed && signal?.aborted) { failed = true; primary = signal.reason; }
  if (!eof) {
    const result = await cancel(failed ? primary : undefined);
    if (!result.ok) errors.push(result.error);
  }
  try { reader.releaseLock(); } catch (error) { errors.push(error); }
  if (errors.length > 0) {
    if (failed) throw new AggregateError([primary, ...errors], "读取存档失败且 reader 清理失败");
    throw new AggregateError(errors, "存档 reader 清理失败");
  }
  if (failed) throw primary;
  }
  try {
    signal?.throwIfAborted();
    for (;;) {
      const result = await Promise.race([reader.read(), aborted]);
      signal?.throwIfAborted();
      if (result.done) { eof = true; break; }
      yield result.value;
    }
  } catch (error) { failed = true; primary = error; }
  finally { await cleanup(); }
}
