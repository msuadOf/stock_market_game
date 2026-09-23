import assert from "node:assert/strict";
import test from "node:test";
import { assertWorkerE2EStepAllowed, readWorkerSpeedMetrics, restoreWorkerSlot, stepWorkerOnce, workerPausePreferenceRequest } from "./worker-host.ts";
import type { WorkerRequestPort } from "./worker-request.ts";

class FakeWorker implements WorkerRequestPort {
  readonly listeners = new Set<(event: MessageEvent) => void>();
  readonly sent: unknown[] = [];
  addEventListener(_type: "message", listener: (event: MessageEvent) => void): void { this.listeners.add(listener); }
  removeEventListener(_type: "message", listener: (event: MessageEvent) => void): void { this.listeners.delete(listener); }
  postMessage(message: unknown): void { this.sent.push(message); }
  emit(value: unknown): void { for (const listener of this.listeners) listener({ data: value } as MessageEvent); }
}

test("Given a generation-correlated Worker metrics response, when read, then it validates the shared speed contract", async () => {
  const worker = new FakeWorker();
  const pending = readWorkerSpeedMetrics(worker, 1, 2);
  worker.emit({ type: "speedMetrics", requestId: 1, generation: 2, metrics: {
    requested: { mode: "fixed", multiplier: 60 }, actual_multiplier: 60, sample_duration_ms: 1_000, sample_ticks: 60, running: true,
  } });
  assert.equal((await pending).actual_multiplier, 60);
});

test("Given a restore response from the old request generation, when read, then it yields the new authority before load completes", async () => {
  const worker = new FakeWorker();
  const pending = restoreWorkerSlot(worker, { valid: "already-parsed" }, 2, 1);
  worker.emit({
    type: "restored",
    requestId: 2,
    generation: 1,
    nextGeneration: 2,
    snapshot: { seq: 4, tick: 4, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} },
  });
  const restored = await pending;
  assert.equal(restored.nextGeneration, 2);
  assert.equal(restored.snapshot.seq, 4);
  assert.equal(restored.snapshot.tick, 4);
  assert.equal(worker.sent.length, 1);
});

test("Given local pause preferences, when sent across the Worker boundary, then the request retains both flags and generation", () => {
  assert.deepEqual(workerPausePreferenceRequest(9, 3, {
    pause_after_close: true,
    pause_before_open: false,
  }), {
    type: "setPausePreferences",
    requestId: 9,
    generation: 3,
    preferences: { pause_after_close: true, pause_before_open: false },
  });
});

test("Given a paused Worker, the correlated single-step response exposes its committed tick", async () => {
  const worker = new FakeWorker();
  const pending = stepWorkerOnce(worker, 10, 3);
  worker.emit({ type: "stepped", requestId: 10, generation: 3, tick: 8 });
  assert.equal(await pending, 8);
  assert.deepEqual(worker.sent, [{ type: "stepOnce", requestId: 10, generation: 3 }]);
});

test("Given production or a missing injected capability, Worker E2E stepping is rejected", () => {
  assert.throws(() => assertWorkerE2EStepAllowed(false, true), /E2E 构建/);
  assert.throws(() => assertWorkerE2EStepAllowed(true, false), /未注入/);
  assert.doesNotThrow(() => assertWorkerE2EStepAllowed(true, true));
});
