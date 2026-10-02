import assert from "node:assert/strict";
import { test } from "node:test";
import { observeChartContainers } from "./chart-resize.ts";

test("chart size observer watches both containers and disconnects on cleanup", () => {
  const previousDescriptor = Object.getOwnPropertyDescriptor(globalThis, "ResizeObserver");
  const observed: Element[] = [];
  let resizeCount = 0;
  let disconnected = false;
  let notifyResize: ResizeObserverCallback | undefined;
  class TestResizeObserver {
    constructor(callback: ResizeObserverCallback) {
      notifyResize = callback;
    }

    observe(target: Element) {
      observed.push(target);
    }

    disconnect() {
      disconnected = true;
    }

    unobserve() {}
  }
  Object.defineProperty(globalThis, "ResizeObserver", { value: TestResizeObserver, configurable: true });

  try {
    const first = {} as Element;
    const second = {} as Element;
    const disconnect = observeChartContainers([first, second], () => { resizeCount += 1; });
    notifyResize?.([], {} as ResizeObserver);
    disconnect();

    assert.deepEqual(observed, [first, second]);
    assert.equal(resizeCount, 1);
    assert.equal(disconnected, true);
  } finally {
    if (previousDescriptor === undefined) delete (globalThis as { ResizeObserver?: typeof ResizeObserver }).ResizeObserver;
    else Object.defineProperty(globalThis, "ResizeObserver", previousDescriptor);
  }
});
