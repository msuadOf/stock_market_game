import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { corepackProbe } from "./build-matrix.mjs";

const script = resolve("scripts/desktop/build-matrix.sh");
const bash = "/bin/bash";

function runPlanner(arguments_, environment = {}) {
  return execFileSync(bash, [script, ...arguments_], {
    encoding: "utf8",
    env: { ...process.env, ...environment },
    stdio: "pipe",
  });
}

function createCommand(directory, name, source) {
  const command = join(directory, name);
  writeFileSync(command, `#!/bin/sh\n${source}\n`);
  chmodSync(command, 0o755);
}

function runConstrainedPreflight(directory, missing, environment = {}) {
  const receipt = join(directory, "tauri-build");
  const commandPath = directory;
  writeFileSync(receipt, "");
  const commands = {
    cargo: missing === "target"
      ? `if [ "$1" = target ] && [ "$2" = list ]; then exit 0; fi\nif [ "$1" = tauri ] && [ "$2" = build ]; then printf '%s ' "$@" >"${receipt}"; exit 0; fi\nexit 0`
      : `if [ "$1" = target ] && [ "$2" = list ]; then printf '%s\\n' x86_64-pc-windows-msvc; exit 0; fi\nif [ "$1" = tauri ] && [ "$2" = build ]; then printf '%s ' "$@" >"${receipt}"; exit 0; fi\nexit 0`,
    corepack: "test \"$1\" = pnpm && test \"$2\" = --version",
    "cargo-xwin": "exit 0",
    makensis: "test \"$1\" = /CMDHELP",
    "lld-link": "exit 0",
    "llvm-rc": "exit 0",
    rustup: missing === "target"
      ? "if [ \"$1\" = target ] && [ \"$2\" = list ]; then printf '%s\\n' x86_64-unknown-linux-gnu; exit 0; fi\nexit 1"
      : "if [ \"$1\" = target ] && [ \"$2\" = list ]; then printf '%s\\n' x86_64-pc-windows-msvc; exit 0; fi\nexit 1",
  };
  if (missing === "lld") {
    delete commands["lld-link"];
  }
  else delete commands[missing];
  for (const [name, source] of Object.entries(commands)) createCommand(directory, name, source);
  createCommand(directory, "node", `exec "${process.execPath}" "$@"`);
  createCommand(directory, "test", "exec /usr/bin/test \"$@\"");
  createCommand(directory, "dirname", "exec /usr/bin/dirname \"$@\"");

  return {
    receipt,
    run: () => runPlanner(["--target", "windows"], { PATH: commandPath, ...environment }),
  };
}

test("Given every simulated host and target when dry running then it classifies all nine packaging cells", () => {
  for (const host of ["linux", "macos", "windows"]) {
    const output = runPlanner(["--dry-run", "--host", host, "--target", "all"]);

    assert.equal((output.match(/^Host: /gm) ?? []).length, 3, output);
    assert.equal((output.match(/^Tauri package: stock-market-game$/gm) ?? []).length, 3, output);
    assert.match(output, new RegExp(`Host: ${host}\\nTarget: linux`));
    assert.match(output, new RegExp(`Host: ${host}\\nTarget: macos`));
    assert.match(output, new RegExp(`Host: ${host}\\nTarget: windows`));
  }
});

test("Given the matrix when dry running then it names native bundles, constrained NSIS paths, and unsupported routes", () => {
  const linux = runPlanner(["--dry-run", "--host", "linux", "--target", "all"]);
  const macos = runPlanner(["--dry-run", "--host", "macos", "--target", "all"]);
  const windows = runPlanner(["--dry-run", "--host", "windows", "--target", "all"]);

  assert.match(linux, /Host: linux\nTarget: linux\nTauri package: stock-market-game\nStatus: supported/);
  assert.match(linux, /--bundles deb,rpm,appimage/);
  assert.match(linux, /Host: linux\nTarget: windows\nTauri package: stock-market-game\nStatus: constrained/);
  assert.match(linux, /--runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis/);
  assert.match(linux, /the LLVM lld-link driver/);
  assert.match(linux, /Preflight checks performed: none \(dry-run\)/);
  assert.match(linux, /Host: linux\nTarget: macos\nTauri package: stock-market-game\nStatus: unsupported/);
  assert.match(linux, /macOS application bundles require a Mac computer/);

  assert.match(macos, /Host: macos\nTarget: macos\nTauri package: stock-market-game\nStatus: supported/);
  assert.match(macos, /--bundles app,dmg/);
  assert.match(macos, /Host: macos\nTarget: windows\nTauri package: stock-market-game\nStatus: constrained/);
  assert.match(macos, /NSIS-only/);
  assert.match(macos, /Host: macos\nTarget: linux\nTauri package: stock-market-game\nStatus: unsupported/);

  assert.match(windows, /Host: windows\nTarget: windows\nTauri package: stock-market-game\nStatus: supported/);
  assert.match(windows, /--bundles msi,nsis/);
  assert.match(windows, /MSI remains Windows-native/);
  assert.match(windows, /Host: windows\nTarget: linux\nTauri package: stock-market-game\nStatus: unsupported/);
  assert.match(windows, /Host: windows\nTarget: macos\nTauri package: stock-market-game\nStatus: unsupported/);
});

test("Given invalid planner arguments when invoked then it fails with an actionable diagnostic", () => {
  assert.throws(
    () => runPlanner(["--target", "solaris"]),
    /--target must be one of: linux, macos, windows, all/,
  );
  assert.throws(
    () => runPlanner(["--host", "linux", "--target", "linux"]),
    /--host is only available with --dry-run/,
  );
  assert.throws(
    () => runPlanner(["--dry-run", "--host"]),
    /--host requires a value/,
  );
  assert.throws(
    () => runPlanner([]),
    /normal builds require exactly one --target/,
  );
});

test("Given a dry run when tool names resolve to an execution sentinel then it never invokes the sentinel", () => {
  const directory = mkdtempSync(join(tmpdir(), "desktop-build-matrix-test-"));
  const sentinel = join(directory, "cargo");
  const receipt = join(directory, "executed");

  writeFileSync(sentinel, `#!/bin/sh\ntouch "${receipt}"\nexit 99\n`);
  chmodSync(sentinel, 0o755);

  try {
    const output = runPlanner(["--dry-run", "--host", "linux", "--target", "windows"], {
      PATH: `${directory}:${process.env.PATH}`,
    });

    assert.match(output, /Mode: dry-run \(no commands were executed\)/);
    assert.throws(() => execFileSync("test", ["-e", receipt]));
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("Given constrained normal builds when the Windows MSVC target, lld, or resource compiler is absent then preflight rejects before Tauri", () => {
  for (const { prerequisite, missing } of [
    { prerequisite: "windows target", missing: "target" },
    { prerequisite: "lld", missing: "lld" },
    { prerequisite: "llvm-rc", missing: "llvm-rc" },
  ]) {
    const directory = mkdtempSync(join(tmpdir(), "desktop-build-matrix-preflight-"));
    const preflight = runConstrainedPreflight(directory, missing);

    try {
      assert.throws(
        preflight.run,
        new RegExp(prerequisite === "windows target" ? "x86_64-pc-windows-msvc" : prerequisite),
      );
      assert.equal(readFileSync(preflight.receipt, "utf8"), "", `${prerequisite}: cargo tauri build must not run`);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  }
});

test("Given constrained normal builds when only a versioned llvm-rc is present then preflight rejects before cargo Tauri starts", () => {
  const directory = mkdtempSync(join(tmpdir(), "desktop-build-matrix-preflight-"));
  const preflight = runConstrainedPreflight(directory, "llvm-rc");
  createCommand(directory, "llvm-rc-18", "exit 0");

  try {
    assert.throws(preflight.run, /llvm-rc/);
    assert.equal(readFileSync(preflight.receipt, "utf8"), "");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("Given constrained normal builds when NSIS accepts its documented usage probe then probes reach the structured build", () => {
  const directory = mkdtempSync(join(tmpdir(), "desktop-build-matrix-probe-"));
  const preflight = runConstrainedPreflight(directory, "none");
  for (const command of ["lld-link", "llvm-rc"]) {
    createCommand(directory, command, "test \"$#\" -eq 0");
  }

  try {
    const output = preflight.run();

    assert.match(output, /Generated command: \(cd apps\/desktop\/src-tauri && cargo tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis\)/);
    assert.equal(readFileSync(preflight.receipt, "utf8"), "tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis ");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("Given constrained normal builds when the NSIS version probe fails then cargo Tauri never starts", () => {
  const directory = mkdtempSync(join(tmpdir(), "desktop-build-matrix-nsis-failure-"));
  const preflight = runConstrainedPreflight(directory, "none");
  createCommand(directory, "makensis", "test \"$1\" = -VERSION");

  try {
    assert.throws(preflight.run, /makensis is required/);
    assert.equal(readFileSync(preflight.receipt, "utf8"), "", "NSIS probe failure must block cargo tauri build");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("Given constrained normal builds when only generic LLVM linker drivers are available then preflight rejects before cargo Tauri starts", () => {
  const directory = mkdtempSync(join(tmpdir(), "desktop-build-matrix-lld-link-"));
  const preflight = runConstrainedPreflight(directory, "lld");
  createCommand(directory, "ld.lld", "test \"$#\" -eq 0");
  createCommand(directory, "lld", "test \"$#\" -eq 0");

  try {
    assert.throws(preflight.run, /lld-link/);
    assert.equal(readFileSync(preflight.receipt, "utf8"), "");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("Given constrained normal builds when exact LLVM tools exit nonzero with no args then preflight reaches the structured build", () => {
  for (const tool of ["lld-link", "llvm-rc"]) {
    const directory = mkdtempSync(join(tmpdir(), "desktop-build-matrix-failing-llvm-"));
    const preflight = runConstrainedPreflight(directory, "none");
    createCommand(directory, tool, "exit 1");

    try {
      const output = preflight.run();
      assert.match(output, /Generated command: \(cd apps\/desktop\/src-tauri && cargo tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis\)/);
      assert.equal(readFileSync(preflight.receipt, "utf8"), "tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis ", `${tool}: structured cargo tauri build should run`);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  }
});

test("Given a Corepack command interpreter when normal preflight runs then it receives the fixed managed-pnpm probe", () => {
  const directory = mkdtempSync(join(tmpdir(), "desktop-build-matrix-corepack-cmd-"));
  const calls = [];

  try {
    assert.equal(corepackProbe("win32", (command, arguments_) => {
      calls.push({ command, arguments_ });
      return { command, result: { error: undefined, status: 0 } };
    }), true);
    assert.deepEqual(calls, [{ command: "cmd.exe", arguments_: ["/d", "/s", "/c", "corepack pnpm --version"] }]);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("Given Corepack is unavailable when normal preflight runs then cargo Tauri never starts", () => {
  const directory = mkdtempSync(join(tmpdir(), "desktop-build-matrix-corepack-"));
  const preflight = runConstrainedPreflight(directory, "none");
  createCommand(directory, "corepack", "exit 1");

  try {
    assert.throws(preflight.run, /Corepack-managed pnpm is required/);
    assert.equal(readFileSync(preflight.receipt, "utf8"), "");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
