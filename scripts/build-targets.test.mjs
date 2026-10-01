import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmod, lstat, mkdir, mkdtemp, readFile, readdir, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";

const modulePath = new URL("./build-targets.mjs", import.meta.url);
const repository = path.resolve(import.meta.dirname, "..");
const api = async () => {
  const implementation = await import(modulePath.href).catch((error) => {
    assert.fail(`target build API is not implemented: ${error.message}`);
  });
  return implementation;
};

test("parser requires an explicit target and rejects ambiguous or unsafe options", async () => {
  const { parseBuildArgs } = await api();
  assert.equal(parseBuildArgs([]).help, true);
  assert.equal(parseBuildArgs(["--help"]).help, true);
  assert.equal(parseBuildArgs(["server", "--help"]).help, true);
  assert.deepEqual(parseBuildArgs(["webui", "--jobs", "4", "--dry-run"], 8), {
    target: "webui", jobs: 4, dryRun: true, help: false,
  });
  assert.equal(parseBuildArgs(["server"], 8).jobs, 8);
  assert.equal(parseBuildArgs(["server", "--output", "target/build-artifacts/server-next"], 8).output, "target/build-artifacts/server-next");
  for (const arguments_ of [
    ["invalid"], ["desktop", "server"], ["webui", "--jobs"],
    ["server", "--jobs", "0"], ["server", "--jobs", "1.2"],
    ["server", "--jobs", "9007199254740992"], ["webui", "--out-dir", "/"],
    ["desktop", "--host", "windows"], ["webui", "--jobs", "4", "--jobs", "2"],
    ["--help", "garbage"], ["server", "--dry-run", "--dry-run"], ["server", "--help", "garbage"],
  ]) assert.throws(() => parseBuildArgs(arguments_, 8), /target|argument|jobs|option/i);
});

test("pure Server plan uses only locked Cargo release and independent executable output", async () => {
  const { createBuildPlan, parseBuildArgs } = await api();
  for (const host of ["linux", "windows", "macos"]) {
    const plan = createBuildPlan(parseBuildArgs(["server", "--jobs", "4"]), { root: repository, host, buildId: "fixture" });
    assert.equal(plan.commands.length, 2);
    assert.equal(plan.commands[0].command, "rustc");
    assert.deepEqual(plan.commands[0].args, ["-vV"]);
    assert.equal(plan.commands[1].command, "cargo");
    assert.deepEqual(plan.commands[1].args, ["build", "--locked", "--release", "-p", "server", "--bin", "server", "--jobs", "4", "--target", "<rustc-host>"]);
    assert.doesNotMatch(JSON.stringify(plan.commands), /pnpm|wasm|tauri|web-ui/);
    assert.deepEqual(plan.artifact.files, [host === "windows" ? "server.exe" : "server", "LICENSE"]);
    assert.match(plan.artifact.directory, /build-artifacts[/\\]server$/);
    assert.deepEqual(plan.artifact.startArgs, ["--services", "server"]);
  }
});

test("Web plans compile real TypeScript, shared WASM and Rust web-ui, without fixed host mode", async () => {
  const { createBuildPlan, parseBuildArgs } = await api();
  for (const target of ["webui", "webui-server"]) {
    const plan = createBuildPlan(parseBuildArgs([target, "--jobs", "4"]), { root: repository, host: "linux", buildId: "fixture" });
    const serialized = JSON.stringify(plan);
    assert.match(serialized, /--frozen-lockfile/);
    assert.match(serialized, /tsc/);
    assert.match(serialized, /--build/);
    assert.match(serialized, /wasm-pack/);
    assert.match(serialized, /check-wasm-threading/);
    assert.match(serialized, /check-web-release-wasm/);
    assert.match(serialized, /web-ui/);
    assert.doesNotMatch(serialized, /VITE_HOST|vite preview|regression|\.tmp/);
    const wasm = plan.commands.find(({ command }) => command === "wasm-pack");
    assert.match(wasm.args[wasm.args.indexOf("--out-dir") + 1], /apps[/\\]web-wasm[/\\]wasm-pack-output[/\\]fixture[/\\]wasm[/\\]pkg$/);
    assert.equal(wasm.env.CARGO_BUILD_JOBS, "4");
    assert.equal(wasm.env.RUSTUP_TOOLCHAIN, "nightly-2026-09-05");
    assert.deepEqual(plan.artifact.files, ["server", "webui/", "LICENSE"]);
    assert.deepEqual(plan.artifact.startArgs, ["--services", target === "webui" ? "webui" : "all"]);
    assert.match(plan.artifact.directory, new RegExp(`${target}$`));
  }
});

test("Desktop uses native Tauri installers from the existing matrix rather than a bin", async () => {
  const { createBuildPlan, parseBuildArgs } = await api();
  for (const [host, bundles] of [["linux", "deb,rpm,appimage"], ["windows", "msi,nsis"], ["macos", "app,dmg"]]) {
    const plan = createBuildPlan(parseBuildArgs(["desktop", "--jobs", "4"]), { root: repository, host, buildId: "fixture" });
    const bundle = plan.commands.find(({ args }) => args[0] === "tauri");
    assert.equal(bundle.command, "cargo");
    assert.equal(bundle.args[bundle.args.indexOf("--bundles") + 1], bundles);
    assert.match(JSON.stringify(bundle), /beforeBuildCommand/);
    assert.equal(bundle.env.CARGO_BUILD_JOBS, "4");
    assert.deepEqual(plan.artifact.files, ["bundle/", ...(host === "macos" ? [] : ["portable/"]), "LICENSE"]);
    assert.doesNotMatch(JSON.stringify(plan), /cargo-xwin|VITE_HOST/);
  }
});

test("dry-run does not run compilers or create any artifact and prints requirements/deadline", async () => {
  const { main } = await api();
  const root = await mkdtemp(path.join(tmpdir(), "build-plan-"));
  let output = "";
  try {
    await main(["webui", "--dry-run", "--jobs", "4"], {
      root, host: "linux", output: (message) => { output += message; },
      run: () => assert.fail("dry-run executed a build"),
    });
    assert.match(output, /dry-run.*no.*execut/i);
    assert.match(output, /300000ms/);
    assert.match(output, /--features web-ui/);
    assert.match(output, /--services webui/);
    assert.match(output, /Node.*Rust/i);
    await assert.rejects(lstat(path.join(root, "target")), { code: "ENOENT" });
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("artifact publisher refuses existing paths, stale files and symlink parents", async () => {
  const { publishArtifact } = await api();
  const root = await mkdtemp(path.join(tmpdir(), "build-artifact-"));
  try {
    const source = path.join(root, "fresh-server");
    await writeFile(path.join(root, "LICENSE"), "root license");
    await writeFile(source, "new executable");
    const output = path.join(root, "target/build-artifacts/server");
    await publishArtifact({ root, target: "server", executable: source });
    assert.equal(await readFile(path.join(output, "server"), "utf8"), "new executable");
    assert.equal(await readFile(path.join(output, "LICENSE"), "utf8"), "root license");
    await writeFile(path.join(output, "user-save"), "keep");
    await assert.rejects(publishArtifact({ root, target: "server", executable: source }), /exist|refus/i);
    assert.equal(await readFile(path.join(output, "user-save"), "utf8"), "keep");
    await assert.rejects(publishArtifact({ root, target: "unknown", executable: source }), /target/);
    await assert.rejects(publishArtifact({ root, target: "webui", executable: path.join(root, "missing"), webRoot: root }), /ENOENT|missing/);
    await assert.rejects(lstat(path.join(root, "target/build-artifacts/webui")), { code: "ENOENT" });
    const other = path.join(root, "other");
    await mkdir(other);
    await rm(path.join(root, "target/build-artifacts"), { recursive: true });
    await symlink(other, path.join(root, "target/build-artifacts"), "dir");
    await assert.rejects(publishArtifact({ root, target: "server", executable: source }), /symbolic|symlink/i);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("shell Server route works without Node and does not touch frontend or an existing artifact", { skip: process.platform === "win32" }, async () => {
  const root = await mkdtemp(path.join(tmpdir(), "build-no-node-"));
  try {
    await mkdir(path.join(root, "scripts"));
    await writeFile(path.join(root, "LICENSE"), "root license");
    for (const filename of ["build.sh", "server-build.sh"]) {
      const source = await readFile(path.join(repository, "scripts", filename), "utf8").catch((error) => assert.fail(`${filename} missing: ${error.message}`));
      await writeFile(path.join(root, "scripts", filename), source);
    }
    const bin = path.join(root, "tools");
    await mkdir(bin);
    for (const name of ["bash", "env", "dirname", "mkdir", "cp", "mv", "rm", "date", "getconf", "timeout", "sleep", "kill"]) {
      const executable = spawnSync("/bin/sh", ["-c", `command -v ${name}`], { encoding: "utf8" }).stdout.trim();
      if (executable) await symlink(executable, path.join(bin, name));
    }
    await writeFile(path.join(bin, "rustc"), '#!/bin/sh\nprintf "host: x86_64-unknown-linux-gnu\\n"\n');
    await chmod(path.join(bin, "rustc"), 0o755);
    const cargoSource = '#!/bin/sh\nprintf "%s\\n" "$*" > cargo-receipt\ntarget=other-configured-target\nprevious=\nfor argument in "$@"; do\nif [ "$previous" = --target ]; then target=$argument; fi\nprevious=$argument\ndone\n/bin/mkdir -p "$CARGO_TARGET_DIR/$target/release"\nprintf "native executable" > "$CARGO_TARGET_DIR/$target/release/server"\n';
    await writeFile(path.join(bin, "cargo"), cargoSource);
    await chmod(path.join(bin, "cargo"), 0o755);
    const run = (args) => spawnSync("/bin/bash", [path.join(root, "scripts/build.sh"), "server", ...args], {
      env: { ...process.env, PATH: bin }, encoding: "utf8", timeout: 2500,
    });
    const dryRun = run(["--dry-run", "--jobs", "4"]);
    assert.equal(dryRun.status, 0, dryRun.stderr);
    assert.match(dryRun.stdout, /cargo build.*--release.*--jobs 4/);
    await assert.rejects(lstat(path.join(root, "cargo-receipt")), { code: "ENOENT" });
    const built = run(["--jobs", "4"]);
    assert.equal(built.status, 0, built.stderr);
    assert.equal(await readFile(path.join(root, "cargo-receipt"), "utf8"), "build --locked --release -p server --bin server --jobs 4 --target x86_64-unknown-linux-gnu\n");
    assert.equal(await readFile(path.join(root, "target/build-artifacts/server/server"), "utf8"), "native executable");
    assert.notEqual(run(["--jobs", "4"]).status, 0);
    assert.equal(run(["--jobs", "4", "--output", "target/build-artifacts/server-next"]).status, 0);
    assert.notEqual(run(["--jobs", "0"]).status, 0);
    assert.equal(run(["--help"]).status, 0);
    assert.notEqual(run(["--help", "garbage"]).status, 0);
    assert.notEqual(run(["--dry-run", "--dry-run"]).status, 0);
    const invalidHelp = spawnSync("/bin/bash", [path.join(root, "scripts/build.sh"), "--help", "garbage"], { env: { ...process.env, PATH: bin }, encoding: "utf8", timeout: 1000 });
    assert.notEqual(invalidHelp.status, 0);
    assert.deepEqual(await readdir(path.join(root, "target/build-work")), []);
    await writeFile(path.join(bin, "cargo"), '#!/bin/sh\nexit 9\n');
    const failed = run(["--jobs", "4", "--output", "target/build-artifacts/server-failure"]);
    assert.notEqual(failed.status, 0);
    await assert.rejects(lstat(path.join(root, "target/build-artifacts/server-failure")), { code: "ENOENT" });
    assert.deepEqual(await readdir(path.join(root, "target/build-work")), []);
    await writeFile(path.join(bin, "cargo"), '#!/bin/sh\nsleep 30 &\nwait\n');
    await rm(path.join(bin, "timeout"));
    await writeFile(path.join(bin, "timeout"), '#!/bin/bash\nif [ "$1" = --signal=KILL ]; then exec /usr/bin/timeout --signal=KILL 2s "${@:3}"; fi\nexec /usr/bin/timeout --signal=TERM --kill-after=0.05s 0.15s "${@:4}"\n');
    await chmod(path.join(bin, "timeout"), 0o755);
    const timedOut = run(["--jobs", "4", "--output", "target/build-artifacts/server-timeout"]);
    assert.notEqual(timedOut.status, 0);
    assert.deepEqual(await readdir(path.join(root, "target/build-work")), []);
    await assert.rejects(lstat(path.join(root, "target/build-artifacts/server-timeout")), { code: "ENOENT" });
    await writeFile(path.join(bin, "cargo"), cargoSource);
    await rm(path.join(bin, "timeout"));
    await symlink("/usr/bin/timeout", path.join(bin, "timeout"));
    await rm(path.join(bin, "mv"));
    await writeFile(path.join(bin, "mv"), '#!/bin/sh\nif [ "$1" = --version ]; then exit 0; fi\nfor destination in "$@"; do :; done\n/bin/mkdir "$destination"\nprintf "keep" > "$destination/user-save"\nexit 0\n');
    await chmod(path.join(bin, "mv"), 0o755);
    const raced = run(["--jobs", "4", "--output", "target/build-artifacts/server-race"]);
    assert.notEqual(raced.status, 0);
    assert.match(raced.stderr, /no-clobber move was skipped/);
    assert.equal(await readFile(path.join(root, "target/build-artifacts/server-race/user-save"), "utf8"), "keep");
    assert.deepEqual(await readdir(path.join(root, "target/build-work")), []);
    await assert.rejects(lstat(path.join(root, "apps")), { code: "ENOENT" });
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("batch routes Server before Node and supplies a no-Node bounded Cargo release entry", async () => {
  const batch = await readFile(path.join(repository, "scripts/build.bat"), "utf8");
  assert.match(batch, /server-build\.bat/i);
  assert.ok(batch.indexOf("server-build.bat") < batch.indexOf("node "));
  const server = await readFile(path.join(repository, "scripts/server-build.ps1"), "utf8").catch((error) => assert.fail(`PowerShell implementation missing: ${error.message}`));
  assert.match(server, /300000/);
  assert.match(server, /--release/);
  assert.match(server, /--jobs/);
  assert.doesNotMatch(server, /pnpm|wasm-pack|cargo tauri/);
});

test("fresh frontend staging excludes old assets, keeps TS build-info local, and preserves WASM license", async () => {
  const { stageFrontend, createBuildPlan, parseBuildArgs } = await api();
  const root = await mkdtemp(path.join(tmpdir(), "frontend-stage-"));
  try {
    const source = path.join(root, "apps/web");
    await mkdir(path.join(source, "node_modules/.tmp"), { recursive: true });
    await mkdir(path.join(source, "node_modules/react"));
    await writeFile(path.join(source, "node_modules/react/package.json"), '{}');
    await writeFile(path.join(source, "node_modules/.tmp/user-data"), "keep");
    await writeFile(path.join(root, "LICENSE"), "original license");
    await writeFile(path.join(source, "index.html"), "fresh entry");
    await writeFile(path.join(source, ".env.production"), "VITE_ENGINE_HOST=remote");
    for (const name of ["dist", "wasm-pkg", "wasm-diagnostics-pkg"]) {
      await mkdir(path.join(source, name));
      await writeFile(path.join(source, name, "stale"), "never package");
    }
    const plan = createBuildPlan(parseBuildArgs(["webui", "--jobs", "4"]), { root, host: "linux", buildId: "test" });
    await mkdir(plan.work, { recursive: true });
    await mkdir(plan.wasmPackage, { recursive: true });
    await writeFile(path.join(plan.wasmPackage, "web_wasm.js"), "fresh wasm glue");
    await stageFrontend(plan);
    assert.equal(await readFile(path.join(plan.frontend, "wasm-pkg/web_wasm.js"), "utf8"), "fresh wasm glue");
    for (const name of ["dist", "wasm-diagnostics-pkg", ".env.production", "wasm-pkg/stale", "node_modules/.tmp"]) {
      await assert.rejects(lstat(path.join(plan.frontend, name)), { code: "ENOENT" });
    }
    assert.equal(await readFile(path.join(plan.work, "LICENSE"), "utf8"), "original license");
    assert.equal(await readFile(path.join(source, "wasm-pkg/stale"), "utf8"), "never package");
    assert.equal(await readFile(path.join(source, "node_modules/.tmp/user-data"), "utf8"), "keep");
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("repeat Web packaging uses a fresh output and never carries old Vite assets", async () => {
  const { publishArtifact } = await api();
  const root = await mkdtemp(path.join(tmpdir(), "web-package-"));
  try {
    const executable = path.join(root, "server.exe");
    const webRoot = path.join(root, "fresh-dist");
    await writeFile(path.join(root, "LICENSE"), "root license");
    await writeFile(executable, "binary");
    await mkdir(webRoot);
    await writeFile(path.join(webRoot, "index.html"), "fresh html");
    await writeFile(path.join(webRoot, "current.js"), "current asset");
    const oldOutput = path.join(root, "target/build-artifacts/webui");
    await mkdir(oldOutput, { recursive: true });
    await writeFile(path.join(oldOutput, "user-save"), "keep");
    const output = "target/build-artifacts/webui-next";
    const directory = await publishArtifact({ root, target: "webui", executable, webRoot, output, host: "windows" });
    assert.deepEqual((await readdir(directory)).sort(), ["LICENSE", "server.exe", "webui"]);
    assert.equal(await readFile(path.join(directory, "webui/current.js"), "utf8"), "current asset");
    assert.equal(await readFile(path.join(oldOutput, "user-save"), "utf8"), "keep");
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("shared supervisor bounds the whole Node build rather than resetting per phase", async () => {
  const { main } = await api();
  const commands = [];
  const root = await mkdtemp(path.join(tmpdir(), "build-supervisor-"));
  try {
    await main(["webui-server", "--jobs", "4"], {
      root, host: "linux", output: () => {}, run: async (command) => { commands.push(command); },
    });
    assert.deepEqual(await readdir(path.join(root, "target/build-work")), []);
    assert.deepEqual(await readdir(path.join(root, "apps/web-wasm/wasm-pack-output")), []);
  } finally { await rm(root, { recursive: true, force: true }); }
  assert.equal(commands.length, 1);
  assert.ok(commands[0].timeoutMs <= 299000 && commands[0].timeoutMs > 298000);
  assert.match(commands[0].args[0], /build-targets-worker/);
  assert.deepEqual(commands[0].args.slice(1), ["webui-server", "--jobs", "4"]);
});

test("supervisor kills a worker without finally and removes its owned work/WASM trees", async () => {
  const { superviseBuild, createBuildPlan, parseBuildArgs } = await api();
  const { runBoundedCommand } = await import("./run-with-deadline.mjs");
  const root = await mkdtemp(path.join(tmpdir(), "build-timeout-"));
  const plan = createBuildPlan(parseBuildArgs(["webui", "--jobs", "4"]), { root, host: "linux", buildId: "timeout" });
  try {
    await mkdir(plan.cargoDirectory, { recursive: true });
    await writeFile(path.join(plan.cargoDirectory, "retained-cache"), "keep cache");
    const source = `const fs=require('node:fs');fs.writeFileSync(${JSON.stringify(path.join(root, "worker-started"))},'started');fs.writeFileSync(${JSON.stringify(path.join(plan.work, "partial"))},'work');fs.writeFileSync(${JSON.stringify(path.join(plan.wasmRoot, "LICENSE"))},'license');setInterval(()=>{},1000);`;
    await assert.rejects(superviseBuild(plan, ["webui", "--jobs", "4"], {
      timeoutMs: 1000, run: (command) => runBoundedCommand({ ...command, command: process.execPath, args: ["-e", source], timeoutMs: 300, cleanupReserveMs: 50 }),
    }), /budget|deadline|terminated/);
    assert.equal(await readFile(path.join(root, "worker-started"), "utf8"), "started");
    await assert.rejects(lstat(plan.work), { code: "ENOENT" });
    await assert.rejects(lstat(plan.wasmRoot), { code: "ENOENT" });
    await assert.rejects(lstat(plan.artifact.directory), { code: "ENOENT" });
    assert.equal(await readFile(path.join(plan.cargoDirectory, "retained-cache"), "utf8"), "keep cache");
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("native target overrides Cargo configuration and never publishes the stale unqualified exe", async () => {
  const { executeBuild, parseBuildArgs } = await api();
  const root = await mkdtemp(path.join(tmpdir(), "build-native-target-"));
  try {
    await mkdir(path.join(root, "target/build-cache/server/release"), { recursive: true });
    await writeFile(path.join(root, "target/build-cache/server/release/server"), "stale exe");
    await writeFile(path.join(root, "LICENSE"), "root license");
    await executeBuild(parseBuildArgs(["server", "--jobs", "4"]), {
      root, host: "linux", buildId: "native", commandRunner: async (command) => {
        if (command.command === "rustc") return "host: x86_64-unknown-linux-gnu\n";
        const nativeTarget = command.args[command.args.indexOf("--target") + 1];
        assert.equal(nativeTarget, "x86_64-unknown-linux-gnu");
        const directory = path.join(command.env.CARGO_TARGET_DIR, nativeTarget, "release");
        await mkdir(directory, { recursive: true });
        await writeFile(path.join(directory, "server"), "fresh native executable");
      },
    });
    assert.equal(await readFile(path.join(root, "target/build-artifacts/server/server"), "utf8"), "fresh native executable");
    assert.equal(await readFile(path.join(root, "target/build-cache/server/release/server"), "utf8"), "stale exe");
    await assert.rejects(lstat(path.join(root, "target/build-work/server-native")), { code: "ENOENT" });
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("publisher cleans its exclusively-created stage when copying fails without touching user files", { skip: process.platform === "win32" }, async () => {
  const { publishArtifact } = await api();
  const root = await mkdtemp(path.join(tmpdir(), "publish-failure-"));
  const executable = path.join(root, "unreadable-server");
  try {
    await writeFile(path.join(root, "LICENSE"), "root license");
    await writeFile(executable, "binary");
    await chmod(executable, 0o000);
    await assert.rejects(publishArtifact({ root, target: "server", executable }), /EACCES|permission/i);
    assert.deepEqual(await readdir(path.join(root, "target/build-artifacts")), []);
    assert.equal(await readFile(path.join(root, "LICENSE"), "utf8"), "root license");
  } finally {
    await chmod(executable, 0o600);
    await rm(root, { recursive: true, force: true });
  }
});

test("production compiler child does not inherit build-machine remote mode, address or token", async () => {
  const { executeBuildCommand } = await api();
  const variables = { VITE_ENGINE_HOST: "remote", VITE_REMOTE_BASE_URL: "https://build-machine.example", VITE_REMOTE_TOKEN: "private-build-machine-token" };
  const previous = Object.fromEntries(Object.keys(variables).map((name) => [name, process.env[name]]));
  try {
    Object.assign(process.env, variables);
    const output = await executeBuildCommand({
      action: "native-target", command: process.execPath,
      args: ["-e", 'require("node:fs").writeSync(1,JSON.stringify(Object.fromEntries(["VITE_ENGINE_HOST","VITE_REMOTE_BASE_URL","VITE_REMOTE_TOKEN","CARGO_BUILD_JOBS"].filter(name=>name in process.env).map(name=>[name,process.env[name]]))))'],
      cwd: repository, env: { CARGO_BUILD_JOBS: "4" },
    });
    assert.deepEqual(JSON.parse(output), { CARGO_BUILD_JOBS: "4" });
    for (const [name, value] of Object.entries(variables)) assert.equal(process.env[name], value);
  } finally {
    for (const [name, value] of Object.entries(previous)) {
      if (value === undefined) delete process.env[name]; else process.env[name] = value;
    }
  }
});

test("failed Cargo never publishes a stale executable from the reusable cache", async () => {
  const { executeBuild, parseBuildArgs } = await api();
  const root = await mkdtemp(path.join(tmpdir(), "build-failure-"));
  try {
    await mkdir(path.join(root, "target/build-cache/server/release"), { recursive: true });
    await writeFile(path.join(root, "target/build-cache/server/release/server"), "stale executable");
    await writeFile(path.join(root, "LICENSE"), "root license");
    await assert.rejects(executeBuild(parseBuildArgs(["server", "--jobs", "4"]), {
      root, host: "linux", buildId: "failure", commandRunner: async () => { throw new Error("fixture Cargo exit 9"); },
    }), /Cargo exit 9/);
    await assert.rejects(lstat(path.join(root, "target/build-artifacts/server")), { code: "ENOENT" });
    await assert.rejects(lstat(path.join(root, "target/build-work/server-failure")), { code: "ENOENT" });
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("native bundles preserve contained macOS symlinks but reject external bundle links", async () => {
  const { publishArtifact } = await api();
  const root = await mkdtemp(path.join(tmpdir(), "bundle-package-"));
  try {
    await writeFile(path.join(root, "LICENSE"), "root license");
    const bundleRoot = path.join(root, "fresh-bundle");
    await mkdir(path.join(bundleRoot, "macos/Game.app/Contents/Resources"), { recursive: true });
    await mkdir(path.join(bundleRoot, "macos/Game.app/Contents/MacOS"));
    await writeFile(path.join(bundleRoot, "macos/Game.app/Contents/MacOS/game"), "native binary");
    await symlink("MacOS/game", path.join(bundleRoot, "macos/Game.app/Contents/binary"));
    const directory = await publishArtifact({ root, target: "desktop", bundleRoot, host: "macos" });
    assert.equal((await lstat(path.join(directory, "bundle/macos/Game.app/Contents/binary"))).isSymbolicLink(), true);
    await symlink(path.join(bundleRoot, "macos/Game.app/Contents/MacOS/game"), path.join(bundleRoot, "absolute-contained"));
    await assert.rejects(publishArtifact({ root, target: "desktop", bundleRoot, output: "target/build-artifacts/desktop-absolute" }), /absolute|relocat/i);
    await rm(path.join(bundleRoot, "absolute-contained"));
    await symlink(path.join(root, "LICENSE"), path.join(bundleRoot, "external-license"));
    await assert.rejects(publishArtifact({ root, target: "desktop", bundleRoot, output: "target/build-artifacts/desktop-next" }), /escaped/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("prebuilt UI skips Node/WASM builds and native desktop uses a cache with freshly generated unsigned bundles", async () => {
  const { createBuildPlan, parseBuildArgs } = await api();
  for (const host of ["linux", "windows", "macos"]) {
    const options = parseBuildArgs(["desktop", "--frontend-dist", "target/ci-frontend", "--jobs", "4"]);
    const plan = createBuildPlan(options, { root: repository, host, buildId: "prebuilt" });
    assert.equal(plan.frontendDist, path.join(repository, "target/ci-frontend"));
    assert.equal(plan.cargoDirectory, path.join(repository, "target/build-cache/desktop"));
    assert.deepEqual(plan.commands.map(({ action, command }) => action ?? command), ["native-target", "check-frontend", "clear-bundles", "cargo"]);
    const native = plan.commands.at(-1);
    assert.ok(native.args.includes("--no-sign"));
    assert.equal(native.args.includes("--no-binary-patching"), host === "macos");
    assert.equal(JSON.parse(native.args[native.args.indexOf("--config") + 1]).build.frontendDist, plan.frontendDist);
    assert.equal(plan.artifact.files.includes("portable/"), host !== "macos");
  }
  assert.throws(() => parseBuildArgs(["server", "--frontend-dist", "target/ui"]), /server|frontend/i);
  assert.throws(() => createBuildPlan(parseBuildArgs(["webui", "--frontend-dist", "../outside"]), { root: repository }), /frontend|outside/i);
});

test("prebuilt UI is checked before native compilation and stale cached bundles cannot be republished", async () => {
  const { executeBuild, parseBuildArgs } = await api();
  const root = await mkdtemp(path.join(tmpdir(), "ci-native-build-"));
  const frontend = path.join(root, "target/ci-frontend");
  const bundle = path.join(root, "target/build-cache/desktop/x86_64-pc-windows-msvc/release/bundle/msi");
  try {
    await writeFile(path.join(root, "LICENSE"), "license");
    await mkdir(frontend, { recursive: true });
    await writeFile(path.join(frontend, "index.html"), "entry");
    const options = parseBuildArgs(["desktop", "--frontend-dist", "target/ci-frontend", "--jobs", "4"]);
    const probeOnly = async (command) => {
      assert.equal(command.command, "rustc", "bad frontend must prevent compilation");
      return "host: x86_64-pc-windows-msvc\n";
    };
    await assert.rejects(executeBuild(options, { root, host: "windows", buildId: "invalid", commandRunner: probeOnly }), /WASM/);
    await writeFile(path.join(frontend, "web_wasm_bg.wasm"), Buffer.from([0, 97, 115, 109, 1, 0, 0, 0]));
    await mkdir(bundle, { recursive: true });
    await writeFile(path.join(bundle, "stale.msi"), "stale");
    await executeBuild(options, { root, host: "windows", buildId: "fresh", commandRunner: async (command) => {
      if (command.command === "rustc") return "host: x86_64-pc-windows-msvc\n";
      await assert.rejects(lstat(bundle), { code: "ENOENT" });
      await mkdir(bundle, { recursive: true });
      await writeFile(path.join(bundle, "fresh.msi"), "fresh");
      await writeFile(path.join(bundle, "../../stock-market-game.exe"), "portable binary");
    } });
    const output = path.join(root, "target/build-artifacts/desktop");
    assert.equal(await readFile(path.join(output, "portable/stock-market-game.exe"), "utf8"), "portable binary");
    assert.equal(await readFile(path.join(output, "portable/LICENSE"), "utf8"), "license");
    assert.deepEqual(await readdir(path.join(output, "bundle/msi")), ["fresh.msi"]);
    assert.equal(await readFile(path.join(frontend, "index.html"), "utf8"), "entry");
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("Linux portable desktop contains its real AppImage and preserves executable permissions", async () => {
  const { publishArtifact } = await api();
  const root = await mkdtemp(path.join(tmpdir(), "portable-desktop-"));
  try {
    await writeFile(path.join(root, "LICENSE"), "license");
    const bundleRoot = path.join(root, "bundle");
    await mkdir(path.join(bundleRoot, "appimage"), { recursive: true });
    const appimage = path.join(bundleRoot, "appimage/game.AppImage");
    await writeFile(appimage, "real appimage");
    await chmod(appimage, 0o755);
    const directory = await publishArtifact({ root, target: "desktop", host: "linux", bundleRoot });
    const portable = path.join(directory, "portable/stock-market-game.AppImage");
    assert.equal(await readFile(portable, "utf8"), "real appimage");
    if (process.platform !== "win32") assert.equal((await lstat(portable)).mode & 0o111, 0o111);
    await writeFile(path.join(bundleRoot, "appimage/ambiguous.AppImage"), "other");
    await assert.rejects(publishArtifact({ root, target: "desktop", host: "linux", bundleRoot, output: "target/build-artifacts/desktop-next" }), /AppImage|exactly/i);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("prebuilt UI cannot escape the workspace through a symbolic-link parent", { skip: process.platform === "win32" }, async () => {
  const { executeBuild, parseBuildArgs } = await api();
  const root = await mkdtemp(path.join(tmpdir(), "prebuilt-parent-"));
  try {
    await writeFile(path.join(root, "LICENSE"), "license");
    await mkdir(path.join(root, "source/dist"), { recursive: true });
    await writeFile(path.join(root, "source/dist/index.html"), "html");
    await writeFile(path.join(root, "source/dist/web_wasm_bg.wasm"), Buffer.from([0, 97, 115, 109, 1, 0, 0, 0]));
    await mkdir(path.join(root, "target"));
    await symlink(path.join(root, "source"), path.join(root, "target/ui-link"), "dir");
    const options = parseBuildArgs(["webui-server", "--frontend-dist", "target/ui-link/dist"]);
    await assert.rejects(executeBuild(options, { root, host: "linux", buildId: "link", commandRunner: async (command) => {
      if (command.command === "rustc") return "host: x86_64-unknown-linux-gnu\n";
      assert.fail("unexpected compiler invocation");
    } }), /symbolic|symlink/i);
  } finally { await rm(root, { recursive: true, force: true }); }
});
