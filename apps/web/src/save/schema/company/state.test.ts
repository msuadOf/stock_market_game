import assert from "node:assert/strict";
import test from "node:test";
import { SaveSchemaError } from "../primitives.ts";
import { parseSaveJson, parseSaveSlot } from "../../save-schema.ts";
import { names, historicalRaw, correctionProjectedRaw, policyProjectedRaw, raw, sample, system, binding } from "./state-test-fixture.ts";
for(const [index,name] of names.entries()) test(`历史Rust${name}的synthetic bindings投影对象/raw无损且单一权威tag`, {timeout:10000}, () => {
 const value=sample(index);assert.deepEqual(parseSaveSlot(value),value);assert.deepEqual(parseSaveJson(raw[index]),value);
 assert.deepEqual(Object.keys(parseSaveSlot(value).company_state),[index===2?"LegacyOperations":"CompanySystem"]);
});
for(const [index,name] of names.entries())test(`历史Rust${name}原件保持未投影并独立拒绝，合成空表后缺runtime_sync_policy明确拒绝`,{timeout:10000},()=>{
 assert.throws(()=>parseSaveSlot(JSON.parse(historicalRaw[index])),/report_correction_operations/);
 assert.throws(()=>parseSaveJson(historicalRaw[index]),/report_correction_operations/);
 assert.throws(()=>parseSaveSlot(JSON.parse(correctionProjectedRaw[index])),/runtime_sync_policy/);
 assert.throws(()=>parseSaveJson(correctionProjectedRaw[index]),/runtime_sync_policy/);
});
for(const index of [0,1])test(`历史Rust${names[index]}的synthetic policy投影缺required bindings明确拒绝`,{timeout:10000},()=>{
 assert.throws(()=>parseSaveSlot(JSON.parse(policyProjectedRaw[index])),/repurchase_execution_bindings/);
 assert.throws(()=>parseSaveJson(policyProjectedRaw[index]),/repurchase_execution_bindings/);
});
test("required单tag拒绝旧双源根字段、缺失/null/双tag/未知tag", {timeout:10000}, () => {
 const value=sample();assert.deepEqual(parseSaveSlot(value),value);
 for(const company_state of [undefined,null,{}, {Unknown:{}}, {...value.company_state as object, LegacyOperations:{}}]){const x={...value,company_state};if(company_state===undefined)delete(x as Record<string,unknown>).company_state;assert.throws(()=>parseSaveSlot(x),/company_state/);}
 for(const key of ["groups","company_operations","closing_registry","company_system","corporate_actions"])assert.throws(()=>parseSaveSlot({...value,[key]:{}}),new RegExp(key));
 for(const index of [0,2]){const root=sample(index);const state=root.company_state as Record<string,Record<string,unknown>>;const tag=Object.keys(state)[0];for(const key of Object.keys(state[tag])){const x=structuredClone(root);delete(x.company_state as typeof state)[tag][key];assert.throws(()=>parseSaveSlot(x),new RegExp(key));}assert.throws(()=>parseSaveSlot({...root,company_state:{[tag]:{...state[tag],extra:0}}}),/extra/);}
});
test("setup模式与selected公司tag必须一致，不迁移旧源", {timeout:10000}, () => {
 const simple=sample();assert.deepEqual(parseSaveSlot(simple),simple);const legacy=sample(2);assert.deepEqual(parseSaveSlot(legacy),legacy);
 assert.throws(()=>parseSaveSlot({...simple,company_state:legacy.company_state}),/company_state/);
 assert.throws(()=>parseSaveSlot({...legacy,company_state:simple.company_state}),/company_state/);
});
test("真实REG SettlementDayMarket.binding五字段required且unknown逐层拒绝", {timeout:10000}, () => {
 const root=sample();assert.deepEqual(parseSaveSlot(root),root);assert.deepEqual(Object.keys(binding(root)).sort(),["market_source","market_unit","opening","post","predecessor"]);
 for(const key of ["predecessor","opening","market_unit","market_source","post"]){const x=sample();delete binding(x)[key];assert.throws(()=>parseSaveSlot(x),new RegExp(key));const y=sample();binding(y)[key]=null;assert.throws(()=>parseSaveSlot(y),new RegExp(key));}
 const walk=(node:unknown,path:(string|number)[]=[])=>{if(node!==null&&typeof node==="object"&&!Array.isArray(node)){const x=sample();let target:any=binding(x);for(const key of path)target=target[key];target.unexpected=0;assert.throws(()=>parseSaveSlot(x),/unexpected/);for(const [key,v]of Object.entries(node))walk(v,[...path,key]);}else if(Array.isArray(node))node.forEach((v,i)=>walk(v,[...path,i]));};walk(binding(root));
});
test("公司状态全部真实非空结构拒绝未知字段，nullable必填不被默认掩盖", {timeout:10000}, () => {
 const root=sample();assert.deepEqual(parseSaveSlot(root),root);
 const collect=(node:unknown,path:(string|number)[]=[],all:(string|number)[][]=[]): (string|number)[][]=>{if(node!==null&&typeof node==="object"&&!Array.isArray(node)){all.push(path);for(const [key,v]of Object.entries(node))collect(v,[...path,key],all);}else if(Array.isArray(node))node.forEach((v,i)=>collect(v,[...path,i],all));return all;};
 const nullable:(string|number)[][]=[];const findNull=(node:unknown,path:(string|number)[]=[])=>{if(node===null)nullable.push(path);else if(typeof node==="object")for(const [key,v]of Object.entries(node as object))findNull(v,[...path,key]);};findNull(root.company_state);
 for(const path of nullable){const x=sample();let target:any=x.company_state;for(const key of path.slice(0,-1))target=target[key];delete target[path.at(-1)!];assert.throws(()=>parseSaveSlot(x),new RegExp(String(path.at(-1))));}
 for(const path of collect(root.company_state)){const x=sample();let target:any=x.company_state;for(const key of path)target=target[key];const keys=Object.keys(target);if(keys.length===0||keys.every(k=>/^\d+$/.test(k)))continue;target.unexpected=0;const location="company_state"+path.map(key=>typeof key==="number"?`[${key}]`:`.${key}`).join("");assert.throws(()=>parseSaveSlot(x),(error:unknown)=>{assert.ok(error instanceof Error,`${location}: ${String(error)}`);if(error instanceof SaveSchemaError)assert.ok(error.path===location||error.path.startsWith(location+"."),error.path);else assert.ok(error.message.startsWith(location+" ")||error.message.startsWith(location+"."),error.message);return true;});}
});
test("rawJSON在覆盖前拒绝重复tag、转义后同键与REG binding重复键", {timeout:10000}, () => {
 assert.deepEqual(parseSaveJson(raw[0]),sample());
 for(const field of ["company_state","system","predecessor","market_unit","post"]){const input=raw[0];const marker=`"${field}":`;const index=input.indexOf(marker);assert.ok(index>=0);assert.throws(()=>parseSaveJson(input.slice(0,index)+`"${field}":null,`+input.slice(index)),/重复/);}
 assert.throws(()=>parseSaveJson(raw[0].replace('"company_state":','"company_state":null,"\\u0063ompany_state":')),/重复/);
});
test("canonical整数字符串拒绝前导零/负零与unsafe裸数，保真实单位", {timeout:10000}, () => {
 const root=sample();assert.deepEqual(parseSaveSlot(root),root);
 for(const value of ["00","01","+1","-0","18446744073709551616",1]){const x=sample();system(x).system.issuers["0"].issued_shares=value;assert.throws(()=>parseSaveSlot(x),/issued_shares/);}
 const bad=raw[0].replace('"company":0','"company":-0');assert.notEqual(bad,raw[0]);assert.throws(()=>parseSaveJson(bad),/company|负零/);
});
