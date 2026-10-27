/** Worker 恢复重启的可失效令牌。 */
export class WasmRestartGate {
  private epoch = 0;
  begin(): number { return ++this.epoch; }
  invalidate(): void { this.epoch++; }
  schedule(token: number, isCurrent: () => boolean, restart: () => void): void {
    queueMicrotask(() => { if (token === this.epoch && isCurrent()) restart(); });
  }
}
