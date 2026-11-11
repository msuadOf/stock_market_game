import {test} from 'node:test';
import assert from 'node:assert/strict';
import {compareWorkerSamples} from './worker-compare.mjs';

function sample(workerMode) {
  return {mode:'libos',workerMode,mathMode:'std',fineWaitUs:500,wasmHash:'a'.repeat(64),glueHash:'b'.repeat(64),errors:[],result:{result:{mathMode:'std',seed:42,setupSha256:'c'.repeat(64),fineWaitUs:500,warm:{from:0,to:1000,events:12},sample:{from:1000,to:4000,events:34},businessHash:'123',elapsedMs:1000}}};
}

test('比较相同输入及上下行模式', {timeout:10000}, () => {
  assert.equal(compareWorkerSamples(sample('std'), sample('libos')).businessHash, '123');
});

test('不同模式、输入、失败或输出不得视为可比较样本', {timeout:10000}, () => {
  for (const mutate of [
    x=>{x.mode='std'}, x=>{x.workerMode='std'}, x=>{x.fineWaitUs=50},
    x=>{x.result.result.fineWaitUs=50}, x=>{x.result.result.seed=43},
    x=>{x.result.result.setupSha256='d'.repeat(64)}, x=>{x.result.result.sample.events++},
    x=>{x.result.result.businessHash='124'}, x=>{x.errors.push('fault')},
    x=>{x.mathMode='libm';x.result.result.mathMode='libm'}, x=>{x.result.result.mathMode='libm'},
    x=>{x.result.failure='fault'}, x=>{x.result.result.elapsedMs=0}, x=>{x.wasmHash='wrong'},
  ]) {
    const candidate=sample('libos'); mutate(candidate);
    assert.throws(()=>compareWorkerSamples(sample('std'),candidate));
  }
});
