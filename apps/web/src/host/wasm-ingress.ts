export class WasmIngressReceiver {
  private generation = 0;
  private token: number | null = null;
  private closed = false;
  private readonly enqueue: (token: number, intent: unknown) => void;

  constructor(enqueue: (token: number, intent: unknown) => void) {
    this.enqueue = enqueue;
  }

  bind(generation: number, token: number): void {
    if (this.closed) throw new Error("Browser ingress 已关闭");
    if (!Number.isSafeInteger(generation) || generation <= this.generation) throw new Error("Browser ingress generation 必须严格推进");
    if (!Number.isSafeInteger(token) || token < 1) throw new Error("Browser ingress token 必须是正安全整数");
    this.generation = generation;
    this.token = token;
  }

  submit(generation: unknown, intent: unknown): void {
    if (this.closed) throw new Error("Browser ingress 已关闭");
    if (this.token === null) throw new Error("Browser ingress 尚未就绪");
    if (generation !== this.generation) throw new Error("Browser ingress 请求属于已过期 generation");
    this.enqueue(this.token, intent);
  }

  close(): void {
    this.closed = true;
    this.token = null;
  }
}
