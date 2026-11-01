import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { parseSessionCorporateActions } from "./state.ts";
import { parseSaveSlot } from "../../save-schema.ts";
import { syntheticPolicyProjection, sample, system } from "./state-test-fixture.ts";
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

test("REG pending_opening 显式可空且全部嵌套字段严格必填（synthetic）", {timeout:10000}, () => {
 const actions=system(sample()).corporate_actions;
 const registry=actions.registries[0];
 assert.equal(registry.pending_opening,null);
 const missing=structuredClone(actions);delete missing.registries[0].pending_opening;
 assert.throws(()=>parseSessionCorporateActions(missing,"actions"),/pending_opening/);
 // 此输入只验证 wire 形状；真实冻结来源、数量与收盘消费由 Rust SaveSlot 校验。
 registry.pending_opening={
  predecessor:{settled_on:registry.settled_on,receipt_count:String(registry.receipts.length),unit:"Initial"},
  opening:{reference:{movement_event_id:"stock-distribution:synthetic",registration_event_id:"synthetic",
   source_lots:[["original"]],from_unit:"Initial",to_unit:"Initial"},
   request:{event_id:"stock-distribution:synthetic",day:"2026-01-07",scope:{NonTradingTransfer:{basis:"合成结构测试"}},changes:[]}}
 };
 assert.deepEqual(parseSessionCorporateActions(actions,"actions"),actions);
 for(const path of [[],["predecessor"],["opening"],["opening","reference"],["opening","request"]]){
  let node=registry.pending_opening;for(const key of path)node=node[key];
  for(const key of Object.keys(node)){
   const bad=structuredClone(actions);let target=bad.registries[0].pending_opening;for(const part of path)target=target[part];delete target[key];
   assert.throws(()=>parseSessionCorporateActions(bad,"actions"),new RegExp(key));
  }
  const extra=structuredClone(actions);let target=extra.registries[0].pending_opening;for(const part of path)target=target[part];target.unexpected=0;
  assert.throws(()=>parseSessionCorporateActions(extra,"actions"),/unexpected/);
 }
});
