import { parseSaveSlot } from "./save-schema.ts";

export type DayEndSaveReference = { readonly seq: number; readonly settledDate: string };

export function validateDayEndCandidate(value: unknown, reference: DayEndSaveReference) {
  const slot = parseSaveSlot(value);
  if (slot.snapshot.seq !== reference.seq || slot.civil_clock.settled_through !== reference.settledDate) {
    throw new Error(`日终候选不属于请求的自然日 ${reference.settledDate} / seq ${reference.seq}`);
  }
  return slot;
}
