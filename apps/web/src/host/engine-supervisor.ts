import { createEngineSupervisor as createLibosSupervisor, type EngineSupervisorOptions as LibosOptions } from "../../../../packages/libos/browser/engine-supervisor.ts";
import { parseHostFailure } from "./protocol-failure.ts";

export { allocateFacadeWorkerId, FacadeSpawnError, type FacadePrimaryLayout, type FacadeSubstrate, type SupervisorTimers } from "../../../../packages/libos/browser/engine-supervisor.ts";
export type EngineSupervisorOptions = Omit<LibosOptions, "parseHostFailure" | "workerNamePrefix">;

/** 宿主注入协议错误解析；线程、槽位和关停机制均由 LibOS 管理。 */
export function createEngineSupervisor(options: EngineSupervisorOptions) {
  return createLibosSupervisor({ ...options, parseHostFailure, workerNamePrefix: "stock-game" });
}
