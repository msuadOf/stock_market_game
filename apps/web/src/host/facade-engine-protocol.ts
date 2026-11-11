import type { FacadeSlotLayout } from "./facade-thread-binding.ts";
import { GEN_BITS_DEFAULT, HEADER_WORDS, JOIN_STRIDE, SLOT_STRIDE } from "./facade-slot-protocol.ts";

export interface FacadeEngineSubstrateMessage {
  readonly type: "facadeEngineSubstrate";
  readonly module: WebAssembly.Module;
  readonly memory: WebAssembly.Memory;
  readonly layout: FacadeSlotLayout;
  readonly diagnostics: boolean;
}
/** 同一专用控制端只分配一个 Memory；配置阶段不再重复 structured clone Memory。 */
export interface FacadeEngineMemoryMessage {
  readonly type: "facadeEngineMemory";
  readonly allocationId: number;
  readonly memory: WebAssembly.Memory;
}
export interface FacadeEngineDeathMessage {
  readonly type: "facadeEngineDeath";
  readonly allocationId: 1;
  readonly primary: unknown;
}
export function parseFacadeEngineDeath(value: unknown): FacadeEngineDeathMessage {
  const source = record(value);
  if (source.type !== "facadeEngineDeath" || source.allocationId !== 1
    || Object.keys(source).sort().join(",") !== "allocationId,primary,type") throw new Error("facadeEngineDeath 形状非法");
  return { type: "facadeEngineDeath", allocationId: 1, primary: source.primary };
}
export interface FacadeEngineConfigurationMessage {
  readonly type: "facadeEngineConfiguration";
  readonly allocationId: number;
  readonly module: WebAssembly.Module;
  readonly layout: FacadeSlotLayout;
  readonly diagnostics: boolean;
  readonly libosFault?: unknown;
}
export function parseFacadeEngineMemory(value: unknown): FacadeEngineMemoryMessage {
  const source = record(value);
  if (source.type !== "facadeEngineMemory" || Object.keys(source).sort().join(",") !== "allocationId,memory,type"
    || source.allocationId !== 1 || !(source.memory instanceof WebAssembly.Memory) || !(source.memory.buffer instanceof SharedArrayBuffer)) {
    throw new Error("facadeEngineMemory 必须为本控制端首次分配的 shared Memory");
  }
  return { type: "facadeEngineMemory", allocationId: 1, memory: source.memory };
}
export function parseFacadeEngineConfiguration(value: unknown): FacadeEngineConfigurationMessage {
  const source = record(value);
  if (source.type !== "facadeEngineConfiguration" || source.allocationId !== 1
    || Object.keys(source).some((key) => !["type", "allocationId", "module", "layout", "diagnostics", "libosFault"].includes(key))
    || !(source.module instanceof WebAssembly.Module) || typeof source.diagnostics !== "boolean") throw new Error("facadeEngineConfiguration 形状非法");
  if (Object.hasOwn(source, "libosFault") && source.libosFault === undefined) throw new Error("显式 LibOS 故障布局不得为 undefined");
  // layout 的完整 header/bounds 在 supervisor 关联唯一已登记 Memory 后校验。
  if (source.layout === null || typeof source.layout !== "object" || Array.isArray(source.layout)) throw new Error("facade engine layout 必须为对象");
  return { type: "facadeEngineConfiguration", allocationId: 1, module: source.module, layout: source.layout as unknown as FacadeSlotLayout,
    diagnostics: source.diagnostics,
    ...(Object.hasOwn(source, "libosFault") ? { libosFault: source.libosFault } : {}) };
}
function record(value: unknown): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("facade engine 控制消息必须为对象");
  return value as Record<string, unknown>;
}
export function parseFacadeEngineConfigured(value: unknown): { readonly type: "facadeEngineConfigured" } {
  const source = record(value);
  if (source.type !== "facadeEngineConfigured" || Object.keys(source).length !== 1) throw new Error("facadeEngineConfigured 形状非法");
  return { type: "facadeEngineConfigured" };
}
export function parseFacadeEngineSubstrate(value: unknown): FacadeEngineSubstrateMessage {
  const source = record(value);
  if (source.type !== "facadeEngineSubstrate" || Object.keys(source).some((key) => !["type", "module", "memory", "layout", "diagnostics"].includes(key))) throw new Error("facadeEngineSubstrate 形状非法");
  const { module, memory, diagnostics } = source;
  if (!(module instanceof WebAssembly.Module) || !(memory instanceof WebAssembly.Memory) || typeof SharedArrayBuffer !== "function" || !(memory.buffer instanceof SharedArrayBuffer) || typeof diagnostics !== "boolean") throw new Error("facadeEngineSubstrate 必须为真实 module/shared memory/diagnostics");
  const input = record(source.layout);
  const keys = ["baseAddr", "headerWords", "slotStride", "joinStride", "capacity", "genBits"] as const;
  if (Object.keys(input).length !== keys.length || Object.keys(input).some((key) => !(keys as readonly string[]).includes(key))) throw new Error("facade engine layout 形状非法");
  const values = keys.map((key) => {
    const result = input[key];
    if (typeof result !== "number" || !Number.isInteger(result) || result < 0 || result > 0xffffffff) throw new Error(`facade layout.${key} 必须为 u32`);
    return result;
  });
  const [baseAddr, headerWords, slotStride, joinStride, capacity, genBits] = values;
  if (baseAddr % 4 !== 0 || headerWords !== HEADER_WORDS || slotStride !== SLOT_STRIDE || joinStride !== JOIN_STRIDE || capacity === 0 || genBits < 1 || genBits > GEN_BITS_DEFAULT) throw new Error("facade engine layout 与槽协议不一致");
  return { type: "facadeEngineSubstrate", module, memory, diagnostics, layout: Object.freeze({ baseAddr, headerWords, slotStride, joinStride, capacity, genBits }) };
}
