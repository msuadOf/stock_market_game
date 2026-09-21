import assert from "node:assert/strict";
import test from "node:test";
import { civilUpdate, frame, snapshot, tickBatch } from "./protocol-test-fixtures.ts";
import { inspectWasmUpdateDelivery } from "./wasm-update-delivery.ts";

test("Given a TickBatch, when classified for WASM delivery, then it counts one market tick and never pauses", () => {
  const delivery = inspectWasmUpdateDelivery(
    tickBatch([frame(1, 0, ["600000"])], snapshot(1, 1)),
    { pause_after_close: true, pause_before_open: true },
  );

  assert.equal("TickBatch" in delivery.update, true);
  assert.equal(delivery.recordsMarketTick, true);
  assert.equal(delivery.pausesAtBarrier, false);
});

test("Given a CivilUpdate, when classified for WASM delivery, then it never counts a market tick and respects barrier preferences", () => {
  const running = inspectWasmUpdateDelivery(civilUpdate(), {
    pause_after_close: false,
    pause_before_open: false,
  });
  const paused = inspectWasmUpdateDelivery(civilUpdate(), {
    pause_after_close: true,
    pause_before_open: false,
  });

  assert.equal("CivilUpdate" in running.update, true);
  assert.equal(running.recordsMarketTick, false);
  assert.equal(running.pausesAtBarrier, false);
  assert.equal(paused.recordsMarketTick, false);
  assert.equal(paused.pausesAtBarrier, true);
});
