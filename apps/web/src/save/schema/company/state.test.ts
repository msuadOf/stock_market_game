import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { parseSessionCorporateActions } from "./state.ts";
import { SaveSchemaError } from "../primitives.ts";
import { parseSaveJson, parseSaveSlot } from "../../save-schema.ts";
const names = ["generated-company-state-registry.json", "manual-company-state-registry.json", "legacy-company-state.json"] as const;
const historicalRaw = names.map(name => readFileSync(new URL(`../../fixtures/source-b/${name}`, import.meta.url), "utf8"));
// synthetic：仅为未改动的其他 codec 保留历史结构覆盖；补策略和 bindings 都不代表当前 Rust producer 或恢复已验。
function syntheticPolicyProjection(text: string): string {
 const root=JSON.parse(text) as Record<string,any>;
 if(root.setup===undefined||typeof root.setup!=="object"||root.setup===null||Object.hasOwn(root.setup,"runtime_sync_policy"))return text;
 return JSON.stringify({...root,setup:{...root.setup,runtime_sync_policy:"every_tick"}});
}
function syntheticBindingsProjection(text: string): string {
 const root=JSON.parse(text) as Record<string,any>;const company=root.company_state.CompanySystem;
 if(company===undefined)return text;
 assert.equal(Object.hasOwn(company.corporate_actions,"repurchase_execution_bindings"),false,"历史原件应保持旧字段集合");
 return JSON.stringify({...root,company_state:{CompanySystem:{...company,corporate_actions:{...company.corporate_actions,repurchase_execution_bindings:[]}}}});
}
const policyProjectedRaw = historicalRaw.map(syntheticPolicyProjection);
const raw = policyProjectedRaw.map(syntheticBindingsProjection);
function sample(index = 0): Record<string, unknown> { return JSON.parse(raw[index]); }
function system(root: Record<string, unknown>): Record<string, any> { return (root.company_state as Record<string, any>).CompanySystem; }
function binding(root: Record<string, unknown>): Record<string, any> { return system(root).corporate_actions.registries[0].receipts[0].request.scope.SettlementDayMarket.binding; }
for(const [index,name] of names.entries()) test(`历史Rust${name}的synthetic bindings投影对象/raw无损且单一权威tag`, {timeout:10000}, () => {
 const value=sample(index);assert.deepEqual(parseSaveSlot(value),value);assert.deepEqual(parseSaveJson(raw[index]),value);
 assert.deepEqual(Object.keys(parseSaveSlot(value).company_state),[index===2?"LegacyOperations":"CompanySystem"]);
});
for(const [index,name] of names.entries())test(`历史Rust${name}原件缺runtime_sync_policy明确拒绝`,{timeout:10000},()=>{
 assert.throws(()=>parseSaveSlot(JSON.parse(historicalRaw[index])),/runtime_sync_policy/);
 assert.throws(()=>parseSaveJson(historicalRaw[index]),/runtime_sync_policy/);
});
for(const index of [0,1])test(`历史Rust${names[index]}原件缺required bindings明确拒绝`,{timeout:10000},()=>{
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
const nonempty = ["generated-custom-monthly-nonempty.json", "manual-custom-monthly-nonempty.json", "generated-quarterly-nonempty.json", "manual-quarterly-nonempty.json"];
for(const name of nonempty) test(`历史非空SimpleGenerated报告${name}的synthetic bindings投影保留来源频率`, {timeout:10000}, () => {
 const original=readFileSync(new URL(`../../fixtures/source-b/${name}`,import.meta.url),"utf8");
 assert.throws(()=>parseSaveJson(original),/runtime_sync_policy/);
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

for (const [numerator, denominator] of [["1", "0"], ["2", "2"], ["0", "2"]]) test(`ExactDividendTaxAmount拒绝非规范分数${numerator}/${denominator}`, {timeout:10000}, () => {
 const actions=system(sample()).corporate_actions;
 // 仅为窄 codec 单元构造；不作为真实 Rust producer 证据。
 actions.dividend_tax_books=[{operation_seq:0,account:"0",stock:"0",profile:"IndividualPublicMarket",opened_on:"2026-01-01",opening_lots:[],settled_on:"2026-01-01",lots:[],days:[],dividends:[],collections:[{operation_seq:0,event_id:"fraction-codec",day:"2026-01-01",available_cash:"0",collected:"0",remaining_cash:"0",outstanding:{numerator:"1",denominator:"1"},needs_funds:false}],redenominations:[],stock_bonus_bases:[]}];
 assert.deepEqual(parseSessionCorporateActions(actions,"actions"),actions);
 actions.dividend_tax_books[0].collections[0].outstanding={numerator,denominator};
 assert.throws(()=>parseSessionCorporateActions(actions,"actions"),/分母|约分/);
});

for(const amount of ["1701411834604692317316873037158841057.28","-1701411834604692317316873037158841057.28"])test(`AccountingAmount拒绝超Rust i128分值域${amount}`,{timeout:10000},()=>{
 const value=sample();assert.deepEqual(parseSaveSlot(value),value);system(value).system.implementation.state.companies["0"].generation.amounts.revenue=amount;
 assert.throws(()=>parseSaveSlot(value),/revenue.*i128/);
});

for(const last_month of ["1899-12","2200-01"])test(`SimpleFinanceState.last_month拒绝Rust范围外${last_month}`,{timeout:10000},()=>{
 const value=sample();assert.deepEqual(parseSaveSlot(value),value);system(value).system.implementation.state.companies["0"].finance.last_month=last_month;
 assert.throws(()=>parseSaveSlot(value),/last_month.*1900/);
});
test("AccountingAmount保留Rust可解码的正负最大绝对分值",{timeout:10000},()=>{
 for(const amount of ["1701411834604692317316873037158841057.27","-1701411834604692317316873037158841057.27"]){const value=sample();system(value).system.implementation.state.companies["0"].generation.amounts.revenue=amount;assert.deepEqual(parseSaveSlot(value),value);}
});

for(const [key,invalid,boundary] of [["rate_bp",2147483648,2147483647],["loss_carryforward_years",65536,65535]]as const)test(`Bank income_tax_policy.${key}保持Rust整数位宽`,{timeout:10000},()=>{
 const value=JSON.parse(syntheticPolicyProjection(readFileSync(new URL("../../fixtures/source-b/nonempty-legacy-group.json",import.meta.url),"utf8")));const policy=value.setup.company_operations.companies[1].books.Bank.income_tax_policy;
 policy[key]=boundary;assert.deepEqual(parseSaveSlot(value),value);policy[key]=invalid;
 assert.throws(()=>parseSaveSlot(value),new RegExp(`income_tax_policy.${key}`));
});

for(const origin_year of [-2147483649,2147483648])test(`SimpleFinanceState所得税origin_year拒绝i32越界${origin_year}`,{timeout:10000},()=>{
 const value=sample();system(value).system.implementation.state.companies["0"].finance.income_tax_position.loss_pool=[{origin_year,remaining:"1.00"}];
 assert.throws(()=>parseSaveSlot(value),/origin_year/);
});
for(const period of ["1899-12","2200-01"])test(`SimpleFinanceState所得税restatements拒绝期间越界${period}`,{timeout:10000},()=>{
 const value=sample();system(value).system.implementation.state.companies["0"].finance.income_tax_position.restatements={"1":period};
 assert.throws(()=>parseSaveSlot(value),/restatements.*1900/);
});
