// 只终止已观察且birth仍相同的owned树；复用项目已审terminateTree，不按裸PID清理。
import fs from 'node:fs';
import { terminateTree } from '/data1/<USER>/workplace/stock_market_game/.worktree/main-merge-2026-10-25/scripts/run-with-deadline.mjs';

const [inputPath, outputPath, deadlineText] = process.argv.slice(2);
if (process.argv.length !== 5) throw new Error('需要owned输入、排他输出和绝对截止');
const deadline = Number(deadlineText);
if (!Number.isSafeInteger(deadline) || deadline <= Date.now()) throw new Error('owned终止截止已耗尽');
const rows = JSON.parse(fs.readFileSync(inputPath, 'utf8'));

function current(row) {
  let raw;
  try { raw = fs.readFileSync(`/proc/${row.pid}/stat`, 'utf8'); }
  catch (error) { if (['ENOENT', 'ESRCH'].includes(error.code)) return undefined; throw error; }
  const fields = raw.slice(raw.lastIndexOf(')') + 2).trim().split(/\s+/);
  if (fields[19] !== row.birth) return undefined;
  return { pid: row.pid, birth: fields[19], state: fields[0] };
}

const result = { observed: rows.length, terminatedRoots: [], alive: [], status: '失败' };
try {
  for (const row of rows) {
    const live = current(row);
    if (live === undefined || ['Z', 'X'].includes(live.state)) continue;
    await terminateTree({ pid: row.pid, birth: row.birth }, false, deadline);
    result.terminatedRoots.push({ pid: row.pid, birth: row.birth });
  }
  result.alive = rows.map(current).filter(row => row !== undefined && !['Z', 'X'].includes(row.state));
  if (result.alive.length !== 0) throw new Error('owned终止后仍有同birth活进程');
  if (Date.now() >= deadline) throw new Error('owned终止确认未在共享截止前完成');
  result.status = '已观察同birth owned全部退出';
} catch (error) {
  result.error = String(error.stack);
  result.alive = rows.map(current).filter(row => row !== undefined && !['Z', 'X'].includes(row.state));
  process.exitCode = 1;
} finally {
  fs.writeFileSync(outputPath, JSON.stringify(result, null, 2) + '\n', { flag: 'wx' });
}
