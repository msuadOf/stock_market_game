import assert from 'node:assert/strict';
import {test} from 'node:test';
import {parseCpuSnapshot, aggregateDescendants, verifyCpuResult} from './cpu-samples.mjs';

test('只聚合目标进程树并保留多核百分比', {timeout:10000}, () => {
  const rows = parseCpuSnapshot('100 1 2.0 100\n101 100 250.0 400\n102 101 50.0 600\n200 1 999.0 900\n');
  assert.deepEqual(aggregateDescendants(rows, 100), {processes:3, cpuPercent:302, rssKiB:1100});
  assert.throws(() => aggregateDescendants(rows, 999), /根进程/);
  assert.throws(() => parseCpuSnapshot('100 1 非法 100'), /CPU 采样/);
});

test('CPU 结果绑定完整且参数一致的基准文件', {timeout:10000}, () => {
  const result = {mode:'libos',workerMode:'libos',mathMode:'libm',fineWaitUs:50,errors:[],result:{result:{fineWaitUs:50,mathMode:'libm'}}};
  const bytes = Buffer.from(JSON.stringify(result));
  assert.match(verifyCpuResult(bytes, 50, 'libos', 'libm'), /^[a-f0-9]{64}$/);
  assert.throws(() => verifyCpuResult(bytes, 50, 'libos', 'std'), /参数/);
  assert.throws(() => verifyCpuResult(bytes, 50, 'std', 'libm'), /参数/);
  assert.throws(() => verifyCpuResult(bytes, 500, 'libos', 'libm'), /参数/);
  result.errors.push('浏览器错误');
  assert.throws(() => verifyCpuResult(Buffer.from(JSON.stringify(result)), 50, 'libos', 'libm'), /错误/);
});
