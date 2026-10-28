// 共享外层绝对截止；仅复用已审进程树监督，不创建新 Cargo 调度。
import { runBoundedCommand } from '/data1/baiyifan/workplace/stock_market_game/.worktree/main-merge-2026-10-25/scripts/run-with-deadline.mjs';
const deadline = Number(process.argv[2]);
const command = process.argv[3];
const args = process.argv.slice(4);
try {
  if (!Number.isSafeInteger(deadline) || typeof command !== 'string' || command.length === 0) throw new Error('绝对截止或命令非法');
  const remaining = Math.floor(deadline - Date.now());
  if (remaining <= 1000 || remaining > 300000) throw new Error('共享外层截止已到或超过300秒');
  await runBoundedCommand({ command, args, timeoutMs: remaining, cleanupReserveMs: 1000 });
} catch (error) {
  const finalExit = setTimeout(() => process.exit(1), 5);
  process.stderr.write(`${error.message}\n`, () => { clearTimeout(finalExit); process.exit(1); });
}
