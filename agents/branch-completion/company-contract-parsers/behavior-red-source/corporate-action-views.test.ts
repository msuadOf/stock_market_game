import assert from 'node:assert/strict';
import test from 'node:test';
import {parseQueuedRightsSubscriptions,parseRejectedRightsSubscriptions,parseSimplePreferenceRejections} from './corporate-action-views.ts';
const queued=()=>({event_id:'r1',account:'42',requested_shares:'9007199254740993',submitted_on:'2030-03-02'});
const rejected=()=>({...queued(),rejected_on:'2030-03-03',reason:'公开配售剩余额度耗尽'});
test('认购排队与拒绝均校验显式非0owner，不混淆受理与支付', {timeout:10000}, () => {
  assert.deepEqual(parseQueuedRightsSubscriptions([queued()],'42'),[queued()]); assert.deepEqual(parseRejectedRightsSubscriptions([rejected()],'42'),[rejected()]);
  for(const patch of [{account:'0'},{account:'18446744073709551616'},{account:'042'},{requested_shares:'0'},{requested_shares:'01'},{requested_shares:'18446744073709551616'},{event_id:''},{submitted_on:'2030-02-29'},{extra:1}]) assert.throws(()=>parseQueuedRightsSubscriptions([{...queued(),...patch}],'42'));
  for(const patch of [{account:'43'},{reason:' '},{rejected_on:'2030-03-01'},{rejected_on:undefined}]) assert.throws(()=>parseRejectedRightsSubscriptions([{...rejected(),...patch}],'42'));
  for(const parser of [parseQueuedRightsSubscriptions,parseRejectedRightsSubscriptions]) assert.throws(()=>parser([], '00'));
});
test('偏好拒绝台账使用u32 CompanyId与现有两类proposal', {timeout:10000}, () => {
  const row={company:4294967295,evaluated_on:'2030-03-31',kind:'CashDividend',detail:'可分配利润不足'};
  assert.deepEqual(parseSimplePreferenceRejections([row]),[row]);
  assert.deepEqual(parseSimplePreferenceRejections([{...row,company:0,kind:'StockDistribution'}]),[{...row,company:0,kind:'StockDistribution'}]);
  for(const patch of [{company:'C-1'},{company:4294967296},{company:-1},{company:0.5},{detail:''},{kind:'IssuerRepurchase'},{evaluated_on:'2030-04-31'},{extra:true}]) assert.throws(()=>parseSimplePreferenceRejections([{...row,...patch}]));
});
