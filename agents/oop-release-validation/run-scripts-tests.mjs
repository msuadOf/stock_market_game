import { spawn } from "node:child_process";
import { writeFile, readdir } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { runBoundedCommand } from "../../scripts/run-with-deadline.mjs";

async function runSuite() {
// 每个文件独立使用进程外 deadline，记录全部结果后才决定批次退出码。
const root = process.cwd();
const output = path.join(root, "agents/oop-release-validation");
const round = process.argv[2] ?? "first";
if (!/^[a-z0-9-]+$/.test(round)) throw new Error("执行批次名称非法");
const files = [];
async function discover(directory) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const filename = path.join(directory, entry.name);
    if (entry.isDirectory()) await discover(filename);
    else if (entry.isFile() && entry.name.endsWith(".test.mjs")) files.push(path.relative(root, filename));
  }
}
await discover(path.join(root, "scripts"));
files.sort();
const startedAt = Date.now();
const policy = { node: process.version, node_executable: process.execPath, available_cpu_count: os.availableParallelism(), file_count: files.length, concurrent_file_workers: 4, case_timeout_ms: 10000, external_file_deadline_ms: 10000, test_isolation: "none", test_concurrency: 1 };
console.log(JSON.stringify(policy));
const results = [];
let cursor = 0;
async function worker() {
  while (cursor < files.length) {
    const filename = files[cursor++];
    const logName = `scripts-${round}-${filename.replace(/^scripts\//, "").replace(/\.test\.mjs$/, "").replaceAll("/", "-")}.log`;
    const arguments_ = ["scripts/run-with-deadline.mjs", "10000", "--", process.execPath, "--test", "--test-timeout=10000", "--test-isolation=none", "--test-concurrency=1", "--test-reporter=tap", filename];
    const fileStartedAt = Date.now();
    let stdout = "";
    let stderr = "";
    const result = await new Promise((resolve) => {
      const child = spawn(process.execPath, arguments_, { cwd: root, env: { ...process.env, TMPDIR: path.join(root, ".tmp") }, stdio: ["ignore", "pipe", "pipe"] });
      child.stdout.on("data", (chunk) => { stdout += chunk; });
      child.stderr.on("data", (chunk) => { stderr += chunk; });
      child.on("error", (error) => resolve({ exit_code: null, signal: null, error: error.message }));
      child.on("close", (code, signal) => resolve({ exit_code: code, signal }));
    });
    const record = { file: filename, ...result, wall_ms: Date.now() - fileStartedAt, log: logName };
    await writeFile(path.join(output, logName), `命令：${process.execPath} ${arguments_.join(" ")}\n结果：${JSON.stringify(record)}\n\n${stdout}\n${stderr}`);
    results.push(record);
    console.log(JSON.stringify(record));
  }
}
await Promise.all(Array.from({ length: 4 }, worker));
results.sort((left, right) => left.file.localeCompare(right.file));
const failed = results.filter((result) => result.exit_code !== 0);
await writeFile(path.join(output, `scripts-${round}-results.json`), JSON.stringify({ policy, wall_ms: Date.now() - startedAt, results }, null, 2) + "\n");
await writeFile(path.join(output, `scripts-${round}-results.md`), `# scripts 完整脚本回归（长验收）\n\n使用 Node ${process.version}；${files.length} 个文件，最多 4 个文件 worker 并行；每文件进程外 10000ms deadline，每 case 10000ms timeout，文件内 test-concurrency=1、test-isolation=none。可用 CPU：${policy.available_cpu_count}。\n\n通过 ${results.length - failed.length}/${results.length}，失败 ${failed.length}/${results.length}。批次 wall-clock：${Date.now() - startedAt}ms。本文件记录此批次执行结果，不代表测试执行器修改了产品或原用例。\n\n| 文件 | 结果 | 耗时 ms | 日志 |\n|---|---|---:|---|\n${results.map((result) => `| ${result.file} | ${result.exit_code === 0 ? "通过" : "失败"}（exit ${result.exit_code}） | ${result.wall_ms} | [${result.log}](${result.log}) |`).join("\n")}\n`);
process.exitCode = failed.length > 0 ? 1 : 0;

}

// 全文件聚合属于长验收；外层监督覆盖所有短命令与结果记录。
if (process.env.STOCK_RELEASE_SCRIPTS_WORKER === "1") {
  await runSuite();
} else {
  await runBoundedCommand({ command: process.execPath, args: [fileURLToPath(import.meta.url), ...process.argv.slice(2)],
    env: { ...process.env, STOCK_RELEASE_SCRIPTS_WORKER: "1" }, timeoutMs: 300000, cleanupReserveMs: 1000 });
}
