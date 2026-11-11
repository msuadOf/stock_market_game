import assert from "node:assert/strict";
import test from "node:test";
import { createFacadeEngineInitializer } from "./facade-engine-initialize.ts";
import { HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE } from "./facade-slot-protocol.ts";

async function exercise(runtime: "enabled" | "disabled" | "unspecified", layoutValue: unknown, exportName: "verification_host_fault_layout" | "libos_host_fault_layout" | "none" = "libos_host_fault_layout") {
  const channel = new MessageChannel();
  const memory = new WebAssembly.Memory({initial:1, maximum:2, shared:true});
  const module = new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0]));
  const layout = {baseAddr:0,headerWords:HEADER_WORDS,slotStride:SLOT_STRIDE,joinStride:JOIN_STRIDE,capacity:2,genBits:29};
  const primary = {engine:{workerId:0,address:5000},ingress:{workerId:4294967295,address:6000},death:{address:7000,bytes:4}};
  let calls = 0, configured: Record<string, unknown> | undefined;
  channel.port1.addEventListener("message", event => {
    if (event.data.type === "facadeEngineMemory") channel.port1.postMessage({type:"facadeEngineMemoryRegistered",allocationId:1});
    else if (event.data.type === "facadeEngineDeath") channel.port1.postMessage({type:"facadeEngineDeathRegistered",allocationId:1});
    else if (event.data.type === "facadeEngineConfiguration") { configured = event.data; channel.port1.postMessage({type:"facadeEngineConfigured"}); }
  });
  channel.port1.start();
  const initializer = createFacadeEngineInitializer({ isolated:()=>true, createMemory:()=>memory,
    environment:{bindFacadeThreadEnvironment(){},installFacadeSlotLayout(){},getArenaView:()=>({words:new Int32Array(memory.buffer),capacity:2,genBits:29})},
    load:async()=>({module,diagnostics:false,glue:{initSync:()=>({memory,__wbindgen_thread_destroy(){}}),wasm_tombstone_init_primary:()=>primary,
      facade_slot_init_for_setup(){},facade_slot_layout:()=>layout,set_step_runtime_override(){},
      ...(exportName === "none" ? {} : { [exportName]:()=>{ calls++; return layoutValue; } }),
    }}),onFailure:()=>{},
  });
  const options = runtime === "enabled" ? {runtimeOverride:true} : runtime === "disabled" ? {runtimeOverride:false} : {};
  try {
    await initializer.initialize({type:"init",controlPort:channel.port2,setup:{},...options});
    return {calls,configured};
  } finally { initializer.dispose(); channel.port1.close(); channel.port2.close(); }
}

test("关闭或未请求 runtime 不访问故障布局", {timeout:10000}, async () => {
  for (const mode of ["disabled", "unspecified"] as const) {
    const result = await exercise(mode, {unexpected:true});
    assert.equal(result.calls, 0);
    assert.equal(Object.hasOwn(result.configured!, "libosFault"), false);
  }
});
test("启用 runtime 时故障布局导出不得返回 undefined", {timeout:10000}, async () => {
  await assert.rejects(exercise("enabled", undefined), /故障域.*布局/);
});
test("启用 runtime 时布局随配置发送", {timeout:10000}, async () => {
  const raw = {globalStateAddress:8192,domainAbortAddress:8200,slots:[]};
  const result = await exercise("enabled", raw);
  assert.equal(result.calls, 1);
  assert.deepEqual(result.configured?.libosFault, raw);
});
test("正式 LibOS 故障布局随 runtime 配置发送", {timeout:10000}, async () => {
  const raw = {globalStateAddress:8192,domainAbortAddress:8200,slots:[]};
  const result = await exercise("enabled", raw, "libos_host_fault_layout");
  assert.equal(result.calls, 1);
  assert.deepEqual(result.configured?.libosFault, raw);
});
test("正式 LibOS 故障布局缺失时显式失败", {timeout:10000}, async () => {
  await assert.rejects(exercise("enabled", undefined, "libos_host_fault_layout"), /故障域.*布局/);
});
test("启用 runtime 时无正式 LibOS 导出不得握手成功", {timeout:10000}, async () => {
  await assert.rejects(exercise("enabled", {}, "none"), /LibOS.*故障.*导出/);
  await assert.rejects(exercise("enabled", {}, "verification_host_fault_layout"), /LibOS.*故障.*导出/);
});
test("关闭或未请求 runtime 时不要求正式 LibOS 导出", {timeout:10000}, async () => {
  for (const mode of ["disabled", "unspecified"] as const) {
    const result = await exercise(mode, {}, "none");
    assert.equal(Object.hasOwn(result.configured!, "libosFault"), false);
  }
});
