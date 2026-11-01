export type InitializationPhase = "package" | "engine" | "restore";
const phases: readonly InitializationPhase[] = ["package", "engine", "restore"];
const labels: Record<InitializationPhase, string> = { package: "正在校验行情引擎", engine: "正在初始化行情引擎", restore: "正在建立市场会话" };
type Timers = { now(): number; set(callback: () => void, milliseconds: number): number; clear(id: number): void };
const timers: Timers = { now: () => performance.now(), set: (callback, milliseconds) => window.setTimeout(callback, milliseconds), clear: (id) => window.clearTimeout(id) };
/** 每个真实阶段单独限时，总期限不随进度重置。 */
export class InitializationDeadline {
  private phase: InitializationPhase | undefined;
  private disposed = false;
  private stageTimer: number | undefined;
  private totalTimer: number;
  private readonly totalAt: number;
  private stageAt: number | undefined;
  private readonly fail: (phase: string) => void;
  private readonly progress: (label: string) => void;
  private readonly clock: Timers;
  constructor(fail: (phase: string) => void, progress: (label: string) => void, clock: Timers = timers) {
    this.fail = fail; this.progress = progress; this.clock = clock;
    this.totalAt = clock.now() + 120_000;
    this.totalTimer = clock.set(() => fail("总期限"), 120_000);
  }
  advance(phase: InitializationPhase): void {
    if (this.disposed) throw new Error("初始化 deadline 已关闭");
    const now = this.checkActive();
    const index = phases.indexOf(phase);
    if (index < 0 || index !== (this.phase === undefined ? 0 : phases.indexOf(this.phase) + 1)) throw new Error("初始化阶段顺序无效");
    if (this.stageTimer !== undefined) this.clock.clear(this.stageTimer);
    this.phase = phase;
    this.stageAt = now + 30_000;
    this.progress(labels[phase]);
    this.stageTimer = this.clock.set(() => this.fail(labels[phase]), 30_000);
  }
  private checkActive(): number {
    if (this.disposed) throw new Error("初始化 deadline 已关闭");
    const now = this.clock.now();
    if (now >= this.totalAt || (this.stageAt !== undefined && now >= this.stageAt)) {
      const expired = now >= this.totalAt ? "总期限" : labels[this.phase!];
      this.fail(expired);
      throw new Error(`初始化在${expired}超时`);
    }
    return now;
  }
  complete(): boolean {
    if (this.disposed) return false;
    const now = this.clock.now();
    if (now >= this.totalAt || (this.stageAt !== undefined && now >= this.stageAt)) {
      this.fail(now >= this.totalAt ? "总期限" : labels[this.phase!]);
      return false;
    }
    this.dispose();
    return true;
  }
  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.clock.clear(this.totalTimer);
    if (this.stageTimer !== undefined) this.clock.clear(this.stageTimer);
  }
}
