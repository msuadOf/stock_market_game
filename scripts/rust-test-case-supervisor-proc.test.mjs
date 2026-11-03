import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { readFileSync } from "node:fs";
import { mkdir, mkdtemp, readFile, rm } from "node:fs/promises";
import path from "node:path";
import { createInterface } from "node:readline";
import { it } from "node:test";
import { PROCESS_SCAN_CONCURRENCY, createCompletionCleanup } from "./rust-test-case-supervisor.mjs";

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
    // 只列举本 fixture 的真实 PID：该用例验证 environ EACCES 的归属语义，
    // 不把整机进程数（2100+）带进 1 秒预算；全表列举契约由下方并发用例独立覆盖。
    const cleanup = createCompletionCleanup(fixture.guardian.pid, { listProcesses: async () => [String(fixture.record.pid), String(fixture.guardian.pid)] });
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
    // 同上：真实 /proc environ 与 stat，但列举范围限定为本 fixture 的 PID。
    const cleanup = createCompletionCleanup(fixture.guardian.pid, { listProcesses: async () => [String(fixture.record.pid), String(fixture.guardian.pid)] });
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

it("活进程短暂拒绝读取后同 birth 重试并清理匹配 token", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  let reads = 0;
  const killed = [];
  const denied = Object.assign(new Error("temporary denial"), { code: "EACCES" });
  const cleanup = createCompletionCleanup(777, {
    listProcesses: async () => ["330"],
    readEnvironment: async () => { if (++reads === 1) throw denied; return Buffer.from("STOCK_GAME_RUST_CASE_TOKEN=active\0"); },
    readProcessStat: async (pid) => frozenStat(pid),
    readProcessStatm: async () => "6073 2960 1743 793 0 1553 0\n",
    terminate: async ({ pid }) => { killed.push(pid); },
  });
  await cleanup({ token: "active", pid: 330, total: monotonicMs() + 1000 });
  assert.deepEqual(killed, [330]);
  assert.ok(reads >= 3);
});

it("第二次 environ 暂时拒绝后重新验证 token 与 birth", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  let reads = 0;
  const killed = [];
  const cleanup = createCompletionCleanup(777, {
    listProcesses: async () => ["332"],
    readEnvironment: async () => { if (++reads === 2) throw Object.assign(new Error("temporary denial"), { code: "EACCES" }); return Buffer.from("STOCK_GAME_RUST_CASE_TOKEN=active\0"); },
    readProcessStat: async (pid) => frozenStat(pid),
    terminate: async ({ pid }) => { killed.push(pid); },
  });
  await cleanup({ token: "active", pid: 332, total: monotonicMs() + 1000 });
  assert.equal(reads, 4);
  assert.deepEqual(killed, [332]);
});

it("重试等待受共享 deadline 限制", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  const cleanup = createCompletionCleanup(777, {
    listProcesses: async () => ["333"],
    readEnvironment: async () => { throw Object.assign(new Error("denied"), { code: "EACCES" }); },
    readProcessStat: async (pid) => frozenStat(pid),
    readProcessStatm: async () => "6073 2960 1743 793 0 1553 0\n",
  });
  await assert.rejects(cleanup({ token: "active", pid: 333, total: monotonicMs() + 4 }), /deadline/);
});

it("重试期间 PID birth 改变必须拒绝且不得清理", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  let stats = 0;
  let kills = 0;
  const denied = Object.assign(new Error("temporary denial"), { code: "EACCES" });
  const cleanup = createCompletionCleanup(777, {
    listProcesses: async () => ["331"],
    readEnvironment: async () => { throw denied; },
    readProcessStat: async (pid) => frozenStat(pid, { birth: ++stats === 1 ? "100" : "200" }),
    readProcessStatm: async () => "6073 2960 1743 793 0 1553 0\n",
    terminate: async () => { kills += 1; },
  });
  await assert.rejects(cleanup({ token: "active", pid: 331, total: monotonicMs() + 1000 }), /identity changed/);
  assert.equal(kills, 0);
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

it("出生早于 launcher 的旧进程在 exec 后携带 token 仍必须被发现并清理", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  // execve 不改变 starttime：旧进程可以在 launcher 启动后 exec 到带精确 token 的新映像，
  // 因此不能用出生时间跳过 environ 检查。
  const killed = [];
  const cleanup = createCompletionCleanup(777, {
    // 故意注入 launcherBirth：被否决的“按出生跳过”实现会因此跳过 PID 401，必须让该回归变红。
    launcherBirth: "2000",
    listProcesses: async () => ["401"],
    readProcessStat: async (pid) => frozenStat(pid, { birth: "1" }),
    readEnvironment: async () => Buffer.from("STOCK_GAME_RUST_CASE_TOKEN=active\0"),
    terminate: async ({ pid }) => { killed.push(pid); },
  });
  await cleanup({ token: "active", pid: 401, total: monotonicMs() + 1000 });
  assert.deepEqual(killed, [401]);
});

it("整机 /proc 扫描并发有上限，不得一次性发起全部读取", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  let active = 0;
  let peak = 0;
  const examined = [];
  const listing = Array.from({ length: PROCESS_SCAN_CONCURRENCY * 8 }, (_, index) => String(1000 + index));
  const cleanup = createCompletionCleanup(777, {
    listProcesses: async () => listing,
    readProcessStat: async (pid) => {
      active += 1; peak = Math.max(peak, active); examined.push(String(pid));
      await new Promise((resolve) => setImmediate(resolve));
      active -= 1;
      return frozenStat(pid, { birth: "1", ppid: 999, group: 999 });
    },
    readEnvironment: async () => { throw Object.assign(new Error("unrelated denial"), { code: "EACCES" }); },
    terminate: async () => { throw new Error("没有匹配 token，不得清理"); },
  });
  await cleanup({ token: "active", pid: 1000, total: monotonicMs() + 5000 });
  assert.equal(examined.length, listing.length, "列举中的每个 PID 都必须被检查，不能按出生时间跳过");
  assert.ok(peak <= PROCESS_SCAN_CONCURRENCY, `扫描并发峰值 ${peak} 超过上限 ${PROCESS_SCAN_CONCURRENCY}`);
});

it("environ EACCES 且地址空间已释放时必须等待同 birth 的终态，不得直接报活 owned", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  // 进程退出时 exit_mm 先释放地址空间，task 状态可能仍是 R；单次 statm=0 不是终态证明，
  // 必须按同 birth 等到 Z/X/消失。这里第 7 次 stat 起进入 Z。
  let stats = 0;
  let kills = 0;
  const cleanup = createCompletionCleanup(777, {
    listProcesses: async () => ["308"],
    readEnvironment: async () => { throw Object.assign(new Error("exiting"), { code: "EACCES" }); },
    readProcessStat: async (pid) => frozenStat(pid, { state: ++stats >= 7 ? "Z" : "R" }),
    readProcessStatm: async () => "0 0 0 0 0 0 0\n",
    terminate: async () => { kills += 1; },
  });
  await assert.doesNotReject(cleanup({ token: "active", pid: 308, total: monotonicMs() + 1000 }));
  assert.equal(kills, 0);
  assert.ok(stats >= 7, "必须实际等待到 Z 终态，而不是凭单次快照放行");
});

it("退出窗口等待受共享 deadline 限制", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  const cleanup = createCompletionCleanup(777, {
    listProcesses: async () => ["310"],
    readEnvironment: async () => { throw Object.assign(new Error("exiting"), { code: "EACCES" }); },
    readProcessStat: async (pid) => frozenStat(pid, { state: "R" }),
    readProcessStatm: async () => "0 0 0 0 0 0 0\n",
  });
  await assert.rejects(cleanup({ token: "active", pid: 310, total: monotonicMs() + 60 }), /deadline/);
});

it("地址空间仍存在的 owned non-dumpable 进程必须继续显式失败", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  const cleanup = createCompletionCleanup(777, {
    listProcesses: async () => ["309"],
    readEnvironment: async () => { throw Object.assign(new Error("EACCES: permission denied"), { code: "EACCES" }); },
    readProcessStat: async (pid) => frozenStat(pid, { state: "S" }),
    readProcessStatm: async () => "6073 2960 1743 793 0 1553 0\n",
  });
  await assert.rejects(cleanup({ token: "active", pid: 309, total: monotonicMs() + 1000 }),
    /cannot confirm owned process 309.*EACCES/);
});

// 退出窗口采样只忽略 /proc 竞态确有依据的错误码；其它错误必须原样重抛，不能静默吞掉。
function procRaceCode(error, codes) {
  if (codes.includes(error.code)) return error.code;
  throw error;
}

async function processInExitWindow() {
  for (let attempt = 0; attempt < 60; attempt += 1) {
    const child = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], { stdio: "ignore" });
    const closed = once(child, "close");
    await new Promise((resolve) => setTimeout(resolve, 5 + (attempt % 6) * 5));
    child.kill("SIGKILL");
    for (let poll = 0; poll < 400; poll += 1) {
      let environ;
      try { await readFile(`/proc/${child.pid}/environ`); }
      catch (error) { environ = procRaceCode(error, ["EACCES", "EPERM", "ENOENT", "ESRCH"]); }
      if (environ === "EACCES") {
        try {
          const stat = await readFile(`/proc/${child.pid}/stat`, "utf8");
          const statm = await readFile(`/proc/${child.pid}/statm`, "utf8");
          if (stat.slice(stat.lastIndexOf(")") + 2).split(" ")[0] === "R" && statm.trim() === "0 0 0 0 0 0 0") {
            return { pid: child.pid, closed, state: "R", statm };
          }
        } catch (error) { procRaceCode(error, ["ENOENT", "ESRCH"]); }
      }
      if (environ === "ENOENT") break;
    }
    await closed;
  }
  throw new Error("无法在 60 次内捕获“state=R 且地址空间已释放”的真实退出窗口");
}

it("地址空间从全 0 恢复且 environ 仍 EACCES 时必须显式失败且不得清理", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  // 单次 statm=0 只代表“此刻没有 mm”。若 mm 恢复而 environ 仍拒绝访问，必须回到 fail-closed，
  // 不能把等待当成放行。
  let statmReads = 0;
  let kills = 0;
  const cleanup = createCompletionCleanup(777, {
    listProcesses: async () => ["311"],
    readEnvironment: async () => { throw Object.assign(new Error("EACCES: permission denied"), { code: "EACCES" }); },
    readProcessStat: async (pid) => frozenStat(pid, { state: "R" }),
    readProcessStatm: async () => (++statmReads <= 2 ? "0 0 0 0 0 0 0\n" : "100 10 10 1 0 10 0\n"),
    terminate: async () => { kills += 1; },
  });
  await assert.rejects(cleanup({ token: "active", pid: 311, total: monotonicMs() + 1000 }),
    /cannot confirm owned process 311.*EACCES/);
  assert.equal(kills, 0);
  assert.ok(statmReads >= 3, "必须先观察到全 0，再观察到 mm 恢复");
});

it("退出窗口采样只忽略有依据的 /proc 竞态错误，未知错误必须重抛", () => {
  for (const [codes, code] of [
    [["EACCES", "EPERM", "ENOENT", "ESRCH"], "EACCES"],
    [["EACCES", "EPERM", "ENOENT", "ESRCH"], "EPERM"],
    [["EACCES", "EPERM", "ENOENT", "ESRCH"], "ENOENT"],
    [["ENOENT", "ESRCH"], "ESRCH"],
  ]) {
    assert.equal(procRaceCode(Object.assign(new Error(code), { code }), codes), code);
  }
  const io = Object.assign(new Error("I/O failure"), { code: "EIO" });
  assert.throws(() => procRaceCode(io, ["ENOENT", "ESRCH"]), (error) => error === io, "未知 errno 必须原样重抛");
  const noCode = new Error("unknown failure without code");
  assert.throws(() => procRaceCode(noCode, ["ENOENT"]), (error) => error === noCode, "无 errno 的错误也必须重抛");
});

it("真实退出中的子进程（environ EACCES）必须等到 Z/X/消失后正常结束清理", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  const { pid, closed, state, statm } = await processInExitWindow();
  try {
    assert.equal(state, "R", "捕获时必须仍是 R（地址空间已释放但尚未进入 Z）");
    assert.equal(statm.trim(), "0 0 0 0 0 0 0", "捕获时地址空间已释放");
    const cleanup = createCompletionCleanup(process.pid, { listProcesses: async () => [String(pid)] });
    await assert.doesNotReject(cleanup({ token: "active", pid, total: monotonicMs() + 1000 }));
  } finally {
    try { process.kill(pid, "SIGKILL"); } catch (error) { procRaceCode(error, ["ESRCH"]); }
    await closed;
  }
});
