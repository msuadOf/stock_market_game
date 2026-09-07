interface TerminableWorker {
  postMessage(message: unknown): void;
  terminate(): void;
}

/** 把可恢复的暂停与不可恢复的线程池销毁明确分开。 */
export function createWorkerLifecycle(worker: TerminableWorker): {
  pause(): void;
  dispose(): void;
  isDisposed(): boolean;
} {
  let disposed = false;

  function assertActive(): void {
    if (disposed) throw new Error("WASM Worker 已经销毁，不能继续使用");
  }

  return {
    pause() {
      assertActive();
      worker.postMessage({ type: "stop" });
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      worker.terminate();
    },
    isDisposed() {
      return disposed;
    },
  };
}

export function routeWorkerFailure(
  initialized: boolean,
  message: string,
  handlers: { initialization(message: string): void; runtime(message: string): void },
): void {
  if (initialized) handlers.runtime(message);
  else handlers.initialization(message);
}
