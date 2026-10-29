import type { FacadeChildControlMessage } from "./facade-child-bootstrap.ts";
import { parseHostFailure } from "./protocol-failure.ts";
import { parseFacadeEngineSubstrate, type FacadeEngineSubstrateMessage } from "./facade-engine-protocol.ts";
import { parseFacadeEngineMemory, parseFacadeEngineConfiguration, type FacadeEngineMemoryMessage, type FacadeEngineConfigurationMessage } from "./facade-engine-protocol.ts";

export type FacadeSupervisorControlMessage = FacadeChildControlMessage | FacadeEngineSubstrateMessage | FacadeEngineMemoryMessage | FacadeEngineConfigurationMessage;

/** 先于业务 parser 分流；已识别的 facade 消息不允许静默落入业务路径。 */
export function parseFacadeSupervisorControl(value: unknown): FacadeSupervisorControlMessage | null {
  if (value === null || typeof value !== "object") return null;
  const source = value as Record<string, unknown>;
  const type = source.type;
  if (typeof type !== "string" || !type.startsWith("facade")) return null;
  function exact(keys: string[]): void {
    if (Object.keys(source).some((key) => !keys.includes(key))) throw new Error(`${type} 包含未知字段`);
  }
  function u32(key: string): number {
    const result = source[key];
    if (typeof result !== "number" || !Number.isInteger(result) || result < 0 || result > 0xffffffff) throw new Error(`${type}.${key} 必须为 u32`);
    return result;
  }
  switch (type) {
    case "facadeEngineMemory": return parseFacadeEngineMemory(value);
    case "facadeEngineConfiguration": return parseFacadeEngineConfiguration(value);
    case "facadeEngineSubstrate": return parseFacadeEngineSubstrate(value);
    case "facadeSpawn": {
      exact(["type", "slot", "gen", "name"]);
      const name = source.name;
      if (typeof name !== "string") throw new Error("facadeSpawn.name 必须为 string");
      return { type, slot: u32("slot"), gen: u32("gen"), name };
    }
    case "facadeTerminateChild":
      exact(["type", "workerId"]);
      return { type, workerId: u32("workerId") };
    case "facadeChildReady":
    case "facadeWorkerExit":
      exact(["type", "workerId", "slot", "gen"]);
      return { type, workerId: u32("workerId"), slot: u32("slot"), gen: u32("gen") };
    case "facadeBootstrapFailure": {
      const phase = source.phase;
      if (phase === "validate") {
        exact(["type", "phase", "failure"]);
        return { type, phase, failure: parseHostFailure(source.failure) };
      }
      if (phase !== "instantiate" && phase !== "layout" && phase !== "trampoline" && phase !== "cleanup") throw new Error("facadeBootstrapFailure.phase 非法");
      exact(["type", "phase", "failure", "workerId", "slot", "gen"]);
      return { type, phase, workerId: u32("workerId"), slot: u32("slot"), gen: u32("gen"), failure: parseHostFailure(source.failure) };
    }
    default: throw new Error(`未知 facade 控制消息 ${type}`);
  }
}
