import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

function runNative(command: string, args: string[], input?: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const child = execFile(command, args, { encoding: "utf8", timeout: 3000, killSignal: "SIGKILL" }, (error, stdout, stderr) => {
      if (error !== null) reject(new Error(`${command} 执行失败：${error.message}\n${stderr}`, { cause: error }));
      else resolve(stdout);
    });
    if (input !== undefined) {
      assert.ok(child.stdin);
      child.stdin.end(input);
    }
  });
}

test("真实 Rust glob 使用实际 fs 配置与命令 scope，支持隐藏父目录而不授权其他文件", async () => {
  const library = process.env.STOCK_GAME_SCOPE_GLOB_RLIB;
  assert.ok(library, "原生 scope 短测需 STOCK_GAME_SCOPE_GLOB_RLIB 指向已编译的真实 glob rlib；不下载或重建依赖");
  const configuration = JSON.parse(readFileSync(new URL("../../../desktop/src-tauri/tauri.conf.json", import.meta.url), "utf8")) as {
    plugins?: { fs?: { requireLiteralLeadingDot?: boolean } };
  };
  const capability = JSON.parse(readFileSync(new URL("../../../desktop/src-tauri/capabilities/default.json", import.meta.url), "utf8")) as {
    permissions: (string | { identifier: string; allow?: string[] })[];
  };
  const configured = configuration.plugins?.fs?.requireLiteralLeadingDot;
  if (configured !== undefined) assert.equal(typeof configured, "boolean");
  const patterns = ["open", "write-text-file", "rename", "remove"].map((command) => {
    const permission = capability.permissions.find((entry) => typeof entry !== "string" && entry.identifier === `fs:allow-${command}`);
    assert.ok(permission !== undefined && typeof permission !== "string");
    assert.ok(permission.allow);
    assert.equal(permission.allow.length, 1);
    return permission.allow[0]!;
  });
  const workspace = fileURLToPath(new URL("../../../../", import.meta.url));
  const temporary = mkdtempSync(path.join(workspace, ".tmp", "file-target-native-scope-"));
  const binary = path.join(temporary, "scope-check");
  const source = `
extern crate glob;
fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let require_literal_leading_dot = match arguments[1].as_str() {
        "default" => cfg!(unix),
        "true" => true,
        "false" => false,
        invalid => panic!("invalid fs option: {invalid}"),
    };
    let options = glob::MatchOptions {
        require_literal_separator: true,
        require_literal_leading_dot,
        ..Default::default()
    };
    let filename = "stock-game-day-end-12345678-1234-4123-8123-123456789abc.tmp";
    for raw in &arguments[2..] {
        let pattern = glob::Pattern::new(raw).expect("valid command scope");
        for directory in ["/home/user/saves", "/home/user/.saves", "/home/.user/.saves", "/home/user/.saves/nested/.games"] {
            let temporary_path = format!("{directory}/{filename}");
            assert!(pattern.matches_with(&temporary_path, options), "hidden target temporary path rejected: {temporary_path}");
            for ordinary in ["game.json", ".game.json", "other.tmp", "stock-game-day-end-not-a-uuid.tmp"] {
                let ordinary_path = format!("{directory}/{ordinary}");
                assert!(!pattern.matches_with(&ordinary_path, options), "unselected ordinary file granted: {ordinary_path}");
            }
            assert!(!pattern.matches_with(directory, options), "entire directory granted: {directory}");
        }
    }
    println!("native matcher accepted normal and nested hidden targets; denied ordinary files and directories for all four command scopes");
}
`;
  try {
    await runNative("rustc", [
      "--edition=2021", "-", "--crate-name", "file_target_native_scope",
      "--extern", `glob=${path.resolve(library)}`, "-L", `dependency=${path.dirname(path.resolve(library))}`,
      "-o", binary,
    ], source);
    let checked = "";
    await assert.doesNotReject(async () => {
      checked = await runNative(binary, [configured === undefined ? "default" : String(configured), ...patterns]);
    });
    assert.match(checked, /accepted normal and nested hidden targets/);
  } finally {
    rmSync(temporary, { recursive: true, force: true });
  }
});
