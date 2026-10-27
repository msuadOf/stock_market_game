import assert from "node:assert/strict";
import { copyFile, link, mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import path from "node:path";
import { test } from "node:test";

async function fixture(run) {
  const scratch = path.resolve(import.meta.dirname, "../.tmp");
  await mkdir(scratch, { recursive: true });
  const root = await mkdtemp(path.join(scratch, "w0l-license-"));
  try {
    await mkdir(path.join(root, "apps/web-wasm"), { recursive: true });
    await writeFile(path.join(root, "apps/web-wasm/Cargo.toml"), '[package]\nlicense-file = "../../LICENSE"\n');
    await writeFile(path.join(root, "LICENSE"), "MIT fixture license\n");
    await run(root);
  } finally { await rm(root, { recursive: true, force: true }); }
}

const command = (root, outDir) => ({ command: "wasm-pack", args: ["build", "apps/web-wasm", "--target", "web", "--release", "--out-dir", outDir], cwd: root, env: { CARGO_BUILD_JOBS: "4" } });
const api = () => import("./wasm-pack-build.mjs");

test("wasm-pack 构建显式补齐并验证实际 pkg/LICENSE", { timeout: 10000 }, async () => fixture(async (root) => {
  const { executeWasmPackBuild } = await api();
  for (const relative of [false, true]) {
    const outDir = path.join(root, relative ? "relative/wasm/pkg" : "absolute/wasm/pkg");
    const build = command(root, relative ? path.relative(path.join(root, "apps/web-wasm"), outDir) : outDir);
    let calls = 0;
    await executeWasmPackBuild(root, build, async (received) => {
      calls += 1;
      assert.equal(received, build);
      // 复刻 wasm-pack 0.13.1 的复制目标，证明它并未提供 pkg/LICENSE。
      await copyFile(path.join(root, "LICENSE"), path.resolve(outDir, "../../LICENSE"));
    });
    assert.equal(calls, 1);
    assert.deepEqual(await readFile(path.join(outDir, "LICENSE")), await readFile(path.join(root, "LICENSE")));
  }
}));

for (const kind of ["absolute", "relative", "symlink-chain", "hardlink", "pkg-symlink", "pkg-hardlink"]) {
  test(`wasm-pack 在执行前拒绝 LICENSE 同文件别名：${kind}`, { timeout: 10000, skip: process.platform === "win32" && kind.includes("symlink") }, async () => fixture(async (root) => {
    const { executeWasmPackBuild } = await api();
    const source = path.join(root, "LICENSE");
    const outDir = path.join(root, kind === "absolute" || kind === "relative" ? ".tmp/pkg" : "safe/wasm/pkg");
    await mkdir(outDir, { recursive: true });
    const target = path.resolve(outDir, kind.startsWith("pkg-") ? "LICENSE" : "../../LICENSE");
    if (kind.includes("symlink")) {
      await symlink(source, path.join(root, "license-link"));
      await symlink(path.join(root, "license-link"), target);
    } else if (kind.includes("hardlink")) await link(source, target);
    const argument = kind === "relative" ? path.relative(path.join(root, "apps/web-wasm"), outDir) : outDir;
    await assert.rejects(executeWasmPackBuild(root, command(root, argument), async () => assert.fail("unsafe wasm-pack was invoked")), /LICENSE.*alias/i);
    assert.equal(await readFile(source, "utf8"), "MIT fixture license\n");
  }));
}

for (const change of ["missing-before", "empty-before", "changed-after", "empty-after", "missing-after", "changed-failure"]) {
  test(`LICENSE 源完整性失败阻断构建：${change}`, { timeout: 10000 }, async () => fixture(async (root) => {
    const { executeWasmPackBuild } = await api();
    const source = path.join(root, "LICENSE");
    if (change === "missing-before") await rm(source);
    if (change === "empty-before") await writeFile(source, "");
    const outDir = path.join(root, "safe/wasm/pkg");
    let calls = 0;
    await assert.rejects(executeWasmPackBuild(root, command(root, outDir), async () => {
      calls += 1;
      if (change.startsWith("missing")) await rm(source);
      else await writeFile(source, change === "empty-after" ? "" : "mutated license");
      if (change === "changed-failure") throw new Error("compiler exit 9");
    }), /LICENSE/);
    assert.equal(calls, change.endsWith("before") ? 0 : 1);
    await assert.rejects(readFile(path.join(outDir, "LICENSE")), { code: "ENOENT" });
  }));
}

test("wasm-pack 失败保留错误且不发布 pkg/LICENSE", { timeout: 10000 }, async () => fixture(async (root) => {
  const { executeWasmPackBuild } = await api();
  const outDir = path.join(root, "safe/wasm/pkg");
  await assert.rejects(executeWasmPackBuild(root, command(root, outDir), async () => { throw new Error("compiler exit 9"); }), /compiler exit 9/);
  await assert.rejects(readFile(path.join(outDir, "LICENSE")), { code: "ENOENT" });
}));

test("构建后产生的 LICENSE 别名在显式 copy 前被拒绝", { timeout: 10000 }, async () => fixture(async (root) => {
  const { executeWasmPackBuild } = await api();
  const outDir = path.join(root, "safe/wasm/pkg");
  await assert.rejects(executeWasmPackBuild(root, command(root, outDir), async () => {
    await link(path.join(root, "LICENSE"), path.join(outDir, "LICENSE"));
  }), /LICENSE.*alias/i);
  assert.equal(await readFile(path.join(root, "LICENSE"), "utf8"), "MIT fixture license\n");
}));

test("out-dir 的目录符号链也不能把 wasm-pack LICENSE 指向源文件", { timeout: 10000, skip: process.platform === "win32" }, async () => fixture(async (root) => {
  const { executeWasmPackBuild } = await api();
  await symlink(root, path.join(root, "redirect"), "dir");
  const outDir = path.join(root, "redirect/.tmp/pkg");
  await assert.rejects(executeWasmPackBuild(root, command(root, outDir), async () => assert.fail("unsafe wasm-pack was invoked")), /LICENSE.*alias/i);
  assert.equal(await readFile(path.join(root, "LICENSE"), "utf8"), "MIT fixture license\n");
}));

test("构建替换同内容源文件后仍拒绝新 inode 的 pkg hardlink 别名", { timeout: 10000 }, async () => fixture(async (root) => {
  const { executeWasmPackBuild } = await api();
  const outDir = path.join(root, "safe/wasm/pkg");
  const source = path.join(root, "LICENSE");
  await assert.rejects(executeWasmPackBuild(root, command(root, outDir), async () => {
    // 保留旧 inode，避免文件系统在重建 source 时立即复用它。
    await link(source, path.join(root, "old-license"));
    await rm(source);
    await writeFile(source, "MIT fixture license\n");
    await link(source, path.join(outDir, "LICENSE"));
  }), /LICENSE.*alias/i);
  assert.equal(await readFile(source, "utf8"), "MIT fixture license\n");
}));

test("out-dir 本身是目录符号链时按真实父目录检查 alias", { timeout: 10000, skip: process.platform === "win32" }, async () => fixture(async (root) => {
  const { executeWasmPackBuild } = await api();
  await mkdir(path.join(root, ".tmp/pkg"), { recursive: true });
  await mkdir(path.join(root, "safe/wasm"), { recursive: true });
  await symlink(path.join(root, ".tmp/pkg"), path.join(root, "out-link"), "dir");
  const outDir = path.join(root, "safe/wasm/pkg");
  await symlink(path.join(root, "out-link"), outDir, "dir");
  await assert.rejects(executeWasmPackBuild(root, command(root, outDir), async () => assert.fail("unsafe wasm-pack was invoked")), /LICENSE.*alias/i);
  assert.equal(await readFile(path.join(root, "LICENSE"), "utf8"), "MIT fixture license\n");
}));

test("畸形源 LICENSE 目录在执行前显式拒绝", { timeout: 10000 }, async () => fixture(async (root) => {
  const { executeWasmPackBuild } = await api();
  await rm(path.join(root, "LICENSE"));
  await mkdir(path.join(root, "LICENSE"));
  await assert.rejects(executeWasmPackBuild(root, command(root, path.join(root, "safe/wasm/pkg")), async () => assert.fail("invalid source was accepted")), /LICENSE.*non-empty regular/);
}));

test("畸形产物 LICENSE 目录阻断成功返回且不损坏源", { timeout: 10000 }, async () => fixture(async (root) => {
  const { executeWasmPackBuild } = await api();
  const outDir = path.join(root, "safe/wasm/pkg");
  await assert.rejects(executeWasmPackBuild(root, command(root, outDir), async () => {
    await mkdir(path.join(outDir, "LICENSE"));
  }), /LICENSE/);
  assert.equal(await readFile(path.join(root, "LICENSE"), "utf8"), "MIT fixture license\n");
}));

test("wasm-pack 部分产物失败不会补发 LICENSE", { timeout: 10000 }, async () => fixture(async (root) => {
  const { executeWasmPackBuild } = await api();
  const outDir = path.join(root, "safe/wasm/pkg");
  await assert.rejects(executeWasmPackBuild(root, command(root, outDir), async () => {
    await writeFile(path.join(outDir, "partial.js"), "incomplete");
    throw new Error("compiler partial failure");
  }), /compiler partial failure/);
  assert.equal(await readFile(path.join(outDir, "partial.js"), "utf8"), "incomplete");
  await assert.rejects(readFile(path.join(outDir, "LICENSE")), { code: "ENOENT" });
}));

test("LICENSE gate 拒绝已漂移的 manifest 或非预期 wasm-pack 参数", { timeout: 10000 }, async () => fixture(async (root) => {
  const { executeWasmPackBuild } = await api();
  const build = command(root, path.join(root, "safe/wasm/pkg"));
  const runner = async () => assert.fail("invalid command was invoked");
  for (const invalid of [
    { ...build, command: "cargo" },
    { ...build, cwd: path.join(root, "apps/web-wasm") },
    { ...build, args: ["build", "other-crate", "--out-dir", "pkg"] },
    { ...build, args: ["build", "apps/web-wasm"] },
    { ...build, args: [...build.args, "--out-dir", "other"] },
  ]) await assert.rejects(executeWasmPackBuild(root, invalid, runner), /LICENSE gate/);
  await writeFile(path.join(root, "apps/web-wasm/Cargo.toml"), '[package]\nlicense = "MIT"\n');
  await assert.rejects(executeWasmPackBuild(root, build, runner), /LICENSE gate.*license-file/);
}));

for (const content of ["", "wrong previous license"]) {
  test(`wasm-pack 产生的${content.length === 0 ? "空" : "错误"} pkg/LICENSE 被显式纠正`, { timeout: 10000 }, async () => fixture(async (root) => {
    const { executeWasmPackBuild } = await api();
    const outDir = path.join(root, "safe/wasm/pkg");
    await executeWasmPackBuild(root, command(root, outDir), async () => {
      await writeFile(path.join(outDir, "LICENSE"), content);
    });
    assert.deepEqual(await readFile(path.join(outDir, "LICENSE")), await readFile(path.join(root, "LICENSE")));
  }));
}

test("编译失败同时源 LICENSE 被修改时保留两项原始错误", { timeout: 10000 }, async () => fixture(async (root) => {
  const { executeWasmPackBuild } = await api();
  const compilerError = new Error("compiler exit 9");
  await assert.rejects(executeWasmPackBuild(root, command(root, path.join(root, "safe/wasm/pkg")), async () => {
    await writeFile(path.join(root, "LICENSE"), "changed");
    throw compilerError;
  }), (error) => {
    assert.ok(error instanceof AggregateError);
    assert.equal(error.errors.length, 2);
    assert.equal(error.errors[0], compilerError);
    assert.match(error.errors[1].message, /LICENSE source changed/);
    return true;
  });
}));

for (const targetKind of ["package", "wasm-pack"]) {
  for (const linkKind of ["symlink", "hardlink", "dangling-symlink"]) {
    test(`拒绝 ${targetKind} LICENSE 的非源 ${linkKind}，避免改写无关文件`, { timeout: 10000, skip: process.platform === "win32" && linkKind.includes("symlink") }, async () => fixture(async (root) => {
      const { executeWasmPackBuild } = await api();
      const outDir = path.join(root, "safe/wasm/pkg");
      const unrelated = path.join(root, "user-notice.txt");
      const target = path.resolve(outDir, targetKind === "package" ? "LICENSE" : "../../LICENSE");
      await mkdir(outDir, { recursive: true });
      if (linkKind !== "dangling-symlink") await writeFile(unrelated, "user-owned notice");
      if (linkKind === "hardlink") await link(unrelated, target);
      else await symlink(unrelated, target);
      await assert.rejects(executeWasmPackBuild(root, command(root, outDir), async () => assert.fail("unsafe compiler target accepted")), /LICENSE/);
      if (linkKind === "dangling-symlink") await assert.rejects(readFile(unrelated), { code: "ENOENT" });
      else assert.equal(await readFile(unrelated, "utf8"), "user-owned notice");
    }));
  }
}

for (const linkKind of ["symlink", "hardlink"]) {
  test(`构建后新出现的非源 pkg LICENSE ${linkKind} 也不能被显式 copy 改写`, { timeout: 10000, skip: process.platform === "win32" && linkKind === "symlink" }, async () => fixture(async (root) => {
    const { executeWasmPackBuild } = await api();
    const outDir = path.join(root, "safe/wasm/pkg");
    const unrelated = path.join(root, "user-notice.txt");
    await writeFile(unrelated, "user-owned notice");
    await assert.rejects(executeWasmPackBuild(root, command(root, outDir), async () => {
      if (linkKind === "hardlink") await link(unrelated, path.join(outDir, "LICENSE"));
      else await symlink(unrelated, path.join(outDir, "LICENSE"));
    }), /LICENSE/);
    assert.equal(await readFile(unrelated, "utf8"), "user-owned notice");
  }));
}
