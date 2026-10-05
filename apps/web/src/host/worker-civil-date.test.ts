import assert from "node:assert/strict";
import test from "node:test";
import { createWorkerHost, refreshWorkerBaseline, restoreWorkerSlot } from "./worker-host.ts";
import { WorkerRequestScope, type WorkerRequestPort } from "./worker-request.ts";
import { snapshot } from "./protocol-test-fixtures.ts";
import { currentSaveFixture } from "../save/current-save-fixture.ts";
import type { HostUpdate } from "./host-update.ts";

for (const operation of ["initial", "refresh", "restore"] as const) {
  test(`Worker ${operation} baseline 使用同一权威会话的自然日`, { timeout: 10000 }, async () => {
    const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
    class DateWorker extends EventTarget {
      static current: DateWorker;
      sent: Record<string, unknown>[] = [];
      constructor() { super(); DateWorker.current = this; }
      postMessage(value: Record<string, unknown>) { this.sent.push(value); }
      terminate() {}
      emit(value: unknown) { this.dispatchEvent(Object.assign(new Event("message"), { data: value })); }
    }
    Object.defineProperty(globalThis, "Worker", { configurable: true, value: DateWorker });
    let host: Awaited<ReturnType<typeof createWorkerHost>> | undefined;
    try {
      const creating = createWorkerHost({} as Parameters<typeof createWorkerHost>[0], 42n);
      const worker = DateWorker.current;
      worker.emit({ type: "baseline", generation: 1, snapshot: snapshot(0, 0), civilDate: "2030-01-02" });
      host = await creating;
      const updates: HostUpdate[] = [];
      const starting = host.start((update) => { updates.push(update); });
      worker.emit({ type: "started", generation: 1, requestId: worker.sent.at(-1)!.requestId });
      await starting;
      if (operation !== "initial") {
        const pending = operation === "restore" ? host.load(currentSaveFixture()) : host.refreshBaseline();
        const requestId = worker.sent.at(-1)!.requestId;
        worker.emit(operation === "restore"
          ? { type: "restored", generation: 1, nextGeneration: 2, requestId, snapshot: snapshot(1, 1), civilDate: "2031-02-03" }
          : { type: "refreshed", generation: 1, requestId, snapshot: snapshot(1, 1), civilDate: "2031-02-03" });
        await pending;
      }
      assert.equal(updates.at(-1)?.civilDate, operation === "initial" ? "2030-01-02" : "2031-02-03");
    } finally {
      await host?.dispose();
      if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
      else Object.defineProperty(globalThis, "Worker", original);
    }
  });
}

for (const operation of ["refresh", "restore"] as const) {
  for (const civilDate of [undefined, "2030-02-30"]) {
    test(`Worker ${operation} 拒绝缺失或不存在的自然日 ${String(civilDate)}`, { timeout: 10000 }, async () => {
      class InvalidDateWorker implements WorkerRequestPort {
        readonly listeners = new Set<(event: MessageEvent) => void>();
        addEventListener(_type: "message", listener: (event: MessageEvent) => void) { this.listeners.add(listener); }
        removeEventListener(_type: "message", listener: (event: MessageEvent) => void) { this.listeners.delete(listener); }
        postMessage() {}
        emit(data: unknown) { for (const listener of this.listeners) listener(new MessageEvent("message", { data })); }
      }
      const worker = new InvalidDateWorker();
      const requests = new WorkerRequestScope(worker);
      const pending = operation === "restore"
        ? restoreWorkerSlot(requests, currentSaveFixture(), 1, 1)
        : refreshWorkerBaseline(requests, 1, 1);
      worker.emit({
        type: operation === "restore" ? "restored" : "refreshed",
        requestId: 1, generation: 1, nextGeneration: 2, snapshot: snapshot(0, 0), civilDate,
      });
      await assert.rejects(pending, /civilDate/);
    });
  }
}
