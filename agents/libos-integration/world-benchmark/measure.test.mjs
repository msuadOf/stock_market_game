import {test} from 'node:test';
import assert from 'node:assert/strict';
import {measureGame} from './measure.mjs';

test('计时覆盖批量调用及返回转换，业务摘要在计时后保留精确字符串', () => {
  const calls = [];
  const game = {
    diagnostic_game_steps(count) { calls.push(`step:${count}`); return new Map([['to', count]]); },
    diagnostic_game_business_hash() { calls.push('hash'); return '18446744073709551615'; },
  };
  const times = [10, 13, 20, 25];
  const result = measureGame(game, () => { calls.push('clock'); return times.shift(); });
  assert.deepEqual(calls, ['clock', 'step:1000', 'clock', 'clock', 'step:3000', 'clock', 'hash']);
  assert.equal(result.warmMs, 3);
  assert.equal(result.elapsedMs, 5);
  assert.equal(result.businessHash, '18446744073709551615');
});

test('摘要缺失或格式不合法时显式失败', () => {
  for (const hash of [undefined, 42, '', '-1', '01', '18446744073709551616']) {
    const game = {diagnostic_game_steps: () => new Map(), diagnostic_game_business_hash: () => hash};
    assert.throws(() => measureGame(game, () => 1), /业务状态摘要/);
  }
});

test('采样窗口边界通知不计入批量推进耗时', () => {
  const calls = [];
  const game = {diagnostic_game_steps(n) { calls.push(`step:${n}`); return new Map(); }, diagnostic_game_business_hash() { return '1'; }};
  const times = [10, 12, 20, 25];
  measureGame(game, () => { calls.push('clock'); return times.shift(); }, {
    onSampleStart() { calls.push('start'); }, onSampleEnd() { calls.push('end'); },
  });
  assert.deepEqual(calls, ['clock','step:1000','clock','start','clock','step:3000','clock','end']);
});
