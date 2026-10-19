import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";

test("Remote 登录 UI 显式登录、选择市场及确认重入，不隐式创建匿名局", { timeout: 10000 }, () => {
  const source = readFileSync(new URL("./RemoteLoginScreen.tsx", import.meta.url), "utf8");
  for (const text of ["client.login(", "client.register(", "client.guest()", "client.me(", "client.logout(", "client.markets(", "client.join(", "client.create(", "confirmedRejoin", "记住登录", "无法恢复", "无需邀请码", "整个共享市场", "onConnected("]) assert.ok(source.includes(text), `UI 缺少 ${text}`);
  assert.ok(!source.includes("localStorage"));
  assert.ok(!source.includes("VITE_REMOTE_TOKEN"));
});
