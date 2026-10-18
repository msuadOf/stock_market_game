import type { EngineHost } from "../host/engine-host.ts";
import type { HostFailure, HostUpdate } from "../host/host-update.ts";

interface Ports {
  hostRef: { current: EngineHost | null };
  runningRef: { current: boolean };
  onUpdate(update: HostUpdate): void | boolean;
  onFatal(failure: HostFailure): void;
  onRunning(running: boolean): void;
  onSpeed(speed: number): void;
  onError(message: string): void;
}

export class SessionControlCommands {
  private readonly ports: Ports;
  private tail = Promise.resolve();

  constructor(ports: Ports) { this.ports = ports; }

  private enqueue(label: string, apply: (host: EngineHost) => Promise<void>): Promise<void> {
    const host = this.ports.hostRef.current;
    const pending = this.tail.then(async () => {
      if (host === null) throw new Error("游戏引擎尚未就绪");
      if (host !== this.ports.hostRef.current) return;
      await apply(host);
    }).catch((failure) => {
      if (host === this.ports.hostRef.current) this.ports.onError(`${label}失败：${failure instanceof Error ? failure.message : String(failure)}；请反馈此错误或返回启动页重试。`);
    });
    this.tail = pending;
    return pending;
  }

  toggleRunning(): Promise<void> {
    return this.enqueue("切换模拟状态", async (host) => {
      const nextRunning = !this.ports.runningRef.current;
      if (nextRunning) await host.start(
        (update) => host === this.ports.hostRef.current ? this.ports.onUpdate(update) : false,
        (failure) => { if (host === this.ports.hostRef.current) this.ports.onFatal(failure); },
      );
      else await host.stop();
      if (host === this.ports.hostRef.current) this.ports.onRunning(nextRunning);
    });
  }

  setSpeed(speed: number): Promise<void> {
    return this.enqueue("修改模拟倍速", async (host) => {
      await host.setSpeed(speed);
      if (host === this.ports.hostRef.current) this.ports.onSpeed(speed);
    });
  }

  syncVisibility(hidden: boolean): Promise<void> {
    return this.enqueue("同步后台暂停状态", async (host) => {
      if (hidden) await host.stop();
      else if (this.ports.runningRef.current) await host.start(
        (update) => host === this.ports.hostRef.current ? this.ports.onUpdate(update) : false,
        (failure) => { if (host === this.ports.hostRef.current) this.ports.onFatal(failure); },
      );
    });
  }
}
