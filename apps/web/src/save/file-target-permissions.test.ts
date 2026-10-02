import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

type Permission = string | { identifier: string; allow?: readonly string[] };
const capability = JSON.parse(readFileSync(new URL("../../../desktop/src-tauri/capabilities/default.json", import.meta.url), "utf8")) as {
  windows: string[];
  permissions: Permission[];
  description: string;
};
const manifest = JSON.parse(readFileSync(new URL("../../../desktop/src-tauri/gen/schemas/acl-manifests.json", import.meta.url), "utf8")) as {
  fs: { permissions: Record<string, { commands: { allow: string[] } }> };
};
const temporaryScope = "**/stock-game-day-end-????????-????-4???-[89ab]???-????????????.tmp";

test("原子日终输出的必要命令全部获授权，命令专属 scope 仅扩展到项目 UUID 临时文件", () => {
  for (const command of ["open", "write-text-file", "rename", "remove"]) {
    const identifier = `fs:allow-${command}`;
    const permission = capability.permissions.find((entry) => typeof entry !== "string" && entry.identifier === identifier);
    assert.ok(permission !== undefined && typeof permission !== "string", `${identifier} 必须配置命令专属临时文件 scope`);
    assert.deepEqual(permission.allow, [temporaryScope]);
    assert.deepEqual(manifest.fs.permissions[`allow-${command}`]!.commands.allow, [command.replaceAll("-", "_")]);
  }
});

test("不增加 exists、全局 fs scope、全目录读写或非 main 窗口能力，风险明确登记", () => {
  const identifiers = capability.permissions.map((entry) => typeof entry === "string" ? entry : entry.identifier);
  assert.deepEqual(identifiers.filter((identifier) => identifier.startsWith("fs:")), [
    "fs:allow-read-text-file", "fs:allow-write-text-file", "fs:allow-open", "fs:allow-rename", "fs:allow-remove",
  ]);
  assert.ok(capability.permissions.includes("fs:allow-read-text-file"));
  assert.deepEqual(capability.windows, ["main"]);
  assert.match(capability.description, /风险/);
  assert.match(capability.description, /已.*授权.*文件/);
});
