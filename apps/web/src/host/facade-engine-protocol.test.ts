import assert from "node:assert/strict";
import test from "node:test";
import { parseFacadeEngineConfigured, parseFacadeEngineSubstrate } from "./facade-engine-protocol.ts";
import { HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE } from "./facade-slot-protocol.ts";

test("engine substrate 保留同一 module/memory，configured 只接受唯一形状", { timeout: 10000 }, () => {
  const message = { type: "facadeEngineSubstrate", module: new WebAssembly.Module(new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0])), memory: new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true }), layout: { baseAddr: 0, headerWords: HEADER_WORDS, slotStride: SLOT_STRIDE, joinStride: JOIN_STRIDE, capacity: 4, genBits: 29 }, diagnostics: false };
  const parsed = parseFacadeEngineSubstrate(message);
  assert.equal(parsed.module, message.module); assert.equal(parsed.memory, message.memory);
  assert.deepEqual(parseFacadeEngineConfigured({ type: "facadeEngineConfigured" }), { type: "facadeEngineConfigured" });
  assert.throws(() => parseFacadeEngineConfigured({ type: "facadeEngineConfigured", requestId: 1 }));
  assert.throws(() => parseFacadeEngineSubstrate({ ...message, workerId: 1 }));
  assert.throws(() => parseFacadeEngineSubstrate({ ...message, memory: new WebAssembly.Memory({ initial: 1 }) }));
  assert.throws(() => parseFacadeEngineSubstrate({ ...message, layout: { ...message.layout, capacity: -1 } }));
});
