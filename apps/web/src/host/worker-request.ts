import { CompanyWriteRejectedError, parseCompanyWriteFailure } from "./company-write.ts";
import { CompanyReadRejectedError, parseCompanyReadFailure } from "./company-read-failure.ts";

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
  readonly request: WorkerRequest;
  readonly successType: string;
  readonly timeoutMs: number;
  readonly resolve: (response: WorkerResponse) => void;
  readonly reject: (error: Error) => void;
  listener: ((event: MessageEvent) => void) | null;
  timeout: ReturnType<typeof setTimeout> | null;
  settled: boolean;
};

/** 每个 WorkerHost 独占请求序号与逐请求资源，generation 仍由宿主提供。 */
export class WorkerRequestScope {
  private readonly port: WorkerRequestPort;
  private sequence = 0;
  private readonly pending = new Map<number, PendingRequest>();
  private readonly queued: PendingRequest[] = [];
  private closed: Error | null = null;
  private poisoned: Error | null = null;
  private closingType: string | null = null;
  private activeSaveRequestId: number | null = null;
  private drainingQueue = false;

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
    this.cancelResources(error);
  }

  /** 换代时只取消旧代资源，保留新代请求及其 FIFO 顺序。 */
  cancelGeneration(generation: number, error: Error): void {
    if (this.closed !== null) return;
    const activeSave = this.activeSaveRequestId === null ? undefined : this.pending.get(this.activeSaveRequestId);
    if (activeSave?.request.generation === generation) this.activeSaveRequestId = null;
    const retained: PendingRequest[] = [];
    for (const resource of this.queued) {
      if (resource.request.generation === generation) {
        this.finish(resource);
        resource.reject(error);
      } else retained.push(resource);
    }
    this.queued.splice(0, this.queued.length, ...retained);
    for (const resource of [...this.pending.values()]) {
      if (resource.request.generation !== generation) continue;
      this.finish(resource);
      resource.reject(error);
    }
    this.drainQueued();
  }

  beginClosing(allowedType: string, error: Error): void {
    if (this.closed !== null || this.closingType !== null) throw new Error("Worker 请求 scope 已关闭或正在关闭");
    this.cancelResources(error);
    this.closingType = allowedType;
  }

  request(request: WorkerRequest, successType: string, timeoutMs = 10_000): Promise<WorkerResponse> {
    return new Promise((resolve, reject) => {
      if (this.closed !== null) { reject(this.closed); return; }
      if (this.closingType !== null && request.type !== this.closingType) { reject(new Error("Worker 会话正在关闭，拒绝新业务请求")); return; }
      if (this.poisoned !== null && request.type !== this.closingType) { reject(this.poisoned); return; }
      const resource: PendingRequest = {
        request, successType, timeoutMs, resolve, reject, listener: null, timeout: null, settled: false,
      };
      this.pending.set(request.requestId, resource);
      if (this.activeSaveRequestId !== null) {
        this.queued.push(resource);
        return;
      }
      this.send(resource);
      this.drainQueued();
    });
  }

  private cancelResources(error: Error): void {
    this.activeSaveRequestId = null;
    this.queued.length = 0;
    for (const resource of [...this.pending.values()]) {
      this.finish(resource);
      resource.reject(error);
    }
  }

  private finish(resource: PendingRequest): void {
    if (resource.settled) return;
    resource.settled = true;
    if (resource.timeout !== null) clearTimeout(resource.timeout);
    if (resource.listener !== null) this.port.removeEventListener("message", resource.listener);
    resource.timeout = null;
    resource.listener = null;
    this.pending.delete(resource.request.requestId);
  }

  private send(resource: PendingRequest): boolean {
    if (resource.settled) return false;
    const { request, successType, timeoutMs } = resource;
    if (request.type === "save") this.activeSaveRequestId = request.requestId;
    resource.listener = (event: MessageEvent) => {
      const response = event.data as WorkerResponse;
      if (response.requestId !== request.requestId || response.generation !== request.generation) return;
      if (response.type !== successType && response.type !== "operationError") return;
      this.finish(resource);
      if (response.type === successType) resource.resolve(response);
      else if (request.type === "companyReadQuery" && Object.hasOwn(response, "failure")) {
        let failure: unknown;
        try {
          failure = response.failure;
          resource.reject(new CompanyReadRejectedError(parseCompanyReadFailure(failure), response));
        } catch (decoderError) {
          resource.reject(new AggregateError([decoderError], "Worker 公司只读查询 typed failure 响应协议无效", { cause: { response, failure, decoderError } }));
        }
      } else if (["proposePlayerCompanyAction", "enqueueReportCorrection", "cancelReportCorrection"].includes(request.type)) {
        let failure: unknown;
        try {
          if (Object.hasOwn(response, "failure")) {
            failure = response.failure;
            const parsed = parseCompanyWriteFailure(failure);
            const family = request.type === "proposePlayerCompanyAction" ? "PlayerProposal" : "ReportCorrection";
            if (parsed.kind !== family) throw new Error("Worker 公司写命令 failure family 与请求不符");
            resource.reject(new CompanyWriteRejectedError(parsed));
          } else {
            if (typeof response.message !== "string" || response.message.trim().length === 0) throw new Error("Worker operationError.message 必须是非空字符串");
            resource.reject(new Error(response.message, { cause: response }));
          }
        } catch (decoderError) {
          resource.reject(new AggregateError([decoderError], "Worker 公司写命令响应协议无效", { cause: { response, failure, decoderError } }));
        }
      } else resource.reject(new Error(String(response.message ?? "Worker 操作失败")));
      if (this.activeSaveRequestId === request.requestId) this.activeSaveRequestId = null;
      this.drainQueued();
    };
    this.port.addEventListener("message", resource.listener);
    resource.timeout = setTimeout(() => {
      const error = new Error(`Worker ${request.type} 操作超时（${timeoutMs}ms）`);
      this.finish(resource);
      resource.reject(error);
      if (this.activeSaveRequestId === request.requestId) {
        this.activeSaveRequestId = null;
        this.poisoned = error;
        this.cancelResources(error);
      } else this.drainQueued();
    }, timeoutMs);
    try {
      this.port.postMessage(request);
      return true;
    } catch (error) {
      this.finish(resource);
      if (this.activeSaveRequestId === request.requestId) this.activeSaveRequestId = null;
      resource.reject(error instanceof Error ? error : new Error("Worker postMessage 失败", { cause: error }));
      return false;
    }
  }

  private drainQueued(): void {
    if (this.drainingQueue || this.activeSaveRequestId !== null) return;
    this.drainingQueue = true;
    try {
      while (this.activeSaveRequestId === null && this.queued.length > 0) {
        const resource = this.queued.shift()!;
        if (resource.settled) continue;
        if (this.closed !== null || this.poisoned !== null || this.closingType !== null && resource.request.type !== this.closingType) {
          this.finish(resource);
          resource.reject(this.closed ?? this.poisoned ?? new Error("Worker 会话正在关闭，拒绝排队业务请求"));
          continue;
        }
        this.send(resource);
      }
    } finally {
      this.drainingQueue = false;
    }
  }
}

/** 独立请求兼容入口；WorkerHost 的命令通过其唯一 scope 发送。 */
export function requestWorker(port: WorkerRequestPort, request: WorkerRequest, successType: string, timeoutMs = 10_000): Promise<WorkerResponse> {
  return new WorkerRequestScope(port).request(request, successType, timeoutMs);
}
