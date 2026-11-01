import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { parseSessionCorporateActions } from "./state.ts";
import { parseSaveJson, parseSaveSlot } from "../../save-schema.ts";
import { raw, syntheticCorrectionOperationsProjection, syntheticPolicyProjection, syntheticBindingsProjection, sample, system } from "./state-test-fixture.ts";
const nonempty = ["generated-custom-monthly-nonempty.json", "manual-custom-monthly-nonempty.json", "generated-quarterly-nonempty.json", "manual-quarterly-nonempty.json"];
for(const name of nonempty) test(`历史非空SimpleGenerated报告${name}的synthetic bindings投影保留来源频率`, {timeout:10000}, () => {
 const original=readFileSync(new URL(`../../fixtures/source-b/${name}`,import.meta.url),"utf8");
 assert.throws(()=>parseSaveJson(original),/report_correction_operations/);
 assert.throws(()=>parseSaveJson(syntheticCorrectionOperationsProjection(original)),/runtime_sync_policy/);
 const policyText=syntheticPolicyProjection(original);
 assert.throws(()=>parseSaveJson(policyText),/repurchase_execution_bindings/);
 const text=syntheticBindingsProjection(policyText);const value=JSON.parse(text);
 assert.ok(value.public_library.reports.length>0);assert.deepEqual(parseSaveSlot(value),value);assert.deepEqual(parseSaveJson(text),value);
 for(const report of value.public_library.reports)assert.equal(report.source,"SimpleGenerated");
 for(const field of ["source","origin","reports"]){const changed=structuredClone(value);delete changed.public_library.reports[0][field];assert.throws(()=>parseSaveSlot(changed),new RegExp(field));}
 const last=value.public_library.reports.at(-1);if(name.includes("monthly")){assert.deepEqual(last.origin.MonthlyDisclosure.schedule,value.setup.report_frequency.Monthly.schedule);assert.deepEqual(last.published_at,{date:"2026-01-06",second_of_day:72000});}else{assert.equal(value.setup.report_frequency,"Quarterly");assert.equal(last.origin.ScheduledDisclosure.kind,"Q1");assert.deepEqual(last.published_at,{date:"2026-04-26",second_of_day:64800});}
 const changed=structuredClone(value);changed.public_library.reports[0].source="UnknownSource";assert.throws(()=>parseSaveSlot(changed),/source/);
 const bad=structuredClone(value);bad.public_library.reports[0].reports.unexpected=0;assert.throws(()=>parseSaveSlot(bad),/unexpected/);
});

const bookCases=[ ["share_splits","ShareSplitBook-approved.json",3], ["stock_distributions","StockDistributionBook-approved.json",3], ["rights_offerings","RightsOfferingBook-approved.json",5], ["issuer_repurchases","IssuerRepurchaseBook-approved.json",4] ] as const;
for(const [field,name,count]of bookCases)test(`真实Rust${name}的${count}个required nullable与variant严格校验`,{timeout:10000},()=>{
 const book=JSON.parse(readFileSync(new URL(`../../fixtures/source-b/${name}`,import.meta.url),"utf8"));const actions=system(sample()).corporate_actions;actions[field]=[book];
 assert.deepEqual(parseSessionCorporateActions(actions,"actions"),actions);const nullable=Object.keys(book).filter(key=>book[key]===null);assert.equal(nullable.length,count);
 for(const key of nullable){const bad=structuredClone(actions);delete bad[field][0][key];assert.throws(()=>parseSessionCorporateActions(bad,"actions"),new RegExp(key));const wrong=structuredClone(actions);wrong[field][0][key]={unexpected:0};assert.throws(()=>parseSessionCorporateActions(wrong,"actions"));}
 const extra=structuredClone(actions);extra[field][0].unexpected=1;assert.throws(()=>parseSessionCorporateActions(extra,"actions"),/unexpected/);const variant=structuredClone(actions);variant[field][0].status="Unknown";assert.throws(()=>parseSessionCorporateActions(variant,"actions"),/status/);
});
test("全部已存在公司数组拒绝sparse与继承元素，raw整数拒绝舍入与负零",{timeout:10000},()=>{
 const root=sample();assert.deepEqual(parseSaveSlot(root),root);const paths:(string|number)[][]=[];
 const walk=(node:unknown,path:(string|number)[]=[])=>{if(Array.isArray(node)){paths.push(path);node.forEach((value,index)=>walk(value,[...path,index]));}else if(node!==null&&typeof node==="object")for(const [key,value]of Object.entries(node))walk(value,[...path,key]);};walk(root.company_state);
 for(const path of paths){const x=sample();let target:any=x.company_state;for(const key of path.slice(0,-1))target=target[key];target[path.at(-1)!]=Array(1);assert.throws(()=>parseSaveSlot(x),/数组元素缺失/);}
 for(const token of ["-0","0.000000000000000000001","1e0","9007199254740993"]){const text=raw[0].replace('"company":0','"company":'+token);assert.throws(()=>parseSaveJson(text),/company|整数/);}
});
test("selected公司权威与公开报告source一致，拒绝已知但来自另一模型的报告",{timeout:10000},()=>{
 const legacy=sample(2);const simple=JSON.parse(syntheticBindingsProjection(syntheticPolicyProjection(readFileSync(new URL("../../fixtures/source-b/generated-quarterly-nonempty.json",import.meta.url),"utf8"))));
 for(const [root,source]of [[legacy,"SimulationAccounting"],[simple,"SimpleGenerated"]]as const){assert.deepEqual(parseSaveSlot(root),root);const reports=(root.public_library as {reports:{source:string}[]}).reports;assert.ok(reports.length>0);for(const report of reports)assert.equal(report.source,source);const bad=structuredClone(root);(bad.public_library as {reports:{source:string}[]}).reports[0].source=source==="SimpleGenerated"?"SimulationAccounting":"SimpleGenerated";assert.throws(()=>parseSaveSlot(bad),/public_library\.reports\[0\]\.source/);}
});
for(const [field,name]of bookCases){const original=JSON.parse(readFileSync(new URL(`../../fixtures/source-b/${name}`,import.meta.url),"utf8"));for(const nullableField of Object.keys(original).filter(key=>original[key]===null))test(`${name}.${nullableField}：显式null接受，missing拒绝`,{timeout:10000},()=>{
 const actions=system(sample()).corporate_actions;actions[field]=[structuredClone(original)];assert.deepEqual(parseSessionCorporateActions(actions,"actions"),actions);
 const missing=structuredClone(actions);delete missing[field][0][nullableField];assert.throws(()=>parseSessionCorporateActions(missing,"actions"),new RegExp(nullableField));
 const explicit=structuredClone(actions);explicit[field][0][nullableField]=null;assert.deepEqual(parseSessionCorporateActions(explicit,"actions"),explicit);
});}
test("真实组合Rust非空Legacy集团完整存档按catalog的u32恢复并无损raw解析",{timeout:10000},()=>{
 const text=readFileSync(new URL("../../fixtures/source-b/nonempty-legacy-group.json",import.meta.url),"utf8"),projectedText=syntheticPolicyProjection(text),value=JSON.parse(projectedText);const groups=value.company_state.LegacyOperations.groups;
 assert.deepEqual(groups,[{root:1,holdings:[{company:0,parent_held_shares:80000}]}]);assert.deepEqual(value.setup.groups,groups);
 assert.deepEqual(parseSaveSlot(value),value);assert.deepEqual(parseSaveJson(projectedText),value);
 const bad=structuredClone(value);bad.company_state.LegacyOperations.groups[0].root="1";assert.throws(()=>parseSaveSlot(bad),/groups\[0\]\.root/);
});
