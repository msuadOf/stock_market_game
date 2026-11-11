import {test} from 'node:test';
import assert from 'node:assert/strict';
import {compareNative} from './native-compare.mjs';

function native() {
  return {seed:42, setupSha256:'a'.repeat(64), warm:{from:0,to:1000,events:6032}, sample:{from:1000,to:4000,events:21821}, elapsedMs:1000, businessHash:'6818911892746368900'};
}
function browser() {
  return {mode:'libos',errors:[],result:{result:{seed:42,setupSha256:'a'.repeat(64),warm:{from:0,to:1000,events:6032},sample:{from:1000,to:4000,events:21821},elapsedMs:4000,businessHash:'6818911892746368900'}}};
}
test('Native 与浏览器窗口、事件数和摘要一致时计算吞吐比', () => {
  assert.equal(compareNative(native(), browser()).nativeToWasm, 4);
});
test('缺失、失败和不一致样本不可作为对比', () => {
  for (const mutate of [
    (n, _b) => { n.seed = 43; },
    (_n, b) => { b.result.result.seed = 43; },
    (n, _b) => { n.setupSha256 = 'b'.repeat(64); },
    (n, _b) => { delete n.setupSha256; },
    (n, _b) => { n.warm.events++; },
    (n, _b) => { n.sample.to--; },
    (n, _b) => { n.businessHash = '0'; },
    (n, _b) => { n.elapsedMs = 0; },
    (_n, b) => { b.errors = ['错误']; },
    (_n, b) => { b.result.failure = ''; },
  ]) {
    const n = native(), b = browser();
    mutate(n, b);
    assert.throws(() => compareNative(n, b));
  }
});
