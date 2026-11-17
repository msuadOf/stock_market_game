import { attachRemoteHost, type RemoteMarketHost } from "./remote-host.ts";
import { parseRemoteMarketContext } from "./remote-market-context.ts";
import type { HostUpdate, HostFailure } from "./host-update.ts";

/** 验证入口不注册全局对象；仅由DEV/e2e页面动态导入，backend还必须明确提供验证能力。 */
export function assertRemoteContractEnvironment(dev: boolean, mode: string): void {
  if (!dev || mode !== "e2e") throw new Error("Remote产品合同桥仅允许DEV/e2e构建");
}
export function isContractPublication(value: Readonly<Record<string, unknown>>): boolean {
  return ["PublisherFrame", "PublicationFailure", "ObserverTerminal"].some(key => Object.hasOwn(value, key));
}
export type RemoteContractEvent = Readonly<Record<string, unknown>>;
export interface RemoteAuthorityContractBridge {
  readonly host: RemoteMarketHost;
  readonly socket: WebSocket;
  readonly events: readonly RemoteContractEvent[];
  setAccept(accept: boolean): void;
  setConsumer(consumer: ((update: HostUpdate) => boolean | void) | null): void;
  assertHealthy(): void;
  retainStalePacket(): () => { readonly wireKind: string; readonly oldPacketRejected: true; readonly deliveredBefore: number; readonly deliveredAfter: number; readonly acceptedBefore: number; readonly acceptedAfter: number };
  dispose(): Promise<void>;
}
export async function createBridge(baseUrl: string, token: string, market: unknown, options: {
  readonly onNetwork?: (raw: string) => void;
  readonly onEvent?: (event: RemoteContractEvent) => void;
  readonly onFailure?: (failure: HostFailure) => void;
  readonly accept?: boolean;
} = {}): Promise<RemoteAuthorityContractBridge> {
  assertRemoteContractEnvironment(import.meta.env?.DEV === true, import.meta.env?.MODE ?? "");
  const response = await fetch(new URL("/api/host-parity/authority-contract/setup", baseUrl), {headers:{authorization:`Bearer ${token}`}});
  if (!response.ok) throw new Error(`Server未提供验证能力：HTTP ${response.status}`);
  const capability: unknown = await response.json();
  if (capability === null || typeof capability !== "object" || !("verification" in capability) || capability.verification !== true)
    throw new Error("Server未明确启用验证构建，拒绝安装产品合同桥");
  const context = parseRemoteMarketContext(market);
  const events: RemoteContractEvent[] = [];
  const record = (event: RemoteContractEvent) => { events.push(event); options.onEvent?.(event); };
  let currentSocket: WebSocket | null = null;
  let accept = options.accept === true;
  let disposed = false;
  let failure: unknown;
  let consumer: ((update: HostUpdate) => boolean | void) | null = null;
  let lastPacket: {socket:WebSocket; event:MessageEvent; handler:((this:WebSocket,event:MessageEvent)=>unknown)|null; wireKind:string} | null = null;
  const queued: Array<{socket:WebSocket; event:MessageEvent; handler:((this:WebSocket,event:MessageEvent)=>unknown)|null}> = [];
  const host = await attachRemoteHost(baseUrl, token, context, {webSocketFactory(url, protocols) {
    const socket = new WebSocket(url, protocols);
    currentSocket = socket;
    const send = socket.send.bind(socket);
    socket.send = data => {
      const command: unknown = JSON.parse(String(data));
      record({type:"actual-remote-host-sent",command});
      if (command !== null && typeof command === "object" && "AcceptPublication" in command)
        record({type:"actual-remote-host-accepted",value:command.AcceptPublication});
      send(data);
    };
    let handler: ((this:WebSocket,event:MessageEvent)=>unknown)|null = null;
    Object.defineProperty(socket,"onmessage",{configurable:true,get:()=>handler,set:value=>{handler=value;}});
    socket.addEventListener("message", event => {
      try {
        const raw = String(event.data);
        const value: unknown = JSON.parse(raw);
        if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("真实WS验证事件必须是对象");
        record({type:"actual-network-received",value});
        options.onNetwork?.(raw);
        if (Object.hasOwn(value,"ObserverEvidence")) return;
        if (isContractPublication(value as Readonly<Record<string,unknown>>) || (lastPacket === null && Object.hasOwn(value,"Baseline")))
          lastPacket = {socket,event,handler,wireKind:Object.keys(value)[0]!};
        if (!accept && isContractPublication(value as Readonly<Record<string,unknown>>)) {
          if (queued.length >= 65536) throw new Error("验证端暂停投递队列已满，不能静默丢弃前缀");
          queued.push({socket,event,handler});
        } else handler?.call(socket,event);
      } catch (error) { failure=error; record({type:"bridge-error",message:String(error)}); }
    });
    return socket;
  }});
  try {
    await host.subscribeUpdates(update => {
      const accepted = consumer?.(update) !== false;
      record({type:"actual-remote-host-delivered",update,accepted});
      return accepted;
    }, value => { record({type:"actual-remote-host-failure",value}); options.onFailure?.(value); });
  } catch (error) {
    try { await host.dispose(); }
    catch (cleanup) { throw new AggregateError([error, cleanup], "Remote验证桥附着失败且清理失败"); }
    throw error;
  }
  const assertHealthy = () => { if (failure !== undefined) throw failure; if (disposed) throw new Error("Remote验证桥已关闭"); };
  return {
    host, events,
    get socket() { assertHealthy(); if (currentSocket === null) throw new Error("真实Remote WS尚未创建"); return currentSocket; },
    assertHealthy,
    retainStalePacket() {
      assertHealthy();
      if (lastPacket === null || lastPacket.handler === null) throw new Error("没有实际收到且绑定旧host handler的包，不能声称旧包拒绝");
      const packet = lastPacket;
      return () => {
        if (!disposed) throw new Error("旧包复核需要旧host已退役");
        const count = (type:string) => events.filter(event=>event.type===type).length;
        const deliveredBefore=count("actual-remote-host-delivered"), acceptedBefore=count("actual-remote-host-accepted");
        packet.handler!.call(packet.socket,packet.event);
        const deliveredAfter=count("actual-remote-host-delivered"), acceptedAfter=count("actual-remote-host-accepted");
        if (deliveredAfter!==deliveredBefore || acceptedAfter!==acceptedBefore) throw new Error("退役真实host仍接纳或ACK旧包");
        return {wireKind:packet.wireKind,oldPacketRejected:true,deliveredBefore,deliveredAfter,acceptedBefore,acceptedAfter};
      };
    },
    setConsumer(value) { assertHealthy(); consumer=value; },
    setAccept(value) {
      assertHealthy(); accept=value;
      if (accept) for (const item of queued.splice(0)) item.handler?.call(item.socket,item.event);
    },
    async dispose() { if (disposed) return; disposed=true; queued.length=0; await host.dispose(); },
  };
}
