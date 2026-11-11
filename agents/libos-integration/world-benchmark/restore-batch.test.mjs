import {test} from 'node:test';
import assert from 'node:assert/strict';
import {executeBatch} from './restore-batch.mjs';

test('按指定顺序执行全部成功样本', async () => {
  const calls = [];
  const result = await executeBatch(async job => {
    calls.push(job.order);
  }, () => 0, 280000);
  assert.deepEqual(calls, ['old-first','candidate-first','old-first']);
  assert.deepEqual(result.map(item => item.status), ['passed','passed','passed']);
});

test('基础设施失败或超时后停止新样本', async () => {
  let calls = 0;
  const result = await executeBatch(async () => { calls++; throw Error('超时未确认退出'); }, () => 0, 280000);
  assert.equal(calls, 1);
  assert.equal(result[0].status, 'fatal');
});

test('即使断言差异也停止，不能用旧证据覆盖超时或清理错误', async () => {
  let calls = 0;
  const result = await executeBatch(async () => { calls++; throw new assert.AssertionError({message:'模拟差异'}); }, () => 0, 280000);
  assert.equal(calls, 1);
  assert.equal(result[0].status, 'fatal');
});

test('不在共享 deadline 外启动新样本', async () => {
  let calls = 0;
  const result = await executeBatch(async () => { calls++; }, () => 280000, 280000);
  assert.equal(result[0].status, 'fatal');
  assert.match(result[0].error, /deadline/);
  assert.equal(calls, 0);
});
