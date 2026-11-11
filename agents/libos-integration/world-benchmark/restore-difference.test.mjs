import {test} from 'node:test';
import assert from 'node:assert/strict';
import {inspectDifference} from './restore-difference.mjs';

test('仅对首个不同 tick 统计去除 seq 后的事件多重集', () => {
  const left = '[{"OrderAccepted":{"seq":1,"id":10}},{"OrderAccepted":{"seq":2,"id":20}}]';
  const right = '[{"OrderAccepted":{"seq":1,"id":20}},{"OrderAccepted":{"seq":2,"id":10}}]';
  const result = inspectDifference({offset:9,baselineUtf8:left,otherUtf8:right});
  assert.equal(result.offset, 9);
  assert.equal(result.equalEventMultisetIgnoringSeq, true);
  assert.equal(result.firstDifferentEventIndex, 0);
  assert.equal(inspectDifference({offset:9,baselineUtf8:left,otherUtf8:right.replace('20','21')}).equalEventMultisetIgnoringSeq, false);
});

test('格式损坏或重复事件数量变化不被当作相同', () => {
  assert.throws(() => inspectDifference({offset:-1,baselineUtf8:'[]',otherUtf8:'[]'}));
  assert.throws(() => inspectDifference({offset:0,baselineUtf8:'[{}]',otherUtf8:'[]'}));
  assert.throws(() => inspectDifference({offset:0,baselineUtf8:'[{"X":{"v":1.0000000000000001}}]',otherUtf8:'[]'}));
  assert.throws(() => inspectDifference({offset:0,baselineUtf8:'[{"X":{"v":1e-400}}]',otherUtf8:'[]'}));
  assert.throws(() => inspectDifference({offset:0,baselineUtf8:'[{"X":{"v":9007199254740993}}]',otherUtf8:'[]'}));
  assert.throws(() => inspectDifference({offset:0,baselineUtf8:'[{"X":{"v":-0}}]',otherUtf8:'[]'}));
  assert.equal(inspectDifference({offset:0,baselineUtf8:'[{"X":{"v":"1e-400 \\\" 9007199254740993"}}]',otherUtf8:'[{"X":{"v":"1e-400 \\\" 9007199254740993"}}]'}).equalEventMultisetIgnoringSeq, true);
  const left = '[{"X":{"seq":1}},{"X":{"seq":2}}]';
  const right = '[{"X":{"seq":1}}]';
  assert.equal(inspectDifference({offset:0,baselineUtf8:left,otherUtf8:right}).equalEventMultisetIgnoringSeq, false);
});
