import assert from 'node:assert/strict';
import test from 'node:test';
import {parseOwnerFlatWithholdingReceipts,parseAccountDividendTaxStatus,parseDividendTaxOutstandingViews} from './dividend-tax.ts';
const flat = () => ({payment_id:'pay1',plan_id:'plan1',account:'18446744073709551615',paid_on:'2030-03-02',gross:'9007199254740993',rate_bp:1000,withheld:'900719925474099'});
const outstanding = () => ({account:'42',stock:'600101',outstanding:{numerator:'9007199254740993',denominator:'7'},latest_collection:null});
const collection = () => ({operation_seq:1,event_id:'col1',day:'2030-03-02',available_cash:'30',collected:'30',remaining_cash:'0',outstanding:{numerator:'70',denominator:'1'},needs_funds:true});
test('本人简税回执无损校验，支持非0owner与0额回执', {timeout:10000}, () => {
  assert.deepEqual(parseOwnerFlatWithholdingReceipts([flat()],flat().account),[flat()]);
  const zero={...flat(),gross:'0',withheld:'0',rate_bp:0}; assert.deepEqual(parseOwnerFlatWithholdingReceipts([zero],zero.account),[zero]);
  for(const patch of [{account:'0'},{account:'18446744073709551616'},{account:'042'},{gross:'-1'},{withheld:'-1'},{withheld:'9223372036854775808'},{rate_bp:10001},{rate_bp:-1},{rate_bp:0.5},{paid_on:'2100-02-29'},{extra:0}]) assert.throws(()=>parseOwnerFlatWithholdingReceipts([{...flat(),...patch}],flat().account));
  assert.throws(()=>parseOwnerFlatWithholdingReceipts([], '18446744073709551616'));
  assert.throws(()=>parseOwnerFlatWithholdingReceipts([], '00'));
});
test('税状态三模式与各身份保留，不猜测未知状态或证券', {timeout:10000}, () => {
  for(const mode of ['FlatWithholding','AShareIndividual','Exempt']) {const v={mode,identity:'NonIndividualPending',stocks:[{stock:'600101',status:'TreatmentNotConfigured'}]};assert.deepEqual(parseAccountDividendTaxStatus(v),v);}
  for(const v of [{mode:'IndividualPublicMarket',identity:'Personal',stocks:[]},{mode:'Exempt',identity:'Unknown',stocks:[]},{mode:'AShareIndividual',identity:'Personal',stocks:[{stock:0,status:'Unknown'}]},{mode:'Exempt',identity:'Personal'},{mode:'Exempt',identity:'Personal',stocks:[],extra:1}]) assert.throws(()=>parseAccountDividendTaxStatus(v));
});
test('未清税额保留约简非负分数与无collection事实，真实回执保持历史值', {timeout:10000}, () => {
  assert.deepEqual(parseDividendTaxOutstandingViews([outstanding()],'42'),[outstanding()]);
  const v={...outstanding(),latest_collection:collection()}; assert.deepEqual(parseDividendTaxOutstandingViews([v],'42'),[v]);
  for(const patch of [{account:'43'},{needs_funds:true},{latest_collection:undefined},{outstanding:{numerator:'2',denominator:'4'}},{outstanding:{numerator:'0',denominator:'2'}},{outstanding:{numerator:'-1',denominator:'1'}},{outstanding:{numerator:'170141183460469231731687303715884105728',denominator:'1'}},{outstanding:{numerator:'1',denominator:'18446744073709551616'}},{outstanding:{numerator:'1',denominator:'0'}},{latest_collection:{...collection(),operation_seq:9007199254740992}},{latest_collection:{...collection(),needs_funds:undefined}},{latest_collection:{...collection(),collected:'-1'}}]) assert.throws(()=>parseDividendTaxOutstandingViews([{...outstanding(),...patch}],'42'));
  assert.throws(()=>parseDividendTaxOutstandingViews([], '01'));
});
