import {test} from 'node:test';
import assert from 'node:assert/strict';
import {parseFineWaitUs, initializeDiagnosticWait} from './wait-config.mjs';

test('只接受明确的正整数等待间隔', {timeout:10000}, () => {
  assert.equal(parseFineWaitUs(undefined), 500);
  assert.equal(parseFineWaitUs('50'), 50);
  for (const value of ['', '0', '-1', '1.5', ' 50', '5e2', '1000001']) {
    assert.throws(() => parseFineWaitUs(value));
  }
});

test('准确传入等待参数且初始化错误原样上抛', {timeout:10000}, () => {
  const calls = [];
  initializeDiagnosticWait({init_wasm_wait(...values) { calls.push(values); }}, 50);
  assert.deepEqual(calls, [[500, 50000, 50, 500]]);
  const failure = Error('初始化失败');
  assert.throws(() => initializeDiagnosticWait({init_wasm_wait() { throw failure; }}, 50), error => error === failure);
});
