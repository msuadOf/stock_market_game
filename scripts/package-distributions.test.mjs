import assert from "node:assert/strict";
import { execFile, spawn } from "node:child_process";
import { chmod, lstat, mkdir, mkdtemp, open, readFile, readdir, realpath, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { describe, it } from "node:test";
import { promisify } from "node:util";

const execute = promisify(execFile);
const script = path.join(import.meta.dirname, "package-distributions.mjs");
const nativeTarget = process.platform === "win32"
  ? `${process.arch === "arm64" ? "aarch64" : "x86_64"}-pc-windows-msvc`
  : process.platform === "darwin"
    ? `${process.arch === "arm64" ? "aarch64" : "x86_64"}-apple-darwin`
    : `${process.arch === "arm64" ? "aarch64" : "x86_64"}-unknown-linux-gnu`;
const binary = process.platform === "win32" ? "server.exe" : "server";

async function fixture(context, product = "server") {
  const root = await mkdtemp(path.join(await realpath(tmpdir()), "distribution-test-"));
  context.after(() => rm(root, { recursive: true, force: true }));
  const input = path.join(root, "target/build-artifacts/sample");
  const output = path.join(root, "target/distributions/sample");
  await mkdir(input, { recursive: true });
  await writeFile(path.join(input, "LICENSE"), "fixture license\n");
  if (product !== "desktop") {
    await writeFile(path.join(input, binary), "#!/bin/sh\nprintf fixture\\n\n");
    await chmod(path.join(input, binary), 0o751);
  }
  if (product === "webui-server") {
    await mkdir(path.join(input, "webui/assets"), { recursive: true });
    await writeFile(path.join(input, "webui/index.html"), "<html>fixture</html>");
    await writeFile(path.join(input, "webui/assets/game.wasm"), "fixture wasm");
  }
  return { root, input, output, product, target: nativeTarget };
}

async function packageFixture(fixture_, dependencies = {}) {
  const { packageDistributions } = await import("./package-distributions.mjs");
  return packageDistributions(fixture_, { root: fixture_.root, ...dependencies });
}

async function extract(archive, destination) {
  await mkdir(destination, { recursive: true });
  if (archive.endsWith(".zip") && process.platform !== "win32") {
    await execute("unzip", ["-q", archive, "-d", destination], { env: { ...process.env, LC_ALL: "en_US.UTF-8" } });
  } else {
    await execute("tar", ["-xf", archive, "-C", destination]);
  }
}

async function runCli(fixture_) {
  const stdoutFile = path.join(fixture_.root, "cli.stdout");
  const stderrFile = path.join(fixture_.root, "cli.stderr");
  const stdout = await open(stdoutFile, "wx");
  const stderr = await open(stderrFile, "wx");
  try {
    await new Promise((resolve, reject) => {
      const child = spawn(process.execPath, [script, fixture_.product, "--input", "target/build-artifacts/sample", "--output", "target/distributions/sample", "--target", fixture_.target], {
        cwd: fixture_.root, stdio: ["ignore", stdout.fd, stderr.fd],
      });
      child.on("error", reject);
      child.on("close", async (code) => {
        if (code === 0) resolve();
        else reject(new Error(`CLI exited ${code}: ${await readFile(stderrFile, "utf8")}`));
      });
    });
  } finally {
    await stdout.close();
    await stderr.close();
  }
  return { stdout: await readFile(stdoutFile, "utf8"), stderr: await readFile(stderrFile, "utf8") };
}

describe("package distributions (short fixtures, concurrency=4)", { concurrency: 4, timeout: 10000 }, () => {
  it("keeps all native installer extensions with stable ASCII platform-qualified names", async () => {
    const { desktopInstallerName } = await import("./package-distributions.mjs");
    for (const [target, extensions] of [["aarch64-apple-darwin", ["dmg"]], ["x86_64-pc-windows-msvc", ["msi", "exe"]],
      ["x86_64-unknown-linux-gnu", ["deb", "rpm", "AppImage"]]]) {
      for (const extension of extensions) {
        assert.equal(desktopInstallerName(`股票模拟游戏_0.1.0.${extension}`, target), `desktop-${target}-__0.1.0.${extension}`);
      }
    }
  });
  it("uses native BSD tar ZIP on macOS instead of unsupported Info-ZIP Unicode options", async (context) => {
    const fixture_ = await fixture(context);
    const archive = path.join(fixture_.root, "mac-system.zip");
    const calls = [];
    const { createArchive } = await import("./package-distributions.mjs");
    await createArchive({ source: fixture_.input, entries: [binary, "LICENSE"], output: archive, format: "zip" }, {
      platform: "darwin", run: async (command, args, options) => {
        calls.push({ command, args, options });
        await writeFile(archive, "archive tool fixture");
      },
    });
    assert.deepEqual(calls, [{ command: "tar", args: ["--format", "zip", "--options", "zip:hdrcharset=UTF-8", "-cf", archive, "--", binary, "LICENSE"], options: { cwd: fixture_.input } }]);
  });
  it("CLI packages only the staged server and LICENSE with real archives and executable permissions", async (context) => {
    const fixture_ = await fixture(context);
    await writeFile(path.join(fixture_.input, "old-server"), "stale binary");
    await mkdir(path.join(fixture_.root, "src"));
    await writeFile(path.join(fixture_.root, "src/private.rs"), "not a distribution");
    const result = await runCli(fixture_);
    assert.match(result.stdout, /published/i);
    const manifest = JSON.parse(await readFile(path.join(fixture_.output, "manifest.json"), "utf8"));
    assert.equal(manifest.schema, "distribution-manifest");
    assert.equal(manifest.schema_version, 1);
    assert.equal(manifest.product, "server");
    assert.equal(manifest.target, nativeTarget);
    assert.equal(manifest.files.length, 2);
    const { createHash } = await import("node:crypto");
    for (const entry of manifest.files) {
      const archive = path.join(fixture_.output, entry.name);
      const bytes = await readFile(archive);
      assert.equal(entry.bytes, bytes.length);
      assert.equal(entry.sha256, createHash("sha256").update(bytes).digest("hex"));
      const destination = path.join(fixture_.root, `extracted-${entry.name}`);
      await extract(archive, destination);
      assert.deepEqual((await readdir(destination)).sort(), ["LICENSE", binary].sort());
      assert.equal(await readFile(path.join(destination, binary), "utf8"), await readFile(path.join(fixture_.input, binary), "utf8"));
      if (process.platform !== "win32") assert.equal((await lstat(path.join(destination, binary))).mode & 0o777, 0o751);
    }
  });

  it("packages webui-server in both formats with only the published static tree", async (context) => {
    const fixture_ = await fixture(context, "webui-server");
    await writeFile(path.join(fixture_.input, "server-old.exe"), "old binary");
    const manifest = await packageFixture(fixture_);
    assert.deepEqual(manifest.files.map((entry) => path.extname(entry.name)).sort(), [".gz", ".zip"]);
    for (const entry of manifest.files) {
      const destination = path.join(fixture_.root, `extract-${entry.name}`);
      await extract(path.join(fixture_.output, entry.name), destination);
      assert.deepEqual((await readdir(destination)).sort(), ["LICENSE", binary, "webui"].sort());
      assert.equal(await readFile(path.join(destination, "webui/assets/game.wasm"), "utf8"), "fixture wasm");
    }
  });

  it("strictly parses arguments and detects the actual triple OS and architecture", async () => {
    const { parseArgs, validateTarget } = await import("./package-distributions.mjs");
    assert.deepEqual(parseArgs(["server", "--input", "target/build-artifacts/a", "--output", "target/distributions/a", "--target", nativeTarget]), {
      product: "server", input: "target/build-artifacts/a", output: "target/distributions/a", target: nativeTarget,
    });
    for (const args of [[], ["webui"], ["server", "--input", "a"], ["server", "--input", "a", "--input", "b"], ["server", "--wat", "a"]]) {
      assert.throws(() => parseArgs(args), /usage|product|required|duplicate|unknown/i);
    }
    assert.equal(validateTarget("x86_64-pc-windows-msvc", { platform: "win32", arch: "x64" }), "win32");
    assert.equal(validateTarget("aarch64-apple-darwin", { platform: "darwin", arch: "arm64" }), "darwin");
    assert.equal(validateTarget("x86_64-unknown-linux-musl", { platform: "linux", arch: "x64" }), "linux");
    assert.throws(() => validateTarget("x86_64-pc-windows-msvc", { platform: "linux", arch: "x64" }), /host mismatch/i);
    assert.throws(() => validateTarget("aarch64-unknown-linux-gnu", { platform: "linux", arch: "x64" }), /host mismatch/i);
    for (const target of ["wasm32-unknown-unknown", "x86_64-pc-windows-msvc-linux", "aarch64-apple-ios", "../linux"]) {
      assert.throws(() => validateTarget(target), /unsupported.*target/i);
    }
  });

  it("rejects mismatched host before creating output", async (context) => {
    const fixture_ = await fixture(context);
    fixture_.target = process.platform === "win32" ? "x86_64-unknown-linux-gnu" : "x86_64-pc-windows-msvc";
    await assert.rejects(packageFixture(fixture_), /host mismatch/i);
    await assert.rejects(lstat(fixture_.output), { code: "ENOENT" });
  });

  it("never replaces an existing output directory or file", async (context) => {
    const fixture_ = await fixture(context);
    await mkdir(fixture_.output, { recursive: true });
    await writeFile(path.join(fixture_.output, "user.txt"), "keep me");
    await assert.rejects(packageFixture(fixture_), /already exists/i);
    assert.equal(await readFile(path.join(fixture_.output, "user.txt"), "utf8"), "keep me");
    await rm(fixture_.output, { recursive: true });
    await writeFile(fixture_.output, "user file");
    await assert.rejects(packageFixture(fixture_), /already exists/i);
    assert.equal(await readFile(fixture_.output, "utf8"), "user file");
    await rm(fixture_.output);
    await mkdir(fixture_.output);
    await assert.rejects(packageFixture(fixture_), /already exists/i);
  });

  it("rejects input and output paths outside their direct artifact directories", async (context) => {
    const fixture_ = await fixture(context);
    for (const changes of [{ input: fixture_.root }, { output: fixture_.root }, { input: path.join(fixture_.input, "nested") }, { output: path.join(fixture_.output, "nested") }]) {
      await assert.rejects(packageFixture({ ...fixture_, ...changes }), /direct child/i);
    }
  });

  it("rejects symlink parents, input roots and selected files", { skip: process.platform === "win32" }, async (context) => {
    const fixture_ = await fixture(context);
    const outside = path.join(fixture_.root, "outside");
    await mkdir(outside);
    await symlink(outside, path.join(fixture_.root, "target/distributions"));
    await assert.rejects(packageFixture(fixture_), /symlink/i);
    assert.deepEqual(await readdir(outside), []);
    await rm(path.join(fixture_.root, "target/distributions"));
    await rm(path.join(fixture_.input, binary));
    await writeFile(path.join(outside, binary), "outside executable");
    await symlink(path.join(outside, binary), path.join(fixture_.input, binary));
    await assert.rejects(packageFixture(fixture_), /symlink/i);
    await rm(fixture_.input, { recursive: true });
    await symlink(outside, fixture_.input);
    await assert.rejects(packageFixture(fixture_), /symlink/i);
  });

  it("rejects missing, empty, non-executable binaries and malformed webui trees", async (context) => {
    const fixture_ = await fixture(context, "webui-server");
    await writeFile(path.join(fixture_.input, binary), "");
    await assert.rejects(packageFixture(fixture_), /empty/i);
    await writeFile(path.join(fixture_.input, binary), "binary");
    if (process.platform !== "win32") {
      await chmod(path.join(fixture_.input, binary), 0o644);
      await assert.rejects(packageFixture(fixture_), /executable/i);
      await chmod(path.join(fixture_.input, binary), 0o755);
    }
    await rm(path.join(fixture_.input, "webui/index.html"));
    await assert.rejects(packageFixture(fixture_), /index.html/i);
    await writeFile(path.join(fixture_.input, "webui/index.html"), "index");
    await writeFile(path.join(fixture_.input, "webui/assets/empty.js"), "");
    await assert.rejects(packageFixture(fixture_), /empty/i);
    await rm(path.join(fixture_.input, "webui/assets/empty.js"));
    await rm(path.join(fixture_.input, "LICENSE"));
    await assert.rejects(packageFixture(fixture_), /LICENSE/i);
  });

  it("cleans staging and publishes nothing when an archive command fails", async (context) => {
    const fixture_ = await fixture(context);
    let calls = 0;
    const { runArchiveCommand } = await import("./package-distributions.mjs");
    await assert.rejects(packageFixture(fixture_, { run: async (...args) => {
      calls += 1;
      if (calls === 2) throw new Error("fixture archive failure");
      return runArchiveCommand(...args);
    } }), /fixture archive failure/i);
    await assert.rejects(lstat(fixture_.output), { code: "ENOENT" });
    assert.deepEqual(await readdir(path.dirname(fixture_.output)), []);
  });

  it("preserves a user directory created while archives are being staged", async (context) => {
    const fixture_ = await fixture(context);
    const { runArchiveCommand } = await import("./package-distributions.mjs");
    let created = false;
    await assert.rejects(packageFixture(fixture_, { run: async (...args) => {
      if (!created) {
        created = true;
        await mkdir(fixture_.output);
        await writeFile(path.join(fixture_.output, "user.txt"), "racing user");
      }
      return runArchiveCommand(...args);
    } }), /already exists/i);
    assert.deepEqual(await readdir(fixture_.output), ["user.txt"]);
  });

  it("rejects desktop without installers and requires the new portable publisher contract", async (context) => {
    const fixture_ = await fixture(context, "desktop");
    await mkdir(path.join(fixture_.input, "bundle"));
    await assert.rejects(packageFixture(fixture_), /missing.*install/i);
    const formats = process.platform === "win32" ? [["msi", "msi"], ["nsis", "exe"]] : process.platform === "darwin" ? [["dmg", "dmg"]] : [["deb", "deb"], ["rpm", "rpm"], ["appimage", "AppImage"]];
    for (const [index, [folder, extension]] of formats.entries()) {
      await mkdir(path.join(fixture_.input, "bundle", folder));
      await writeFile(path.join(fixture_.input, "bundle", folder, `game.${extension}`), "installer");
      if (index < formats.length - 1) await assert.rejects(packageFixture(fixture_), /missing.*install/i);
    }
    if (process.platform === "darwin") {
      await assert.rejects(packageFixture(fixture_), /missing.*app/i);
    } else {
      await assert.rejects(packageFixture(fixture_), /portable/i);
    }
  });

  it("requires every promised native installer format and rejects empty installers", async (context) => {
    const fixture_ = await fixture(context, "desktop");
    const formats = process.platform === "win32" ? [["msi", "msi"], ["nsis", "exe"]] : process.platform === "darwin" ? [["dmg", "dmg"]] : [["deb", "deb"], ["rpm", "rpm"], ["appimage", "AppImage"]];
    for (const [folder, extension] of formats) {
      await mkdir(path.join(fixture_.input, "bundle", folder), { recursive: true });
      await writeFile(path.join(fixture_.input, "bundle", folder, `game.${extension}`), "installer");
    }
    for (const [folder, extension] of formats) {
      const installer = path.join(fixture_.input, "bundle", folder, `game.${extension}`);
      await rm(installer);
      await assert.rejects(packageFixture(fixture_), new RegExp(`missing.*${folder}`, "i"));
      await writeFile(installer, "");
      await assert.rejects(packageFixture(fixture_), /empty/i);
      await writeFile(installer, "installer");
    }
  });

  it("requires both Windows MSI and NSIS even when inspected on Linux", async (context) => {
    const fixture_ = await fixture(context, "desktop");
    const { desktopInputs } = await import("./package-distributions.mjs");
    await mkdir(path.join(fixture_.input, "bundle/msi"), { recursive: true });
    await writeFile(path.join(fixture_.input, "bundle/msi/game.msi"), "msi");
    await assert.rejects(desktopInputs(fixture_.input, "win32"), /missing.*nsis/i);
    await mkdir(path.join(fixture_.input, "bundle/nsis"));
    await writeFile(path.join(fixture_.input, "bundle/nsis/game.exe"), "nsis");
    await mkdir(path.join(fixture_.input, "portable"));
    await writeFile(path.join(fixture_.input, "portable/stock-market-game.exe"), "portable");
    await chmod(path.join(fixture_.input, "portable/stock-market-game.exe"), 0o755);
    await writeFile(path.join(fixture_.input, "portable/LICENSE"), "fixture license\n");
    assert.equal((await desktopInputs(fixture_.input, "win32")).installers.length, 2);
    await rm(path.join(fixture_.input, "bundle/msi/game.msi"));
    await assert.rejects(desktopInputs(fixture_.input, "win32"), /missing.*msi/i);
  });

  it("requires Mac dmg plus a valid Unicode app, not dmg alone, without requiring portable", async (context) => {
    const fixture_ = await fixture(context, "desktop");
    const { desktopInputs } = await import("./package-distributions.mjs");
    await mkdir(path.join(fixture_.input, "bundle/dmg"), { recursive: true });
    await writeFile(path.join(fixture_.input, "bundle/dmg/game.dmg"), "dmg");
    await assert.rejects(desktopInputs(fixture_.input, "darwin"), /missing.*app/i);
    const app = path.join(fixture_.input, "bundle/macos/股票模拟游戏.app");
    await mkdir(path.join(app, "Contents/MacOS"), { recursive: true });
    await writeFile(path.join(app, "Contents/MacOS/game"), "executable");
    await chmod(path.join(app, "Contents/MacOS/game"), 0o751);
    const inputs = await desktopInputs(fixture_.input, "darwin");
    assert.deepEqual(inputs.apps, [app]);
    assert.equal(inputs.installers.length, 1);
    await rm(path.join(fixture_.input, "bundle/dmg/game.dmg"));
    await assert.rejects(desktopInputs(fixture_.input, "darwin"), /missing.*dmg/i);
  });

  it("canonicalizes the workspace root without permitting symlinks within it", { skip: process.platform === "win32" }, async (context) => {
    const fixture_ = await fixture(context);
    const alias = path.join(fixture_.root, "workspace-alias");
    await symlink(fixture_.root, alias);
    const manifest = await packageFixture({ ...fixture_, root: alias, input: "target/build-artifacts/sample", output: "target/distributions/sample" });
    assert.equal(manifest.files.length, 2);
    await rm(fixture_.output, { recursive: true });
    await rm(path.join(fixture_.root, "target/distributions"), { recursive: true });
    await mkdir(path.join(fixture_.root, "user-data"));
    await symlink(path.join(fixture_.root, "user-data"), path.join(fixture_.root, "target/distributions"));
    await assert.rejects(packageFixture({ ...fixture_, root: alias, input: "target/build-artifacts/sample", output: "target/distributions/sample" }), /symlink/i);
  });

  it("rejects traversal, option-like archive names and pre-existing archives", async (context) => {
    const fixture_ = await fixture(context);
    const { createArchive } = await import("./package-distributions.mjs");
    const output = path.join(fixture_.root, "archive.zip");
    for (const entry of ["../server", "webui/server", "-server", ".", "..", "C:\\server", "server\nextra"]) {
      await assert.rejects(createArchive({ source: fixture_.input, entries: [entry], output, format: "zip" }), /top-level/i);
    }
    await writeFile(output, "user archive");
    await assert.rejects(createArchive({ source: fixture_.input, entries: [binary], output, format: "zip" }), /already exists/i);
    assert.equal(await readFile(output, "utf8"), "user archive");
  });

  it("copies native installers and archives only portable executable plus LICENSE", { skip: process.platform === "darwin" }, async (context) => {
    const fixture_ = await fixture(context, "desktop");
    const installers = process.platform === "win32" ? [["msi", "股票模拟游戏.msi"], ["nsis", "game.exe"]] : [["deb", "股票模拟游戏.deb"], ["rpm", "game.rpm"], ["appimage", "game.AppImage"]];
    for (const [folder, name] of installers) {
      await mkdir(path.join(fixture_.input, "bundle", folder), { recursive: true });
      await writeFile(path.join(fixture_.input, "bundle", folder, name), `installer ${name}`);
    }
    await writeFile(path.join(fixture_.input, "bundle/old.exe"), "stale binary");
    await mkdir(path.join(fixture_.input, "portable"));
    const portable = process.platform === "win32" ? "stock-market-game.exe" : "stock-market-game.AppImage";
    await writeFile(path.join(fixture_.input, "portable", portable), "portable executable");
    await chmod(path.join(fixture_.input, "portable", portable), 0o755);
    await writeFile(path.join(fixture_.input, "portable/LICENSE"), "fixture license\n");
    await writeFile(path.join(fixture_.input, "portable/old-bin"), "stale binary");
    const manifest = await packageFixture(fixture_);
    assert.equal(manifest.files.length, installers.length + 2);
    for (const [, name] of installers) {
      const extension = path.extname(name);
      const entry = manifest.files.find((file) => file.name.endsWith(extension));
      assert.match(entry.name, /^[A-Za-z0-9][A-Za-z0-9._-]*$/);
      assert.ok(entry.name.startsWith(`desktop-${nativeTarget}-`));
      assert.equal(await readFile(path.join(fixture_.output, entry.name), "utf8"), `installer ${name}`);
    }
    const archive = manifest.files.find((entry) => entry.name.endsWith(".zip"));
    const destination = path.join(fixture_.root, "portable-extract");
    await extract(path.join(fixture_.output, archive.name), destination);
    assert.deepEqual((await readdir(destination)).sort(), ["LICENSE", portable].sort());
    if (process.platform !== "win32") assert.equal((await lstat(path.join(destination, portable))).mode & 0o777, 0o755);
    await rm(fixture_.output, { recursive: true });
    const [folder, name] = installers[0];
    await writeFile(path.join(fixture_.input, "bundle", folder, `另一个游戏${path.extname(name)}`), "colliding installer");
    await assert.rejects(packageFixture(fixture_), /EEXIST|already exists/i);
    await assert.rejects(lstat(fixture_.output), { code: "ENOENT" });
  });

  it("real Unix zip and tar preserve mac app relative symlinks and executable bits", { skip: process.platform === "win32" }, async (context) => {
    const fixture_ = await fixture(context);
    const source = path.join(fixture_.root, "mac-payload");
    const app = path.join(source, "股票模拟游戏.app");
    await mkdir(path.join(app, "Contents/MacOS"), { recursive: true });
    await mkdir(path.join(app, "Contents/Frameworks/F.framework/Versions/A"), { recursive: true });
    await writeFile(path.join(app, "Contents/MacOS/game"), "executable");
    await chmod(path.join(app, "Contents/MacOS/game"), 0o751);
    await writeFile(path.join(app, "Contents/Frameworks/F.framework/Versions/A/F"), "framework");
    await symlink("A", path.join(app, "Contents/Frameworks/F.framework/Versions/Current"));
    await symlink("Versions/Current/F", path.join(app, "Contents/Frameworks/F.framework/F"));
    const { createArchive } = await import("./package-distributions.mjs");
    for (const format of ["zip", "tar.gz"]) {
      const archive = path.join(fixture_.root, `mac.${format}`);
      await createArchive({ source, entries: ["股票模拟游戏.app"], output: archive, format });
      const destination = path.join(fixture_.root, `mac-extract-${format}`);
      await extract(archive, destination);
      assert.deepEqual(await readdir(destination), ["股票模拟游戏.app"]);
      assert.equal((await lstat(path.join(destination, "股票模拟游戏.app/Contents/MacOS/game"))).mode & 0o777, 0o751);
      const { readlink } = await import("node:fs/promises");
      assert.equal(await readlink(path.join(destination, "股票模拟游戏.app/Contents/Frameworks/F.framework/F")), "Versions/Current/F");
    }
  });

  it("rejects absolute, external, dangling and non-mac symlinks before archiving", { skip: process.platform === "win32" }, async (context) => {
    const fixture_ = await fixture(context);
    const app = path.join(fixture_.root, "Game.app");
    await mkdir(app);
    await writeFile(path.join(app, "inside"), "inside");
    await writeFile(path.join(fixture_.root, "outside"), "outside");
    const { validateTree } = await import("./package-distributions.mjs");
    for (const target of [path.join(app, "inside"), "../outside", "missing"]) {
      await symlink(target, path.join(app, "link"));
      await assert.rejects(validateTree(app, { allowSymlinks: true }), /symlink/i);
      await rm(path.join(app, "link"));
    }
    await symlink("inside", path.join(app, "link"));
    await validateTree(app, { allowSymlinks: true });
    await assert.rejects(validateTree(app), /symlink/i);
  });
});
