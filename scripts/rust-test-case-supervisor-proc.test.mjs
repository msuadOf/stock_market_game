import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { readFileSync } from "node:fs";
import { mkdir, mkdtemp, rm } from "node:fs/promises";
import path from "node:path";
import { createInterface } from "node:readline";
import { it } from "node:test";
import { createCompletionCleanup } from "./rust-test-case-supervisor.mjs";

const monotonicMs = () => Number(process.hrtime.bigint()) / 1e6;

async function processWithUnreadableEnvironment(mode) {
  const root = path.resolve(".tmp/deadline-hardgate-tests");
  await mkdir(root, { recursive: true });
  const dir = await mkdtemp(path.join(root, `proc-${mode}-`));
  const ready = path.join(dir, "child-ready");
  const script = `import os,time,json,sys,pathlib,signal,ctypes
mode=sys.argv[1];ready=pathlib.Path(sys.argv[2]);pid=os.fork()
if pid==0:
 if mode=='live':
  if ctypes.CDLL(None).prctl(4,0,0,0,0)!=0:raise RuntimeError('PR_SET_DUMPABLE failed')
  ready.write_text('non-dumpable')
  while True:time.sleep(.01)
 os._exit(0)
try:
 deadline=time.monotonic()+3
 while True:
  stat=pathlib.Path(f'/proc/{pid}/stat').read_text();fields=stat[stat.rindex(')')+2:].split()
  if (mode=='zombie' and fields[0]=='Z') or (mode=='live' and ready.exists()):break
  if time.monotonic()>=deadline:raise RuntimeError('fixture child not ready')
  time.sleep(.001)
 record={'pid':pid,'state':fields[0],'ppid':int(fields[1]),'pgrp':int(fields[2]),'starttime':fields[19]}
 try:record['environment_bytes']=len(pathlib.Path(f'/proc/{pid}/environ').read_bytes())
 except PermissionError as error:record['environ_errno']=error.errno
 print(json.dumps(record),flush=True);sys.stdin.read(1)
finally:
 try:os.kill(pid,signal.SIGKILL)
 except ProcessLookupError:pass
 os.waitpid(pid,0)
`;
  const guardian = spawn("python3", ["-c", script, mode, ready], { stdio: ["pipe", "pipe", "pipe"] });
  const lines = createInterface({ input: guardian.stdout });
  let stderr = "";
  guardian.stderr.on("data", (chunk) => { stderr += chunk; });
  const closed = once(guardian, "close");
  try {
    const [line] = await Promise.race([once(lines, "line"), closed.then(([code]) => { throw new Error(`proc fixture exited ${code}: ${stderr}`); })]);
    return { guardian, record: JSON.parse(line), stop: async () => {
      guardian.stdin.end("q"); await closed; lines.close(); await rm(dir, { recursive: true, force: true });
    } };
  } catch (error) {
    guardian.stdin.end("q"); await closed; lines.close(); await rm(dir, { recursive: true, force: true }); throw error;
  }
}

it("真实 Z 态 sibling 的 environ EACCES 不应误报活 owned 不可确认", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  const fixture = await processWithUnreadableEnvironment("zombie");
  try {
    assert.equal(fixture.record.state, "Z");
    assert.equal(fixture.record.environ_errno, 13);
    assert.equal(fixture.record.ppid, fixture.guardian.pid);
    const cleanup = createCompletionCleanup(fixture.guardian.pid);
    await assert.doesNotReject(cleanup({ token: `${process.pid}:completed-peer`, pid: fixture.guardian.pid, total: monotonicMs() + 1000 }));
    const stat = readFileSync(`/proc/${fixture.record.pid}/stat`, "utf8");
    const fields = stat.slice(stat.lastIndexOf(")") + 2).split(" ");
    assert.equal(fields[0], "Z"); assert.equal(fields[19], fixture.record.starttime);
  } finally { await fixture.stop(); }
});

it("真实活 owned 的 non-dumpable environ EACCES 仍必须显式失败", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  const fixture = await processWithUnreadableEnvironment("live");
  try {
    assert.notEqual(fixture.record.state, "Z");
    assert.equal(fixture.record.environ_errno, 13);
    const cleanup = createCompletionCleanup(fixture.guardian.pid);
    await assert.rejects(cleanup({ token: `${process.pid}:completed-peer`, pid: fixture.guardian.pid, total: monotonicMs() + 1000 }),
      new RegExp(`cannot confirm owned process ${fixture.record.pid}.*EACCES`));
    assert.ok(readFileSync(`/proc/${fixture.record.pid}/stat`, "utf8").includes(String(fixture.record.pid)));
  } finally { await fixture.stop(); }
});

function frozenStat(pid, { state = "R", ppid = 777, group = 777, birth = "100" } = {}) {
  return `${pid} (frozen process) ${[state, ppid, group, ...Array(16).fill("0"), birth, "0"].join(" ")}\n`;
}

it("environ 其它变量 value 中的伪 token 不能取得 owned 身份", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  let kills = 0;
  const cleanup = createCompletionCleanup(777, {
    listProcesses: async () => ["301"],
    readEnvironment: async () => Buffer.from("OTHER=STOCK_GAME_RUST_CASE_TOKEN=active\0"),
    readProcessStat: async (pid) => frozenStat(pid),
    terminate: async () => { kills += 1; },
  });
  await cleanup({ token: "active", pid: 301, total: monotonicMs() + 1000 });
  assert.equal(kills, 0, "不能把其它变量 value 内的 key 当作环境项");
});

it("旧 environ token 与新 PID birth 不得拼接成 owned，更不能误杀", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  let environmentReads = 0;
  let kills = 0;
  const cleanup = createCompletionCleanup(777, {
    listProcesses: async () => ["302"],
    readEnvironment: async () => { environmentReads += 1; return Buffer.from("STOCK_GAME_RUST_CASE_TOKEN=active\0"); },
    readProcessStat: async (pid) => frozenStat(pid, { birth: environmentReads >= 2 ? "200" : "100" }),
    terminate: async () => { kills += 1; },
  });
  await assert.rejects(cleanup({ token: "active", pid: 302, total: monotonicMs() + 1000 }), /identity changed/);
  assert.equal(kills, 0);
});

it("冻结退出竞态只跳过确证 Z 的 PID，仍清理同轮活 token 后代", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  const denied = new Error("frozen environ permission denied"); denied.code = "EACCES";
  let deniedEnvironmentReads = 0;
  const killed = [];
  const cleanup = createCompletionCleanup(777, {
    listProcesses: async () => ["303", "304"],
    readEnvironment: async (pid) => { if (pid === 303 && ++deniedEnvironmentReads >= 2) throw denied; return Buffer.from("STOCK_GAME_RUST_CASE_TOKEN=active\0"); },
    readProcessStat: async (pid) => frozenStat(pid, { state: pid === 303 && deniedEnvironmentReads >= 2 ? "Z" : "R" }),
    terminate: async ({ pid }) => { killed.push(pid); },
  });
  await assert.doesNotReject(cleanup({ token: "active", pid: 304, total: monotonicMs() + 1000 }));
  assert.deepEqual(killed, [304]);
});

it("初筛旧 token 后新 PID 已无该 token，不能从初筛取得 owned 身份", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  let environmentReads = 0;
  let kills = 0;
  const cleanup = createCompletionCleanup(777, {
    listProcesses: async () => ["305"],
    readEnvironment: async () => Buffer.from(++environmentReads === 1 ? "STOCK_GAME_RUST_CASE_TOKEN=active\0" : "OTHER=unrelated\0"),
    readProcessStat: async (pid) => frozenStat(pid, { ppid: 999, group: 999, birth: "200" }),
    terminate: async () => { kills += 1; },
  });
  await cleanup({ token: "active", pid: 305, total: monotonicMs() + 1000 });
  assert.equal(kills, 0, "初筛 token 不能替代同 birth 内的权威 environ");
});

it("已匹配 token 的 detached candidate 后续 environ EACCES 不能静默跳过", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  let environmentReads = 0;
  let kills = 0;
  const denied = new Error("candidate environ permission denied"); denied.code = "EACCES";
  const cleanup = createCompletionCleanup(777, {
    listProcesses: async () => ["306"],
    readEnvironment: async () => { if (++environmentReads > 1) throw denied; return Buffer.from("STOCK_GAME_RUST_CASE_TOKEN=active\0"); },
    readProcessStat: async (pid) => frozenStat(pid, { ppid: 999, group: 999 }),
    terminate: async () => { kills += 1; },
  });
  await assert.rejects(cleanup({ token: "active", pid: 306, total: monotonicMs() + 1000 }), (error) => {
    assert.match(error.message, /cannot confirm owned process 306/);
    assert.equal(error.cause.code, "EACCES");
    return true;
  });
  assert.equal(kills, 0);
});

it("二次 environ 的同 batch token 决定清理 deadline，初筛不提供权威归属", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  let environmentReads = 0;
  const killed = [];
  const deadlineA = monotonicMs() + 500;
  const deadlineB = deadlineA + 500;
  const cleanup = createCompletionCleanup(777, {
    listProcesses: async () => ["307"],
    readEnvironment: async () => Buffer.from(`STOCK_GAME_RUST_CASE_TOKEN=${++environmentReads === 1 ? "active-a" : "active-b"}\0`),
    readProcessStat: async (pid) => frozenStat(pid),
    terminate: async ({ pid }, _force, deadline) => { killed.push({ pid, deadline }); },
  });
  await Promise.all([
    cleanup({ token: "active-a", pid: 307, total: deadlineA }),
    cleanup({ token: "active-b", pid: 308, total: deadlineB }),
  ]);
  assert.deepEqual(killed, [{ pid: 307, deadline: deadlineB }]);
});
