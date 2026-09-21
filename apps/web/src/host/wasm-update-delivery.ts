import type { PausePreferences } from "../types/generated/PausePreferences.ts";
import { parseEngineUpdate } from "./protocol/index.ts";

export type WasmUpdateDelivery = {
  readonly update: ReturnType<typeof parseEngineUpdate>;
  readonly recordsMarketTick: boolean;
  readonly pausesAtBarrier: boolean;
};

export function inspectWasmUpdateDelivery(
  rawUpdate: unknown,
  preferences: PausePreferences,
): WasmUpdateDelivery {
  const update = parseEngineUpdate(rawUpdate);
  const pausesAtBarrier = "CivilUpdate" in update && (
    (preferences.pause_after_close && update.CivilUpdate.kinds.includes("AfterClose"))
    || (preferences.pause_before_open && update.CivilUpdate.kinds.includes("BeforeOpen"))
  );
  return {
    update,
    recordsMarketTick: "TickBatch" in update,
    pausesAtBarrier,
  };
}
