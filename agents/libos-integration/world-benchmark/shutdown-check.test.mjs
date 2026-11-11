import {test} from 'node:test';
import assert from 'node:assert/strict';
import {validateShutdown} from './shutdown-check.mjs';

function valid() {
  return {ok:true,report:{runtime:{owned_workers:2,joined_workers:2,group_finals:1,desk_finals:1,worker_results:[{worker:'group-0',result:{Ok:null}},{worker:'desk-0',result:{Ok:null}}]}}};
}
test('核验真实组和交易部 Final 与 join', {timeout:10000}, () => { validateShutdown(valid()); });
test('缺失、失败、重复身份或 Final 数量不符均拒绝', {timeout:10000}, () => {
  for (const change of [
    x=>{x.ok=false}, x=>{delete x.report.runtime}, x=>{x.report.runtime.joined_workers--},
    x=>{x.report.runtime.group_finals=0}, x=>{x.report.runtime.desk_finals++},
    x=>{x.report.runtime.worker_results[1].worker='group-0'},
    x=>{x.report.runtime.worker_results[1].result={Err:'故障'}},
    x=>{x.report.runtime.worker_results[1].result={}},
  ]) { const value=valid(); change(value); assert.throws(()=>validateShutdown(value)); }
});
