// 沿用已审W2外监督模式；在唯一原始deadline内确认整树退出，再恢复本人源码时间和清理。
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { runBoundedCommand } from '/data1/<USER>/workplace/stock_market_game/.worktree/main-merge-2026-10-25/scripts/run-with-deadline.mjs';

const directory = path.dirname(fileURLToPath(import.meta.url));
const [anchorText, python] = process.argv.slice(2);
const anchor = Number(anchorText);
const parentArgv = (await fs.readFile(`/proc/${process.ppid}/cmdline`, 'utf8')).split(String.fromCharCode(0)).filter(Boolean);
if (!Number.isSafeInteger(anchor) || !python || parentArgv[1] !== path.join(directory, 'absolute-supervisor.mjs')
    || parentArgv[2] !== String(anchor + 300000) || Date.now() < anchor || Date.now() >= anchor + 296000) {
  throw new Error('必须由同一原始300秒绝对监督启动');
}
const owner = '/tmp/stock-market-game-validation-2026-10-27-1311109/engine-r4-payment-red-2026-10-29-01';
const scratch = path.join(owner, 'scratch');
const receiptHandle = await fs.open(path.join(directory, 'supervisor-receipt.json'), 'wx');
const receipt = { actualArgv: process.argv, parentArgv, anchorUnixMs: anchor, workDeadlineUnixMs: anchor + 296000,
  treeExitDeadlineUnixMs: anchor + 297000, sourceRestoreDeadlineUnixMs: anchor + 298000,
  publicationDeadlineUnixMs: anchor + 299000, absoluteDeadlineUnixMs: anchor + 300000, owner, scratch };
const remaining = (end, maximum) => {
  const value = Math.min(maximum, Math.floor(anchor + end - Date.now()));
  if (value <= 1) throw new Error('原始共享deadline预算耗尽');
  return value;
};
const optionalJson = async file => fs.readFile(file, 'utf8').then(JSON.parse, error => {
  if (error.code === 'ENOENT') return undefined;
  throw error;
});
const sameProcessAlive = async (pid, birth) => {
  try {
    const stat = await fs.readFile(`/proc/${pid}/stat`, 'utf8');
    const fields = stat.slice(stat.lastIndexOf(')') + 2).trim().split(' ').filter(Boolean);
    return fields[19] === String(birth) && fields[0] !== 'Z';
  } catch (error) { if (['ENOENT', 'ESRCH'].includes(error.code)) return false; throw error; }
};
let failed = false;
try {
  await runBoundedCommand({ command: python,
    args: [path.join(directory, 'subreaper-owner.py'), String(anchor), python],
    cwd: '/data1/<USER>/workplace/stock_market_game/.worktree/d3-p1-integration-2026-10-28',
    timeoutMs: remaining(297000, 297000), cleanupReserveMs: 100, captureOutput: true,
    onStdout: chunk => process.stdout.write(chunk), onStderr: chunk => process.stderr.write(chunk) });
  receipt.workerCompleted = true;
  receipt.boundedTreeExitConfirmed = true;
} catch (error) {
  failed = true;
  receipt.error = error.message;
  receipt.boundedTreeExitConfirmed = (error.message.startsWith('bounded command exited with ')
    || error.message.includes('process tree termination was requested and direct child close confirmed'))
    && !error.message.includes('process tree termination remains unconfirmed');
} finally {
  try {
    const claim = await optionalJson(path.join(directory, 'exclusive-owner-claim.json'));
    const reaper = await optionalJson(path.join(directory, 'subreaper-owner-receipt.json'));
    if (!reaper || reaper.supervisorPid !== process.pid || reaper.clean !== true || reaper.echild !== true
        || reaper.completedAtUnixMs >= anchor + 297000) {
      throw new Error('subreaper未在原始297秒内确认ECHILD，保留scratch与源码时间receipt');
    }
    receipt.subreaperOwner = reaper;
    if (!claim || claim.wrapperPid !== reaper.ownerPid || claim.owner !== owner || claim.scratch !== scratch) {
      throw new Error('本批排他owner归属未确认，保留scratch与源码时间receipt');
    }
    if (receipt.boundedTreeExitConfirmed !== true
        || await sameProcessAlive(claim.pythonPid, claim.pythonBirthTicks)) {
      throw new Error('整树或同birth Python退出未确认，拒绝恢复源码和清理');
    }
    const monitor = await optionalJson(path.join(directory, 'wasm-build-monitor.json'));
    if (monitor && await sameProcessAlive(monitor.pid, monitor.starttime)) {
      throw new Error('同birth wasm-pack仍存活，拒绝并发恢复源码');
    }
    receipt.exclusiveOwnerClaim = claim;
    const sourceTimes = path.join(directory, 'owned-source-times.json');
    if (await optionalJson(sourceTimes)) {
      await runBoundedCommand({ command: python, args: [path.join(directory, 'owned-native-source.py'), 'restore', sourceTimes],
        timeoutMs: remaining(298000, 1000), cleanupReserveMs: 100, captureOutput: true });
      receipt.ownedSourceTimesRestored = true;
    }
    await runBoundedCommand({ command: 'rm', args: ['-r', '--', scratch],
      timeoutMs: remaining(299000, 1000), cleanupReserveMs: 100, captureOutput: true });
    receipt.scratchCleaned = true;
  } catch (error) {
    failed = true;
    receipt.cleanupError = error.message;
    receipt.scratchPreserved = true;
  }
  receipt.wallMs = Date.now() - anchor;
  if (Date.now() >= anchor + 299000) { failed = true; receipt.deadlineError = '清理或证据发布超过原始299秒'; }
  receipt.failed = failed;
  await receiptHandle.writeFile(`${JSON.stringify(receipt, null, 2)}\n`);
  await receiptHandle.close();
  if (Date.now() >= anchor + 299000) failed = true;
}
if (failed) process.exitCode = 1;
