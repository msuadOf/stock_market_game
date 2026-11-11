import assert from 'node:assert/strict';

export function validateShutdown(shutdown) {
  assert.equal(shutdown?.ok, true, `真实 Session shutdown 失败：${JSON.stringify(shutdown)}`);
  const report = shutdown.report?.runtime;
  assert.ok(report && Number.isSafeInteger(report.owned_workers) && report.owned_workers > 0, 'shutdown 必须拥有真实 worker');
  assert.equal(report.joined_workers, report.owned_workers, '必须收获全部 owned worker 的 join 结果');
  assert.ok(Array.isArray(report.worker_results), '必须保留各 worker 结果');
  assert.equal(report.worker_results.length, report.joined_workers, '必须保留各 worker 结果');
  assert.ok(report.worker_results.every(worker => typeof worker.worker === 'string' && worker.worker.length > 0), 'worker 身份缺失');
  assert.equal(new Set(report.worker_results.map(worker => worker.worker)).size, report.joined_workers, 'worker 身份不得重复');
  assert.ok(Number.isSafeInteger(report.group_finals) && report.group_finals > 0 && Number.isSafeInteger(report.desk_finals) && report.desk_finals > 0, '必须上交组和交易部 Final');
  assert.equal(report.group_finals + report.desk_finals, report.owned_workers, 'Final 必须覆盖全部 owned worker');
  for (const worker of report.worker_results) assert.deepEqual(worker.result, {Ok:null}, '所有 worker 必须正常退出');
}
