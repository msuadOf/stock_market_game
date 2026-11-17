// 隔离验证制品的构建receipt；调用者必须使用外层进程树deadline，本模块不在导入时执行。
import assert from "node:assert/strict";
import { runBoundedCommand } from "./run-with-deadline.mjs";
import { createHash } from "node:crypto";
import { lstat, readFile, readdir, realpath, rm, writeFile } from "node:fs/promises";
import path from "node:path";
const hash = value => createHash("sha256").update(value).digest("hex");
const roots = ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo", "packages/engine", "packages/libos", "packages/native-store", "apps/server", "apps/desktop/src-tauri", "apps/web/src", "apps/web/package.json", "apps/web/vite.config.ts", "apps/web/index.html", "pnpm-lock.yaml", "pnpm-workspace.yaml", "package.json"];
export async function authorityProductSourceIdentity(root) {
  const entries = [];
  async function collect(relative) {
    const target = path.join(root, relative), stat = await lstat(target);
    assert.ok(!stat.isSymbolicLink(), `源码闭包不接受symlink:${relative}`);
    if (stat.isDirectory()) {
      for (const child of (await readdir(target)).sort()) {
        if (["target", "node_modules", "gen", "icons", "wasm-pack-output"].includes(child)) continue;
        await collect(path.posix.join(relative, child));
      }
    } else if (stat.isFile()) entries.push({ path: relative, sha256: hash(await readFile(target)) });
  }
  for (const relative of roots) await collect(relative);
  return entries;
}
export async function executeAuthorityProductBuild({ root, command, args, env = {}, artifact, receiptFile, frontendDist, frontendReceiptFile }, run = (command, args, options) => runBoundedCommand({ command, args, ...options, timeoutMs: 300000 })) {
  assert.ok(path.isAbsolute(root) && path.isAbsolute(artifact) && path.isAbsolute(receiptFile));
  assert.equal(path.basename(command), "cargo", "只为实际cargo构建签发receipt");
  assert.ok(args.includes("build") && !args.includes("--release") && !args.some(value => value === "--profile" || value.startsWith("--profile=")), "故障hook需要debug_assertions，不能冒充默认release");
  const featureArgs = args.flatMap((value, index) => value === "--features" ? [args[index + 1]] : value.startsWith("--features=") ? [value.slice(11)] : []);
  assert.ok(featureArgs.some(value => value?.split(/[ ,]+/).includes("host-parity")), "构建必须显式host-parity隔离故障入口");
  // 旧receipt先失效；编译失败不能复用旧证明。
  try { const stat = await lstat(receiptFile); assert.ok(stat.isFile() && !stat.isSymbolicLink() && stat.nlink === 1); await rm(receiptFile); } catch (error) { if (error.code !== "ENOENT") throw error; }
  assert.ok(path.isAbsolute(frontendDist ?? "") && path.isAbsolute(frontendReceiptFile ?? ""), "验证产品必须提供实际frontendDist及构建receipt，不能嵌旧UI");
  assert.ok(!env.TAURI_CONFIG && !process.env.TAURI_CONFIG, "验证构建不接受未记录的TAURI_CONFIG覆盖");
  const configPath = path.join(root, "apps/desktop/src-tauri/tauri.conf.json");
  const tauriConfig = JSON.parse(await readFile(configPath, "utf8"));
  const configuredDist = path.resolve(path.dirname(configPath), tauriConfig.build.frontendDist);
  assert.equal(await realpath(frontendDist), await realpath(configuredDist), "前端receipt必须绑定Tauri实际frontendDist，不能证明另一目录");
  const frontendReceipt = JSON.parse(await readFile(frontendReceiptFile, "utf8"));
  assert.equal(frontendReceipt.kind, "authority-frontend-build");
  assert.equal(frontendReceipt.distSha256, await authorityFrontendDistHash(frontendDist), "实际前端dist与其构建receipt不符");
  const before = await authorityProductSourceIdentity(root);
  assert.equal(frontendReceipt.sourceSha256, hash(JSON.stringify(before)), "前端dist不是当前源码闭包产生");
  await run(command, args, { cwd: root, env: { ...process.env, ...env } });
  assert.equal(await authorityFrontendDistHash(frontendDist), frontendReceipt.distSha256, "Cargo构建期间实际嵌入前端dist发生改变");
  const after = await authorityProductSourceIdentity(root);
  assert.deepEqual(after, before, "验证产品构建期间源码闭包改变");
  const stat = await lstat(artifact); assert.ok(stat.isFile() && !stat.isSymbolicLink() && stat.size > 0, "缺少实际编译二进制");
  const receipt = { kind: "authority-product-build", verification: true, frontend: { dist: configuredDist, config: "apps/desktop/src-tauri/tauri.conf.json", receipt: frontendReceipt }, sourceSha256: hash(JSON.stringify(before)), artifactSha256: hash(await readFile(artifact)), command, args, toolchain: env.RUSTUP_TOOLCHAIN ?? null, source: before };
  await writeFile(receiptFile, JSON.stringify(receipt, null, 2) + "\n", { flag: "wx" });
  return receipt;
}


export async function authorityFrontendDistHash(directory) {
  const entries = [];
  async function collect(relative) {
    const filename = path.join(directory, relative), stat = await lstat(filename);
    assert.ok(!stat.isSymbolicLink(), "前端dist不能含symlink");
    if (stat.isDirectory()) for (const name of (await readdir(filename)).sort()) await collect(path.posix.join(relative, name));
    else if (stat.isFile()) entries.push({ path: relative, sha256: hash(await readFile(filename)) });
  }
  await collect(""); assert.ok(entries.some(entry => entry.path === "index.html"), "前端dist缺少实际index.html");
  return hash(JSON.stringify(entries));
}

/** 必须包围实际前端构建调用；不提供从现有dist补签receipt的接口。 */
export async function executeAuthorityFrontendBuild({ root, dist, receiptFile }, actualBuild) {
  assert.equal(typeof actualBuild, "function", "必须提供真实前端构建入口");
  assert.equal(process.env.AUTHORITY_PRODUCT_SUPERVISOR, String(process.ppid), "前端构建必须由300秒进程树监督器启动");
  const deadline = Number(process.env.AUTHORITY_PRODUCT_DEADLINE);
  assert.ok(Number.isFinite(deadline) && deadline > Date.now() && deadline - Date.now() <= 300000, "缺少有效完整进程树预算");
  const before = await authorityProductSourceIdentity(root);
  await actualBuild();
  assert.deepEqual(await authorityProductSourceIdentity(root), before, "前端构建期间源码改变");
  const receipt = { kind: "authority-frontend-build", sourceSha256: hash(JSON.stringify(before)), distSha256: await authorityFrontendDistHash(dist) };
  await writeFile(receiptFile, JSON.stringify(receipt, null, 2) + "\n", { flag: "wx" });
  return receipt;
}
