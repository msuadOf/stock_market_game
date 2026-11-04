import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import promises from "node:fs/promises";
import { syncBuiltinESMExports } from "node:module";
import os from "node:os";
import path from "node:path";
import { it } from "node:test";
import { runBoundedCommand } from "./run-with-deadline.mjs";

function identity(pid) {
  try {
    const raw = readFileSync(`/proc/${pid}/stat`, "utf8");
    const fields = raw.slice(raw.lastIndexOf(")") + 2).split(" ");
    return { birth: fields[19], state: fields[0] };
  } catch (error) {
    if (error.code === "ENOENT" || error.code === "ESRCH") return undefined;
    throw error;
  }
}

it("首次快照后父进程 fork 并在 token 读取前退出时仍清理新增后代", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  const directory = mkdtempSync(path.join(os.tmpdir(), "deadline-orphan-race-"));
  const marker = path.join(directory, "child.json");
  const originalList = promises.readdir;
  let output = "";
  let triggered = false;
  const branch = `const {spawn}=require('node:child_process'); const fs=require('node:fs'); const record=(pid)=>{const raw=fs.readFileSync('/proc/'+pid+'/stat','utf8');return {pid,birth:raw.slice(raw.lastIndexOf(')')+2).split(' ')[19]}}; process.on('SIGUSR1',()=>{const child=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{detached:true,stdio:'ignore'});fs.writeFileSync(${JSON.stringify(marker)},JSON.stringify(record(child.pid)));child.unref();process.exit(0)});process.stdout.write(JSON.stringify(record(process.pid)));setInterval(()=>{},1000);`;
  const root = `const {spawn}=require('node:child_process');const child=spawn(process.execPath,['-e',${JSON.stringify(branch)}],{detached:true,stdio:['ignore','pipe','ignore']});child.stdout.once('data',data=>{process.stdout.write(data);child.unref();process.exit(0)});`;
  promises.readdir = async (directory, ...args) => {
    const entries = await originalList(directory, ...args);
    if (directory === "/proc" && !triggered) {
      triggered = true;
      const parent = JSON.parse(output);
      process.kill(parent.pid, "SIGUSR1");
      const until = Date.now() + 1000;
      while (true) {
        const current = identity(parent.pid);
        if (current === undefined || ["Z", "X"].includes(current.state)) break;
        assert.ok(Date.now() < until, "夹具父进程必须在第一次快照后退出");
        await new Promise((resolve) => setTimeout(resolve, 1));
      }
    }
    return entries;
  };
  syncBuiltinESMExports();
  try {
    await runBoundedCommand({ command: process.execPath, args: ["-e", root], timeoutMs: 3500, cleanupReserveMs: 1000, captureOutput: true, onStdout(chunk) { output += chunk; } });
    assert.equal(triggered, true);
    const child = JSON.parse(readFileSync(marker, "utf8"));
    const current = identity(child.pid);
    assert.ok(current === undefined || current.birth !== child.birth || ["Z", "X"].includes(current.state), "第一次快照未包含的后代也必须终态");
  } finally {
    promises.readdir = originalList;
    syncBuiltinESMExports();
    for (const record of [output, (() => { try { return readFileSync(marker, "utf8"); } catch (error) { if (error.code === "ENOENT") return ""; throw error; } })()]) {
      if (record.length === 0) continue;
      const owned = JSON.parse(record);
      const current = identity(owned.pid);
      if (current !== undefined && current.birth === owned.birth && !["Z", "X"].includes(current.state)) process.kill(owned.pid, "SIGKILL");
    }
    rmSync(directory, { recursive: true });
  }
});

for (const exitCode of [0, 17]) {
  it(`root 提前退出 ${exitCode} 后确认独立进程组后代终止，并保留原退出结果`, { timeout: 10000, skip: process.platform !== "linux" }, async () => {
    let output = "";
    const unrelated = spawn(process.execPath, ["-e", "setInterval(()=>{},1000)"], { stdio: "ignore" });
    const unrelatedClosed = once(unrelated, "close");
    const script = `const {spawn}=require('node:child_process'); const fs=require('node:fs'); const child=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{detached:true,stdio:'ignore'}); const raw=fs.readFileSync('/proc/'+child.pid+'/stat','utf8'); process.stdout.write(JSON.stringify({pid:child.pid,birth:raw.slice(raw.lastIndexOf(')')+2).split(' ')[19]})); child.unref(); process.exit(${exitCode});`;
    try {
      const command = runBoundedCommand({ command: process.execPath, args: ["-e", script], timeoutMs: 2500, cleanupReserveMs: 1000, captureOutput: true, onStdout(chunk) { output += chunk; } });
      if (exitCode === 0) await command;
      else await assert.rejects(command, (error) => {
        assert.match(error.message, /exited with 17/);
        assert.doesNotMatch(error.message, /termination.*unconfirmed|root identity unavailable/);
        return true;
      });
      const owned = JSON.parse(output);
      const current = identity(owned.pid);
      assert.ok(current === undefined || current.birth !== owned.birth || ["Z", "X"].includes(current.state), "成功返回或报告原失败前必须确认自有后代终态");
      assert.equal(unrelated.exitCode, null, "不得清理本次命令以外的进程");
      assert.equal(unrelated.signalCode, null);
    } finally {
      unrelated.kill("SIGKILL");
      await unrelatedClosed;
      if (output.length > 0) {
        const owned = JSON.parse(output);
        const current = identity(owned.pid);
        if (current !== undefined && current.birth === owned.birth && !["Z", "X"].includes(current.state)) process.kill(owned.pid, "SIGKILL");
      }
    }
  });
}
