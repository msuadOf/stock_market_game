export interface WorkerResponse {
  type: string;
  requestId?: number;
  message?: unknown;
  [key: string]: unknown;
}

export interface WorkerRequestPort {
  addEventListener(type: "message", listener: (event: MessageEvent) => void): void;
  removeEventListener(type: "message", listener: (event: MessageEvent) => void): void;
  postMessage(message: unknown): void;
}

/** 发送带关联 id 的 Worker 请求；并发请求不会互相误收响应。 */
export function requestWorker(
  port: WorkerRequestPort,
  request: { type: string; requestId: number; [key: string]: unknown },
  successType: string,
  timeoutMs = 10_000,
): Promise<WorkerResponse> {
  return new Promise((resolve, reject) => {
    const cleanup = () => {
      clearTimeout(timeout);
      port.removeEventListener("message", handler);
    };
    const handler = (event: MessageEvent) => {
      const response = event.data as WorkerResponse;
      if (response.requestId !== request.requestId) return;
      if (response.type === successType) {
        cleanup();
        resolve(response);
      } else if (response.type === "operationError") {
        cleanup();
        reject(new Error(String(response.message ?? "Worker 操作失败")));
      }
    };
    const timeout = setTimeout(() => {
      port.removeEventListener("message", handler);
      reject(new Error(`Worker ${request.type} 操作超时（${timeoutMs}ms）`));
    }, timeoutMs);
    port.addEventListener("message", handler);
    port.postMessage(request);
  });
}
