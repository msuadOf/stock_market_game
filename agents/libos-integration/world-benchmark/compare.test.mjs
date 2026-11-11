import {test} from 'node:test';
import assert from 'node:assert/strict';
import {compareSamples} from './compare.mjs';

function sample(mode) {
  return {mode, errors:[], result:{result:{warm:{from:0,to:1000,events:12},sample:{from:1000,to:4000,events:34},businessHash:'18446744073709551615'}}};
}

test('核对模式、推进窗口、事件数和业务摘要', () => {
  assert.equal(compareSamples(sample('std'), sample('libos')).businessHash, '18446744073709551615');
});

test('模式、窗口、事件或摘要不一致时失败', () => {
  for (const mutate of [
    value => { value.mode = 'std'; },
    value => { value.result.result.sample.to = 3999; },
    value => { value.result.result.sample.events++; },
    value => { value.result.result.businessHash = '5'; },
    value => { delete value.result.result.businessHash; },
    value => { value.errors.push('pageerror'); },
    value => { delete value.errors; },
    value => { value.result.failure = {message:'failed'}; },
    value => { value.result.failure = ''; },
    value => { value.result.failure = null; },
  ]) {
    const candidate = sample('libos');
    mutate(candidate);
    assert.throws(() => compareSamples(sample('std'), candidate));
  }
});
