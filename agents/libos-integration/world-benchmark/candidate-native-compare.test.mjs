import {test} from 'node:test';
import assert from 'node:assert/strict';
import {compareCandidate} from './candidate-native-compare.mjs';
import {STEP_SEGMENT_NAMES} from './step-profile.mjs';

const ids = ['310'];
const trace = () => ({accountRoster:['0','310'],observationTrace:[['310','3ff0000000000000','240','bff0000000000000','3fe0000000000000','3fe0000000000000']],candidateTrace:[['310','bff0000000000000','3fe0000000000000','3fe0000000000000']]});
function native() { return {mathMode:'libm',stepProfile:null,binarySha256:'a'.repeat(64),setupSha256:'b'.repeat(64),seed:42,warm:{from:0,to:1000,events:12},sample:{from:1000,to:4000,events:34},elapsedMs:1000,businessHash:'123',...trace()}; }
function wasm() { return {mode:'libos',workerMode:'libos',mathMode:'libm',profileMode:'0',wasmHash:'c'.repeat(64),glueHash:'d'.repeat(64),errors:[],result:{result:{mathMode:'libm',setupSha256:'b'.repeat(64),seed:42,warm:{from:0,to:1000,events:12},sample:{from:1000,to:4000,events:34},elapsedMs:1500,businessHash:'123',...trace()}}}; }
test('相同候选模式、输入及业务摘要才能比较', {timeout:10000}, () => { assert.equal(compareCandidate(native(),wasm(),ids).nativeToWasm, 1.5); });
test('模式、产物、输入或业务不符时拒绝', {timeout:10000}, () => {
  for (const change of [
    (n,_w)=>{n.mathMode='std'}, (_n,w)=>{w.workerMode='std'}, (_n,w)=>{w.mathMode='std'},
    (_n,w)=>{w.result.result.mathMode='std'}, (n,_w)=>{n.binarySha256='bad'}, (_n,w)=>{w.wasmHash='bad'},
    (n,_w)=>{n.observationTrace[0][4]='bad'}, (_n,w)=>{w.result.result.candidateTrace=[]},
    (_n,w)=>{w.result.result.observationTrace[0][4]='3fd0000000000000';w.result.result.candidateTrace[0][2]='3fd0000000000000'},
    (_n,w)=>{w.result.result.observationTrace[0][1]='4000000000000000'},
    (_n,w)=>{w.profileMode='1'},
    (_n,w)=>{w.profileMode=null},
    (n,w)=>{n.observationTrace=[];w.result.result.observationTrace=[]},
    (n,w)=>{n.observationTrace.push(n.observationTrace[0]);n.candidateTrace.push(n.candidateTrace[0]);w.result.result.observationTrace.push(w.result.result.observationTrace[0]);w.result.result.candidateTrace.push(w.result.result.candidateTrace[0])},
    (n,w)=>{n.observationTrace[0][0]='none';w.result.result.observationTrace[0][0]='none'},
    (n,w)=>{n.accountRoster=['0'];w.result.result.accountRoster=['0']},
    (n,_w)=>{n.setupSha256='f'.repeat(64)}, (_n,w)=>{w.result.result.businessHash='124'},
    (_n,w)=>{w.errors.push('故障')}, (_n,w)=>{w.result.failure='故障'},
  ]) {const n=native(),w=wasm();change(n,w);assert.throws(()=>compareCandidate(n,w,ids));}
});
test('画像样本须在两端保留完整且非零的有效账本', {timeout:10000}, () => {
  const n=native(), w=wasm();
  const ledger=()=>STEP_SEGMENT_NAMES.map(name=>[name,['world_run','world_converge','world_seal','world_install','converge_receive'].includes(name)?'1':'0',['world_run','world_converge','world_seal','world_install','converge_receive'].includes(name)?3000:0]);
  n.stepProfile=ledger(); w.profileMode='1'; w.result.result.stepProfile=ledger();
  assert.equal(compareCandidate(n,w,ids).businessHash,'123');
  w.result.result.stepProfile[0][1]='0';
  assert.throws(()=>compareCandidate(n,w,ids));
});
