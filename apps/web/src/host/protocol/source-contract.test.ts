import assert from "node:assert/strict";
import test from "node:test";
import { frame, tickBatch } from "../protocol-test-fixtures.ts";
import { parseEngineUpdate } from "./parse.ts";

function expiryBatch(source: string) {
  const value = structuredClone(frame(1, 0, ["600000"])) as unknown as { facts: { key: { phase_rank: number; source: string } }[] };
  value.facts[0]!.key.phase_rank = 1;
  value.facts[0]!.key.source = source;
  return tickBatch([value as never], null);
}

test("EventSourceIndex 使用 QuoteExpiry，保留 phase_rank 并拒绝旧 P0", () => {
  const parsed = parseEngineUpdate(expiryBatch("QuoteExpiry"));
  assert.ok("TickBatch" in parsed);
  assert.equal(parsed.TickBatch.frames[0]!.facts[0]!.key.source, "QuoteExpiry");
  assert.equal(parsed.TickBatch.frames[0]!.facts[0]!.key.phase_rank, 1);
  assert.throws(() => parseEngineUpdate(expiryBatch("P0")), /source/);
});
