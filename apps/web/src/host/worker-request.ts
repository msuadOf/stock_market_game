export interface WorkerResponse {
  type: string;
  requestId?: number;
  generation?: number;
  message?: unknown;
  [key: string]: unknown;
}

export interface WorkerRequestPort {
  addEventListener(type: "message", listener: (event: MessageEvent) => void): void;
  removeEventListener(type: "message", listener: (event: MessageEvent) => void): void;
  postMessage(message: unknown): void;
}

type WorkerRequest = { type: string; requestId: number; generation: number; [key: string]: unknown };

type PendingRequest = {
  readonly listener: (event: MessageEvent) => void;
  readonly timeout: ReturnType<typeof setTimeout>;
  readonly resolve: (response: WorkerResponse) => void;
  readonly reject: (error: Error) => void;
  settled: boolean;
};

/** 每个 WorkerHost 独占请求序号与逐请求资源，generation 仍由宿主提供。 */
export class WorkerRequestScope {
  private readonly port: WorkerRequestPort;
  private sequence = 0;
  private readonly pending = new Map<number, PendingRequest>();
  private closed: Error | null = null;

  constructor(port: WorkerRequestPort) {
    this.port = port;
  }

  nextRequestId(): number {
    return ++this.sequence;
  }

  pendingCount(): number {
    return this.pending.size;
  }

  close(error: Error): void {
    this.closed = error;
    for (const resource of this.pending.values()) {
      resource.settled = true;
      clearTimeout(resource.timeout);
      this.port.removeEventListener("message", resource.listener);
      resource.reject(error);
    }
    this.pending.clear();
  }

  request(request: WorkerRequest, successType: string, timeoutMs = 10_000): Promise<WorkerResponse> {
    return new Promise((resolve, reject) => {
      if (this.closed !== null) { reject(this.closed); return; }
      const cleanup = () => {
        if (resource.settled) return;
        resource.settled = true;
        clearTimeout(resource.timeout);
        this.port.removeEventListener("message", resource.listener);
        this.pending.delete(request.requestId);
      };
      const handler = (event: MessageEvent) => {
        const response = event.data as WorkerResponse;
        if (response.requestId !== request.requestId) return;
        if (response.generation !== request.generation) return;
        if (response.type === successType) {
          cleanup();
          resource.resolve(response);
        } else if (response.type === "operationError") {
          cleanup();
          resource.reject(new Error(String(response.message ?? "Worker 操作失败")));
        }
      };
      const timeout = setTimeout(() => {
        cleanup();
        resource.reject(new Error(`Worker ${request.type} 操作超时（${timeoutMs}ms）`));
      }, timeoutMs);
      const resource: PendingRequest = { listener: handler, timeout, resolve, reject, settled: false };
      this.pending.set(request.requestId, resource);
      this.port.addEventListener("message", handler);
      try {
        this.port.postMessage(request);
      } catch (error) {
        cleanup();
        reject(error);
      }
    });
  }
}

/** 独立请求兼容入口；WorkerHost 的命令通过其唯一 scope 发送。 */
export function requestWorker(port: WorkerRequestPort, request: WorkerRequest, successType: string, timeoutMs = 10_000): Promise<WorkerResponse> {
  return new WorkerRequestScope(port).request(request, successType, timeoutMs);
}
