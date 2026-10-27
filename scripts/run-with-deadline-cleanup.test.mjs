import assert from "node:assert/strict";
import { ChildProcess, spawn } from "node:child_process";
import { once } from "node:events";
import { EventEmitter } from "node:events";
import { readFileSync } from "node:fs";
import fs from "node:fs";
import promises from "node:fs/promises";
import { syncBuiltinESMExports } from "node:module";
import path from "node:path";
import { it } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";

const sourceRoot = fileURLToPath(new URL("../", import.meta.url));

function scanBoundaryCase(mode) {
  it(mode === "expired" ? "已过 cleanup deadline 不得开始 proc 枚举或发信号" : mode === "parent-disappeared" ? "parent 夹读后消失不能用裸 PPID 认领 child，更不能先 kill" : mode === "snapshot-parent" ? "异步 snapshot 不得拼接已消失 parent 与未知 child" : mode === "stale-root" ? "已退出真实 root 的旧 PID 不能认领新进程" : mode === "reused-group" ? "旧 root 消失后不能凭新 snapshot 的相同 PGID 认领新 group" : "snapshot 后 PID birth 换代必须拒绝且不能误杀", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
    const reads = new Map();
    const kills = [];
    let listings = 0;
    const originalRead = fs.readFileSync;
    const originalList = fs.readdirSync;
    const originalAsyncRead = promises.readFile;
    const originalAsyncList = promises.readdir;
    const originalKill = process.kill;
    const fakePids = new Set(mode === "snapshot-parent" ? [900301, 900302, 900303, 900399] : [900301, 900302, 900399]);
    const fakeRead = (file) => {
      const childList = /^\/proc\/(900301|900302|900303|900399)\/task\/\d+\/children$/.exec(String(file));
      if (childList !== null) return childList[1] === "900301" ? "900302" : "";
      const match = /^\/proc\/(900301|900302|900303|900399)\/stat$/.exec(String(file));
      if (match === null) throw new Error(`fixture 不允许读取非自有虚构路径 ${file}`);
      const pid = Number(match[1]); const count = (reads.get(pid) ?? 0) + 1; reads.set(pid, count);
      if (mode === "reused-group" && (pid === 900301 || pid === 900399)) {
        const error = new Error("fixture old group vanished"); error.code = "ENOENT"; throw error;
      }
      if (mode === "snapshot-parent" && pid === 900302 && count >= 2) {
        const error = new Error("fixture snapshot parent vanished"); error.code = "ENOENT"; throw error;
      }
      if (mode === "parent-disappeared" && pid === 900301 && count >= 2) {
        const error = new Error("fixture parent vanished after child list read"); error.code = "ENOENT"; throw error;
      }
      const birth = mode === "reused-group" || mode === "identity" && pid === 900302 && count >= 2 ? "200" : "100";
      const state = mode === "snapshot-parent" && pid === 900301 ? "Z" : "R";
      const ppid = mode === "reused-group" ? 1 : pid === 900302 ? 900301 : pid === 900303 ? 900302 : 0;
      return `${pid} (fixture) ${[state, ppid, pid === 900399 || pid === 900303 ? 999 : 900301, ...Array(16).fill(0), birth, 0].join(" ")}\n`;
    };
    fs.readFileSync = (file, ...args) => String(file).startsWith("/proc/") ? fakeRead(file) : originalRead(file, ...args);
    fs.readdirSync = (dir, ...args) => { if (dir !== "/proc") return originalList(dir, ...args); listings += 1; return [...fakePids].map(String); };
    promises.readFile = async (file, ...args) => String(file).startsWith("/proc/") ? fakeRead(file) : originalAsyncRead(file, ...args);
    promises.readdir = async (dir, ...args) => { const task = /^\/proc\/(900301|900302|900303|900399)\/task$/.exec(String(dir)); if (task !== null) return [task[1]]; if (dir !== "/proc") return originalAsyncList(dir, ...args); listings += 1; return [...fakePids].map(String); };
    process.kill = (pid, signal) => { assert.ok(fakePids.has(Math.abs(pid)), "fixture 只能观察虚构 PID，禁止真实信号"); kills.push({ pid, signal }); return true; };
    syncBuiltinESMExports();
    try {
      const { terminateTree } = await import(pathToFileURL(path.join(sourceRoot, "scripts/run-with-deadline.mjs")));
      if (mode === "expired") {
        await assert.rejects(terminateTree({ pid: 900301, birth: "100" }, true, 0, () => 1), /deadline/);
        assert.equal(listings, 0, "过期后不能新起全 proc 枚举");
        assert.equal(reads.size, 0); assert.equal(kills.length, 0);
      } else if (mode === "stale-root") {
        const child = new ChildProcess(); child.pid = 900301; child.exitCode = 17;
        await assert.rejects(terminateTree(child, true, Date.now() + 100), /root identity unavailable/);
        assert.equal(kills.length, 0, "已退出真实 ChildProcess 的旧 pid 不能认领新 root");
      } else if (mode === "reused-group") {
        await assert.rejects(terminateTree({ pid: 900301, birth: "100" }, true, Date.now() + 100), (error) => {
          assert.equal(kills.length, 0, `无旧身份锚时不得向同数值新 PGID 或其成员发信号：${JSON.stringify(kills)}`);
          assert.match(error.message, /group identity unavailable/);
          return true;
        });
      } else if (mode === "parent-disappeared" || mode === "snapshot-parent") {
        await assert.rejects(terminateTree({ pid: 900301, birth: "100" }, true, Date.now() + 100), /parent identity disappeared/);
        assert.equal(kills.length, 0, "缺少同 birth parent 夹验时不能把 PPID 当 owned 证明");
      } else {
        await assert.rejects(terminateTree({ pid: 900301, birth: "100" }, true, Date.now() + 100), /identity changed/);
        assert.equal(kills.length, 0, "叶 PID 换代应在任何 kill 前中止");
      }
    } finally {
      fs.readFileSync = originalRead; fs.readdirSync = originalList;
      promises.readFile = originalAsyncRead; promises.readdir = originalAsyncList;
      process.kill = originalKill; syncBuiltinESMExports();
    }
  });
}

scanBoundaryCase("expired");
scanBoundaryCase("identity");
scanBoundaryCase("parent-disappeared");
scanBoundaryCase("snapshot-parent");
scanBoundaryCase("stale-root");
scanBoundaryCase("reused-group");

it("既有 case token cleanup 过期时不得开始扫描", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  const { createCompletionCleanup } = await import("./rust-test-case-supervisor.mjs");
  let listings = 0;
  const cleanup = createCompletionCleanup(900301, {
    listProcesses: async () => { listings += 1; return []; },
  });
  await assert.rejects(cleanup({ token: "expired", pid: 900301, total: Number(process.hrtime.bigint()) / 1e6 - 1 }), /cleanup deadline/);
  assert.equal(listings, 0, "不能在旧 case 的清理界后开始新 scan");
});

it("case token 枚举返回已过期时不得继续读取 environ 或 stat", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  const { createCompletionCleanup } = await import("./rust-test-case-supervisor.mjs");
  let reads = 0;
  const cleanup = createCompletionCleanup(900301, {
    listProcesses: async () => { await new Promise((resolve) => setTimeout(resolve, 20)); return ["900302"]; },
    readEnvironment: async () => { reads += 1; return Buffer.from("OTHER=unrelated\0"); },
    readProcessStat: async () => { throw new Error("过期 fixture 不允许 stat"); },
    terminate: async () => { throw new Error("过期 fixture 不允许 kill"); },
  });
  await assert.rejects(cleanup({ token: "expired-during-list", pid: 900301, total: Number(process.hrtime.bigint()) / 1e6 + 10 }), /cleanup deadline/);
  assert.equal(reads, 0, "枚举晚到不能再发起 environ/stat read");
});

function inFlightCleanupCase(mode) {
  it(mode === "failure" ? "在途正常 token cleanup 遇更早 failure 截止后不得继续枚举或 kill" : "同 flush 晚截止记录在途跨过最早正常记录的界后不得 read 或 kill", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
    const { createCompletionCleanup } = await import("./rust-test-case-supervisor.mjs");
    const originalRead = promises.readFile;
    const originalList = promises.readdir;
    const originalKill = process.kill;
    let releaseRead;
    let enteredRead;
    const entered = new Promise((resolve) => { enteredRead = resolve; });
    let held = false;
    let listings = 0;
    const kills = [];
    const stat = () => `900302 (fixture) ${[kills.length > 0 ? "Z" : "R", 900301, 900301, ...Array(16).fill(0), "100", 0].join(" ")}\n`;
    promises.readFile = async (file, ...args) => {
      if (file === "/proc/900302/stat") {
        if (!held) { held = true; enteredRead(); await new Promise((resolve) => { releaseRead = resolve; }); }
        return stat();
      }
      if (file === "/proc/900302/task/900302/children") return "";
      if (String(file).startsWith("/proc/")) throw new Error(`fixture 不允许读取 ${file}`);
      return originalRead(file, ...args);
    };
    promises.readdir = async (dir, ...args) => {
      if (dir === "/proc/900302/task") { listings += 1; return ["900302"]; }
      if (dir === "/proc") { listings += 1; return ["900302"]; }
      return originalList(dir, ...args);
    };
    process.kill = (pid, signal) => { assert.equal(pid, 900302); kills.push({ pid, signal }); return true; };
    syncBuiltinESMExports();
    try {
      const cleanup = createCompletionCleanup(900301, {
        listProcesses: async () => ["900302"],
        readEnvironment: async () => Buffer.from("STOCK_GAME_RUST_CASE_TOKEN=active\0"),
        readProcessStat: async () => stat(),
      });
      const started = Number(process.hrtime.bigint()) / 1e6;
      const earlier = started + 50;
      const active = { token: "active", pid: 900302, total: started + 500 };
      const pending = mode === "failure" ? cleanup(active) : Promise.all([
        cleanup({ token: "earlier", pid: 900303, total: earlier }), cleanup(active),
      ]);
      const rejected = assert.rejects(pending, /cleanup deadline/);
      await entered;
      if (mode === "failure") await cleanup.afterFailure([], Number(process.hrtime.bigint()) / 1e6 - 1);
      else {
        await new Promise((resolve) => setTimeout(resolve, Math.max(1, earlier - Number(process.hrtime.bigint()) / 1e6 + 1)));
        assert.ok(Number(process.hrtime.bigint()) / 1e6 >= earlier, "必须实际跨过同 flush 的最早 total");
      }
      releaseRead();
      await rejected;
      assert.equal(kills.length, 0, `更早共享界后不能按旧 total kill：${JSON.stringify(kills)}`);
      assert.equal(listings, 0, "在途 stat 晚到后不能按旧 total 新起枚举");
    } finally {
      releaseRead?.();
      promises.readFile = originalRead; promises.readdir = originalList;
      process.kill = originalKill; syncBuiltinESMExports();
    }
  });
}

inFlightCleanupCase("failure");
inFlightCleanupCase("normal-batch");

it("真实 leader 已退出为 Z 时仍清理有同 birth 锚的原 group 成员", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  const { terminateTree } = await import("./run-with-deadline.mjs");
  const leaderScript = "const{spawn}=require('node:child_process');const fs=require('node:fs');const member=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:'ignore'});const raw=fs.readFileSync('/proc/'+member.pid+'/stat','utf8');process.stdout.write(JSON.stringify({pid:member.pid,birth:raw.slice(raw.lastIndexOf(')')+2).split(' ')[19]}));setTimeout(()=>process.exit(0),50);";
  // 自有 parent 暂停事件循环，让已退出 leader 保留为 Z，随后正常 reap；不增加 fixture 工具依赖。
  const parentScript = `const{spawn}=require('node:child_process');const fs=require('node:fs');const leader=spawn(process.execPath,['-e',${JSON.stringify(leaderScript)}],{detached:true,stdio:['ignore','pipe','inherit']});let output='';leader.stdout.on('data',chunk=>{output+=chunk;const member=JSON.parse(output);const raw=fs.readFileSync('/proc/'+leader.pid+'/stat','utf8');process.stdout.write(JSON.stringify({root:{pid:leader.pid,birth:raw.slice(raw.lastIndexOf(')')+2).split(' ')[19]},member})+'\\n');const until=performance.now()+1500;while(performance.now()<until){}});leader.on('close',()=>process.exit(0));`;
  const parent = spawn(process.execPath, ["-e", parentScript], { stdio: ["ignore", "pipe", "inherit"] });
  const closed = once(parent, "close");
  let owned;
  try {
    owned = await new Promise((resolve, reject) => {
      let output = "";
      parent.stdout.on("data", (chunk) => { output += chunk; if (output.includes("\n")) resolve(JSON.parse(output)); });
      parent.once("error", reject);
    });
    const waitingUntil = performance.now() + 500;
    let rootState;
    do {
      const raw = readFileSync(`/proc/${owned.root.pid}/stat`, "utf8");
      const fields = raw.slice(raw.lastIndexOf(")") + 2).split(" ");
      assert.equal(fields[19], owned.root.birth);
      rootState = fields[0];
      if (rootState !== "Z") await new Promise((resolve) => setTimeout(resolve, 2));
    } while (rootState !== "Z" && performance.now() < waitingUntil);
    assert.equal(rootState, "Z", "必须实际建立 leader 已退出但原身份仍占用 PID 的前提");
    await terminateTree(owned.root, true, Date.now() + 500);
    try {
      const raw = readFileSync(`/proc/${owned.member.pid}/stat`, "utf8");
      const fields = raw.slice(raw.lastIndexOf(")") + 2).split(" ");
      assert.equal(fields[19], owned.member.birth); assert.match(fields[0], /^[ZX]$/);
    } catch (error) { if (error.code !== "ENOENT" && error.code !== "ESRCH") throw error; }
  } finally {
    if (owned !== undefined) for (const identity of [owned.member, owned.root]) {
      try {
        const raw = readFileSync(`/proc/${identity.pid}/stat`, "utf8"); const fields = raw.slice(raw.lastIndexOf(")") + 2).split(" ");
        assert.equal(fields[19], identity.birth, "fixture 仅清理同 birth 的自有进程");
        if (fields[0] !== "Z" && fields[0] !== "X") process.kill(identity.pid, "SIGKILL");
      } catch (error) { if (error.code !== "ENOENT" && error.code !== "ESRCH") throw error; }
    }
    await closed;
  }
});

it("短 cleanup reserve 遇 pending hook 仍须在总预算内终止真实 root，且保持未确认错误", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  const { runBoundedCommand } = await import(pathToFileURL(path.join(sourceRoot, "scripts/run-with-deadline.mjs")));
  let root;
  let closed;
  let stdout = "";
  const started = performance.now();
  try {
    await assert.rejects(runBoundedCommand({ command: process.execPath, args: ["-e", "process.stdout.write('ready');setInterval(()=>{},1000)"],
      timeoutMs: 250, cleanupReserveMs: 50, captureOutput: true, onStdout: (chunk) => { stdout += chunk; },
      spawnProcess: (command, args, options) => { root = spawn(command, args, options); closed = once(root, "close"); return root; },
      terminateProcessTree: () => new Promise(() => {}),
    }), /termination.*unconfirmed/);
    const wallMs = performance.now() - started;
    assert.equal(stdout, "ready", "必须有真实 root 启动证据");
    try {
      const stat = readFileSync(`/proc/${root.pid}/stat`, "utf8"); const fields = stat.slice(stat.lastIndexOf(")") + 2).split(" ");
      assert.match(fields[0], /^[ZX]$/, "不能仅拒绝 Promise 却留下真实活 root");
    } catch (error) { if (error.code !== "ENOENT" && error.code !== "ESRCH") throw error; }
    assert.ok(wallMs <= 250, `library rejection wall ${wallMs}ms 超过250ms`);
  } finally {
    if (root !== undefined && root.exitCode === null && root.signalCode === null) root.kill("SIGKILL");
    if (closed !== undefined) await closed;
  }
});

it("最终 cutoff 不能覆盖已收到的真实退出诊断", { timeout: 10000 }, async () => {
  const { runBoundedCommand } = await import("./run-with-deadline.mjs");
  const child = new EventEmitter(); child.stdout = new EventEmitter(); child.stderr = new EventEmitter();
  const result = runBoundedCommand({ command: "compiler", timeoutMs: 100, cleanupReserveMs: 40,
    captureOutput: true, spawnProcess: () => child, terminateProcessTree: () => new Promise(() => {}),
  });
  child.stderr.emit("data", Buffer.from("original compiler diagnostic")); child.emit("close", 17, null);
  await assert.rejects(result, (error) => {
    assert.match(error.message, /exited with 17.*original compiler diagnostic/);
    assert.match(error.message, /termination.*unconfirmed/);
    return true;
  });
});

it("最后信号 timer 在同 loop 迟到总截止之后不得再 kill", { timeout: 10000 }, async () => {
  const { runBoundedCommand } = await import("./run-with-deadline.mjs");
  let root; let closed; let lateKills = 0; let originalKill;
  try {
    await assert.rejects(runBoundedCommand({ command: process.execPath, args: ["-e", "setInterval(()=>{},1000)"], timeoutMs: 150, cleanupReserveMs: 60,
      spawnProcess: (command, args, options) => { root = spawn(command, args, options); closed = once(root, "close"); originalKill = root.kill.bind(root); root.kill = () => { lateKills += 1; return true; }; return root; },
      terminateProcessTree: () => { const until = performance.now() + 130; while (performance.now() < until) {} return new Promise(() => {}); },
    }), /termination.*unconfirmed/);
    assert.equal(lateKills, 0, "不得在真实 deadline 之后启动最后 root 信号");
  } finally { if (originalKill !== undefined) originalKill("SIGKILL"); if (closed !== undefined) await closed; }
});

it("exit 已报17而 pipe 令 close 迟延时仍保留原始诊断", { timeout: 10000 }, async () => {
  const { runBoundedCommand } = await import("./run-with-deadline.mjs");
  const child = new EventEmitter(); child.stdout = new EventEmitter(); child.stderr = new EventEmitter();
  const result = runBoundedCommand({ command: "pipe-held-compiler", timeoutMs: 120, cleanupReserveMs: 50,
    captureOutput: true, spawnProcess: () => child, terminateProcessTree: () => new Promise(() => {}),
  });
  child.stderr.emit("data", Buffer.from("original pipe-held compiler diagnostic")); child.emit("exit", 17, null);
  await assert.rejects(result, (error) => {
    assert.match(error.message, /exited with 17.*original pipe-held compiler diagnostic/);
    assert.match(error.message, /termination.*unconfirmed/);
    assert.doesNotMatch(error.message, /direct child close confirmed/);
    return true;
  });
});

it("真实 exit17 且 detached 孙持有 pipe 时 cutoff 保留诊断并明确 close 未确认", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  const { runBoundedCommand } = await import("./run-with-deadline.mjs");
  let output = "";
  try {
    await assert.rejects(runBoundedCommand({ command: process.execPath,
      args: ["-e", "const{spawn}=require('node:child_process');const fs=require('node:fs');const grand=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{detached:true,stdio:['ignore','inherit','inherit']});const raw=fs.readFileSync('/proc/'+grand.pid+'/stat','utf8');process.stdout.write(JSON.stringify({pid:grand.pid,birth:raw.slice(raw.lastIndexOf(')')+2).split(' ')[19]}));process.stderr.write('real pipe-held compiler diagnostic');process.exit(17);"],
      timeoutMs: 500, cleanupReserveMs: 150, captureOutput: true, onStdout: (chunk) => { output += chunk; },
    }), (error) => {
      assert.match(error.message, /exited with 17.*real pipe-held compiler diagnostic/);
      assert.match(error.message, /termination.*unconfirmed/);
      assert.doesNotMatch(error.message, /direct child close confirmed/);
      return true;
    });
    assert.ok(output.length > 0, "必须实际启动持有 pipe 的孙进程");
  } finally {
    if (output.length > 0) {
      const owned = JSON.parse(output);
      try {
        const raw = readFileSync(`/proc/${owned.pid}/stat`, "utf8"); const fields = raw.slice(raw.lastIndexOf(")") + 2).split(" ");
        assert.equal(fields[19], owned.birth, "测试仅清理同 birth 的自有 pipe holder");
        if (fields[0] !== "Z" && fields[0] !== "X") process.kill(owned.pid, "SIGKILL");
      } catch (error) { if (error.code !== "ENOENT" && error.code !== "ESRCH") throw error; }
    }
  }
});
