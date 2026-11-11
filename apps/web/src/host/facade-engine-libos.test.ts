import assert from "node:assert/strict";
import test from "node:test";
import { parseFacadeEngineConfiguration } from "./facade-engine-protocol.ts";
import { HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE } from "./facade-slot-protocol.ts";

test("配置消息携带 LibOS 故障布局并保留供 supervisor 校验", { timeout: 10000 }, () => {
  const raw = { globalStateAddress: 8192, domainAbortAddress: 8200, slots: [{abortAddress: 8204, waitAddress: 8208}] };
  const layout = { baseAddr: 0, headerWords: HEADER_WORDS, slotStride: SLOT_STRIDE, joinStride: JOIN_STRIDE, capacity: 1, genBits: 29 };
  const result = parseFacadeEngineConfiguration({type:"facadeEngineConfiguration", allocationId:1, module:new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0])), layout, diagnostics:false, libosFault:raw});
  assert.deepEqual(result.libosFault, raw);
});

test("显式提供的 LibOS 布局不能是 undefined", { timeout: 10000 }, () => {
  const layout = { baseAddr:0, headerWords:HEADER_WORDS, slotStride:SLOT_STRIDE, joinStride:JOIN_STRIDE, capacity:1, genBits:29 };
  assert.throws(() => parseFacadeEngineConfiguration({type:"facadeEngineConfiguration", allocationId:1, module:new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0])), layout, diagnostics:false, libosFault:undefined}));
});
