import { MessageChannel as NodeMessageChannel } from "node:worker_threads";

/** 测试中保留真实 Node MessagePort；业务请求只经对端 postMessage 到达监听器。 */
export class WorkerTransportFixture {
  readonly channels: NodeMessageChannel[] = [];
  private readonly internalChannels: NodeMessageChannel[] = [];
  private readonly originalChannel = Object.getOwnPropertyDescriptor(globalThis, "MessageChannel");

  constructor() {
    const channels = this.internalChannels;
    Object.defineProperty(globalThis, "MessageChannel", { configurable: true, writable: true, value: class extends NodeMessageChannel {
      constructor() { super(); channels.push(this); }
    } });
  }

  createChannel(): NodeMessageChannel {
    const channel = new NodeMessageChannel();
    this.channels.push(channel);
    return channel;
  }

  /** 从绑定端口的对端发送业务消息，遵循 MessagePort 的异步派发语义。 */
  dispatch(port: MessagePort, message: unknown): void {
    port.postMessage(message);
  }

  /** 仅边界恶意输入用原生 EventTarget 注入，保持 postMessage clone 前时序可测。 */
  dispatchRaw(port: MessagePort, message: unknown): void {
    port.dispatchEvent(new MessageEvent("message", { data: message }));
  }

  bindEngineTransport(
    listener: (event: MessageEvent<unknown>) => void,
    instanceId: number,
  ): { readonly business: MessagePort; readonly control: MessagePort } {
    const business = this.createChannel();
    const control = this.createChannel();
    listener(new MessageEvent("message", { data: {
      type: "bindEngineTransport", instanceId,
      dataPort: business.port2, controlPort: control.port2,
    } }));
    return { business: business.port1 as unknown as MessagePort, control: control.port1 as unknown as MessagePort };
  }

  bindIngressTransport(
    listener: (event: MessageEvent<unknown>) => void,
    instanceId: number,
    substrate: { readonly module: WebAssembly.Module; readonly memory: WebAssembly.Memory; readonly layout: unknown; readonly diagnostics: boolean; readonly primary?: { readonly workerId: number; readonly address: number } },
  ): MessagePort {
    const business = this.createChannel();
    listener(new MessageEvent("message", { data: {
      type: "bindIngressTransport", instanceId, dataPort: business.port2,
      ...substrate,
    } }));
    return business.port1 as unknown as MessagePort;
  }

  close(): void {
    for (const channel of [...this.channels, ...this.internalChannels]) {
      channel.port1.close();
      channel.port2.close();
    }
    this.channels.length = 0;
    this.internalChannels.length = 0;
    if (this.originalChannel === undefined) Reflect.deleteProperty(globalThis, "MessageChannel");
    else Object.defineProperty(globalThis, "MessageChannel", this.originalChannel);
  }
}
