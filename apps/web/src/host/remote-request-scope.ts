import { remoteJson } from "./remote-request.ts";

export class RemoteRequestScope {
  private readonly pending = new Set<(error: Error) => void>();

  interrupt(message: string): void {
    for (const reject of [...this.pending]) reject(new Error(message));
  }

  request(fetchFn: typeof fetch, url: string, init: RequestInit): Promise<unknown> {
    return new Promise((resolve, reject) => {
      const controller = new AbortController();
      const complete = (error: Error | null, value?: unknown) => {
        if (!this.pending.delete(cancel)) return;
        clearTimeout(timeout);
        if (error === null) resolve(value);
        else { controller.abort(); reject(error); }
      };
      const cancel = (error: Error) => complete(error);
      const timeout = setTimeout(() => cancel(new Error("远程请求确认超时（5000ms），结果未知；请重新同步权威状态后核对")), 5000);
      this.pending.add(cancel);
      remoteJson(fetchFn, url, { ...init, signal: controller.signal }).then(
        (value) => complete(null, value),
        (error: unknown) => complete(error instanceof Error ? error : new Error(String(error))),
      );
    });
  }
}
