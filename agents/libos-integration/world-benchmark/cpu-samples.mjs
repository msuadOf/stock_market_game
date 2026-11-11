import {createHash} from 'node:crypto';

/** 解析 ps 的 pid、ppid、CPU 百分比和 RSS；不按进程名称猜归属。 */
export function parseCpuSnapshot(text) {
  if (typeof text !== 'string' || text.trim() === '') throw Error('CPU 采样为空');
  const seen = new Set();
  return text.trim().split('\n').map(line => {
    const values = line.trim().split(/\s+/);
    if (values.length !== 4 || values.some(value => !/^\d+(?:\.\d+)?$/.test(value))) throw Error('CPU 采样格式错误');
    const [pid, ppid, cpuPercent, rssKiB] = values.map(Number);
    if (![pid, ppid, rssKiB].every(Number.isSafeInteger) || pid <= 0 || seen.has(pid) || !Number.isFinite(cpuPercent)) throw Error('CPU 采样数值无效');
    seen.add(pid);
    return {pid, ppid, cpuPercent, rssKiB};
  });
}

/** 校验采样对应的基准结果并返回文件字节摘要。 */
export function verifyCpuResult(bytes, fineWaitUs, workerMode, mathMode) {
  const result = JSON.parse(bytes.toString('utf8'));
  if (!['std', 'libos'].includes(workerMode) || !['std', 'libm'].includes(mathMode) || result.mode !== 'libos' || result.workerMode !== workerMode || result.mathMode !== mathMode || result.result?.result?.mathMode !== mathMode || result.fineWaitUs !== fineWaitUs || result.result?.result?.fineWaitUs !== fineWaitUs) throw Error('CPU 与基准参数不一致');
  if (!Array.isArray(result.errors) || result.errors.length !== 0 || result.result.failure !== undefined) throw Error('基准包含错误');
  return createHash('sha256').update(bytes).digest('hex');
}

export function aggregateDescendants(rows, rootPid) {
  if (!rows.some(row => row.pid === rootPid)) throw Error('CPU 采样缺少根进程');
  const members = new Set([rootPid]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const row of rows) if (members.has(row.ppid) && !members.has(row.pid)) { members.add(row.pid); changed = true; }
  }
  return rows.filter(row => members.has(row.pid)).reduce((sum, row) => ({
    processes: sum.processes + 1, cpuPercent: sum.cpuPercent + row.cpuPercent, rssKiB: sum.rssKiB + row.rssKiB,
  }), {processes:0, cpuPercent:0, rssKiB:0});
}
