// 进程外监督整个批次；最后一秒只用于终止和清理。
import { performance } from 'node:perf_hooks';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { runBoundedCommand } from '../../../scripts/run-with-deadline.mjs';

const [artifact, output] = process.argv.slice(2);
if (process.argv.length !== 4) throw new Error('必须提供 WASM package 和新的证据目录');
const start = performance.now();
const args = [path.resolve('scripts/run-long-validation.mjs'), '299000', '--', process.execPath,
  path.resolve('agents/worktree-migration-20261106/calendar-v11-draft/run-host-business.mjs'), artifact, output];
await runBoundedCommand({ command: process.execPath, args, timeoutMs: 300000, cleanupReserveMs: 1000 });
const elapsedMs = performance.now() - start;
if (elapsedMs >= 299000) throw new Error('执行及清理未在最后一秒前完成');
await writeFile(path.join(output, 'supervisor.json'), JSON.stringify({ command: process.execPath, args, elapsedMs, exitCode: 0, deadlineMs: 300000, cleanupReserveMs: 1000 }, null, 2), { flag: 'wx' });
