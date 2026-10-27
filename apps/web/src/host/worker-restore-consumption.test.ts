import assert from "node:assert/strict";
import test from "node:test";
import {createWorkerHost} from "./worker-host.ts";
import {currentSaveFixture} from "../save/current-save-fixture.ts";
import {snapshot} from "./protocol-test-fixtures.ts";

test("独立review：restored回包缺目录后关闭Worker并禁旧generation发送",{timeout:10000},async()=>{
 const original=Object.getOwnPropertyDescriptor(globalThis,"Worker");
 class W extends EventTarget {
  static current:W; sent:Record<string,unknown>[]=[]; terminated=0;
  constructor(){super();W.current=this;}
  postMessage(v:Record<string,unknown>){this.sent.push(v);if(v.type==="enqueue")this.emit({type:"enqueued",requestId:v.requestId,generation:v.generation});}
  terminate(){this.terminated++;}
  emit(data:unknown){this.dispatchEvent(Object.assign(new Event("message"),{data}));}
 }
 Object.defineProperty(globalThis,"Worker",{configurable:true,value:W});
 let host:Awaited<ReturnType<typeof createWorkerHost>>|undefined;
 try {
  const creating=createWorkerHost({} as never,1n);const w=W.current;
  w.emit({type:"ingress",generation:1,shared:false});
  w.emit({type:"baseline",generation:1,snapshot:snapshot(0,0),civilDate:"2030-01-02",stockCatalog:[{id:0,code:"600101"}],companyCatalog:[{id:0,code:"C-600101",name:"公司",listed_stock:0}]});
  host=await creating;
  const loading=host.load(currentSaveFixture());const req=w.sent.at(-1)!;
  const refused=assert.rejects(loading,/companyCatalog/);
  w.emit({type:"ingress",generation:2,shared:false});
  w.emit({type:"restored",requestId:req.requestId,generation:1,nextGeneration:2,snapshot:snapshot(0,0),civilDate:"2030-01-02",stockCatalog:[{id:0,code:"600101"}]});
  await refused;
  await Promise.allSettled([host.submitIntent({Cancel:{code:"600101",id:7}})]);
  const enqueues=w.sent.filter(v=>v.type==="enqueue");
  assert.equal(enqueues.length,0,`已收到新ingress并恢复回包，但仍发送旧请求：${JSON.stringify(enqueues)}`);
 }finally{await host?.dispose();if(original===undefined)delete(globalThis as {Worker?:unknown}).Worker;else Object.defineProperty(globalThis,"Worker",original);}
});

for (const scenario of ["precommit", "malformed-next", "missing-catalog", "new-ingress-error"] as const) {
 test(`Worker restore消费边界 ${scenario} 明确区分提交前拒绝与已替换authority`, {timeout:10000}, async()=>{
  const original=Object.getOwnPropertyDescriptor(globalThis,"Worker");
  class W extends EventTarget {
   static current:W; sent:Record<string,unknown>[]=[]; terminated=0;
   constructor(){super();W.current=this;}
   postMessage(v:Record<string,unknown>){this.sent.push(v);if(v.type==="enqueue")this.emit({type:"enqueued",requestId:v.requestId,generation:v.generation});}
   terminate(){this.terminated++;}
   emit(data:unknown){this.dispatchEvent(Object.assign(new Event("message"),{data}));}
  }
  Object.defineProperty(globalThis,"Worker",{configurable:true,value:W});
  let host:Awaited<ReturnType<typeof createWorkerHost>>|undefined;
  try {
   const creating=createWorkerHost({} as never,1n);const w=W.current;
   w.emit({type:"ingress",generation:1,shared:false});
   w.emit({type:"baseline",generation:1,snapshot:snapshot(0,0),civilDate:"2030-01-02",stockCatalog:[{id:0,code:"600101"}],companyCatalog:[{id:0,code:"C-600101",name:"公司",listed_stock:0}]});
   host=await creating;
   const visible=host.snapshot();
   const saved=host.save();
   const savedResult=scenario==="precommit" ? saved : assert.rejects(saved,/WASM_WORKER_RESTORE_BASELINE/);
   const saveRequest=w.sent.at(-1)!;
   const loading=host.load(currentSaveFixture());const req=w.sent.at(-1)!;
   const refused=assert.rejects(loading,scenario==="malformed-next"?/正安全整数/:scenario==="missing-catalog"?/companyCatalog/:/候选拒绝/);
   if(scenario==="new-ingress-error")w.emit({type:"ingress",generation:2,shared:false});
   if(scenario==="precommit"||scenario==="new-ingress-error")w.emit({type:"operationError",requestId:req.requestId,generation:1,message:"候选拒绝"});
   else w.emit({type:"restored",requestId:req.requestId,generation:1,nextGeneration:scenario==="malformed-next"?"2":2,snapshot:snapshot(0,0),civilDate:"2030-01-02",stockCatalog:[{id:0,code:"600101"}]});
   await refused;
   assert.equal(host.snapshot(),visible,"解析失败不能覆盖先前可见baseline");
   if(scenario==="precommit"){
    assert.equal(w.terminated,0);
    await host.submitIntent({Cancel:{code:"600101",id:7}});
    assert.equal(w.sent.filter(v=>v.type==="enqueue").length,1);
    w.emit({type:"saved",requestId:saveRequest.requestId,generation:1,slot:{saved:true}});
    assert.deepEqual(await savedResult,{saved:true});
   }else{
    await assert.rejects(host.submitIntent({Cancel:{code:"600101",id:7}}),/已关闭/);
    assert.equal(w.sent.filter(v=>v.type==="enqueue").length,0);
    assert.equal(w.terminated,1);
    await savedResult;
   }
  }finally{await host?.dispose();if(original===undefined)delete(globalThis as {Worker?:unknown}).Worker;else Object.defineProperty(globalThis,"Worker",original);}
 });
}
