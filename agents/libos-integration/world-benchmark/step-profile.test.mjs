import {test} from 'node:test';
import assert from 'node:assert/strict';
import {validateStepProfile, validateRequestedProfile, STEP_SEGMENT_NAMES} from './step-profile.mjs';

const valid = () => STEP_SEGMENT_NAMES.map(name => [name,['world_run','world_converge','world_seal','world_install','converge_receive'].includes(name) ? '1200000' : '0',['world_run','world_converge','world_seal','world_install','converge_receive'].includes(name) ? 3000 : 0]);
test('按名称核验精确纳秒与执行次数', {timeout:10000}, () => {
  const result = validateStepProfile(valid(), 3000);
  assert.deepEqual(result.get('world_run'), {ns:1200000n,count:3000});
  assert.deepEqual(result.get('converge_receive'), {ns:1200000n,count:3000});
});
test('Native 请求画像时发布前必须验证账本', {timeout:10000}, () => {
  validateRequestedProfile({stepProfile:valid()}, '1');
  assert.throws(()=>validateRequestedProfile({stepProfile:null}, '1'));
  assert.throws(()=>validateRequestedProfile({stepProfile:valid()}, '2'));
});
test('拒绝缺失、重复、负值及错误次数', {timeout:10000}, () => {
  for (const change of [
    x=>x.pop(), x=>x.push(x[0]), x=>{x[0][1]='-1'}, x=>{x[0][1]='0'}, x=>{x[0][2]=2999},
    x=>{x[0][0]='unknown'}, x=>{x[1][1]='1.5'}, x=>{x[2][2]=-1},
    x=>{x.find(row=>row[0]==='world_seal')[2]=0},
    x=>{x.find(row=>row[0]==='converge_receive')[2]=2999},
    x=>{x.find(row=>row[0]==='converge_receive')[1]='0'},
  ]) { const x=valid();change(x);assert.throws(()=>validateStepProfile(x,3000)); }
});
