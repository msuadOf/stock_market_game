import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { readdir, readFile } from "node:fs/promises";
import { it } from "node:test";
import { promisify } from "node:util";

const exec = promisify(execFile);
const wasmTarget = "wasm32-unknown-unknown";

async function productionPackages(root, target) {
  const args = ["tree", "--locked", "--offline", "-p", root, "-e", "normal,build",
    "--prefix", "none", "--format", "{p}"];
  if (target !== undefined) args.push("--target", target);
  const { stdout } = await exec("cargo", args, {
    cwd: new URL("..", import.meta.url), encoding: "utf8", timeout: 5000,
  });
  const packages = new Set(stdout.split(/\r?\n/).map((line) => line.match(/^(\S+) v\S+/)?.[1]).filter(Boolean));
  assert.ok(packages.has(root), `${root} 必须出现在目标依赖图中`);
  return packages;
}

function assertAbsent(packages, names) {
  for (const name of names) assert.ok(!packages.has(name), `目标依赖图不可包含 ${name}`);
}

it("LibOS 两个目标都不反向依赖引擎或任何产品宿主", async () => {
  const graphs = await Promise.all([productionPackages("libos"), productionPackages("libos", wasmTarget)]);
  for (const graph of graphs) {
    assertAbsent(graph, ["engine", "engine-gpu", "server", "web-wasm", "native-store", "axum", "tauri", "rusqlite"]);
  }
  assertAbsent(graphs[0], ["wasm-bindgen", "js-sys", "web-sys", "serde-wasm-bindgen"]);
});

it("Server 和 WASM 产品都链接 engine 与 LibOS，且不串入对方宿主", async () => {
  const [native, wasm] = await Promise.all([productionPackages("server"), productionPackages("web-wasm", wasmTarget)]);
  for (const graph of [native, wasm]) {
    for (const name of ["engine", "libos"]) assert.ok(graph.has(name), `产品依赖图必须包含 ${name}`);
  }
  assertAbsent(native, ["web-wasm", "serde-wasm-bindgen", "web-sys"]);
  assertAbsent(wasm, ["server", "axum", "tokio", "native-store", "rusqlite", "tikv-jemallocator", "tauri"]);
});

async function rustSources(root) {
  const entries = await readdir(root, { withFileTypes: true });
  const files = await Promise.all(entries.map(async (entry) => {
    const url = new URL(entry.name + (entry.isDirectory() ? "/" : ""), root);
    if (entry.isDirectory()) return rustSources(url);
    return entry.isFile() && entry.name.endsWith(".rs") ? [url] : [];
  }));
  return files.flat();
}

const platformCondition = /\b(?:target_\w+|unix|windows)\b/;

function platformViolations(source) {
  const findings = [];
  for (const [attribute, kind, body] of source.matchAll(/#\[\s*(cfg(?:_attr)?)\s*\(([^\]]+)\)\s*]/g)) {
    const condition = body.trim();
    if (!platformCondition.test(condition)) continue;
    // 只允许明确不进入普通构建的 Rust 单测配置。不认可 any(test, ...)。
    if (kind === "cfg" && /^all\(\s*test\s*,/.test(condition)) continue;
    // verification-harness 默认关闭；Browser 故障注入入口不是产品接口。
    const verification = 'all(target_family="wasm",feature="verification-harness")';
    const compact = condition.replace(/\s+/g, "");
    if (kind === "cfg" && (compact === verification || compact === `any(test,${verification})`)) continue;
    findings.push(attribute);
  }
  for (const match of source.matchAll(/\bcfg!\s*\(/g)) {
    let depth = 1;
    const start = match.index + match[0].length;
    let end = start;
    while (end < source.length && depth > 0) {
      if (source[end] === "(") depth++;
      if (source[end] === ")") depth--;
      end++;
    }
    const probe = source.slice(start, end);
    if (platformCondition.test(probe)) findings.push(`cfg!(${probe}`);
  }
  return findings;
}

it("平台边界门禁拒绝隐藏在组合谓词中的生产平台分支", () => {
  assert.deepEqual(platformViolations('#[cfg(all(test, not(target_arch = "wasm32")))]'), []);
  assert.equal(platformViolations('#[cfg(any(test, target_family = "wasm"))]').length, 1);
  assert.equal(platformViolations('#[cfg(not(any(test, target_family = "wasm")))]').length, 1);
  assert.equal(platformViolations('#[cfg_attr(\n  target_os = "linux", inline\n)]').length, 1);
  assert.equal(platformViolations('let x = cfg!(all(any(unix, windows), feature="x"));').length, 1);
});

it("整个 engine 的产品源不按目标选择实现", async () => {
  const engine = new URL("../packages/engine/src/", import.meta.url);
  const sources = await rustSources(engine);
  assert.ok(sources.length > 100, "不得空扫或只扫描已迁移的模块");
  const results = await Promise.all(sources.map(async (file) => {
    const relative = file.pathname.slice(engine.pathname.length);
    const source = await readFile(file, "utf8");
    // 外挂的 Rust 单测文件由模块级 cfg(test) 装配，不属于产品。
    if (/(?:_tests|(?:^|\/)tests)\.rs$/.test(relative) || /\/tests\//.test(relative)) return [];
    return platformViolations(source).map((attr) => `${relative}: ${attr}`);
  }));
  assert.deepEqual(results.flat(), []);
  const manifest = await readFile(new URL("../packages/engine/Cargo.toml", import.meta.url), "utf8");
  assert.doesNotMatch(manifest, /^\s*\[target\./m, "engine 不应有按目标选择的 Cargo 依赖");
});
