import assert from "node:assert/strict";
import { test } from "node:test";
import * as policy from "./startup-policy.ts";

const supported = { isSecureContext: true, crossOriginIsolated: true, sharedArrayBufferAvailable: true };

test("生产和开发构建必须等待选择，只有明确 e2e 构建自动进入 Worker", () => {
  assert.equal(typeof policy.initialStartupTarget, "function");
  for (const mode of ["production", "development", "", "e2e-production"]) {
    assert.equal(policy.initialStartupTarget(mode), null);
  }
  assert.deepEqual(policy.initialStartupTarget("e2e"), { kind: "wasm" });
});

test("本地浏览器验证全部多线程前置条件，桌面和远程不读取浏览器条件", () => {
  assert.equal(typeof policy.resolveStartupTarget, "function");
  assert.deepEqual(policy.resolveStartupTarget("local", "", false, () => supported), { kind: "wasm" });
  const forbiddenProbe = () => { throw new Error("不应检测浏览器 WASM"); };
  assert.deepEqual(policy.resolveStartupTarget("local", "", true, forbiddenProbe), { kind: "tauri" });
  for (const desktop of [true, false]) {
    assert.deepEqual(policy.resolveStartupTarget("remote", " https://server.example:8443/game/ ", desktop, forbiddenProbe), {
      kind: "remote", baseUrl: "https://server.example:8443/game",
    });
  }
  assert.throws(() => policy.resolveStartupTarget("wasm", "", false, forbiddenProbe), /未知启动模式/);
});

test("不安全上下文、未隔离或缺少共享内存明确拒绝，无单线程或主线程降级", () => {
  assert.equal(typeof policy.resolveStartupTarget, "function");
  for (const missing of Object.keys(supported)) {
    assert.throws(() => policy.resolveStartupTarget("local", "", false, () => ({ ...supported, [missing]: false })), (error: unknown) => {
      assert.ok(error instanceof Error);
      assert.match(error.message, new RegExp(missing === "sharedArrayBufferAvailable" ? "SharedArrayBuffer" : missing));
      assert.match(error.message, /HTTPS/);
      assert.match(error.message, /COOP\/COEP/);
      assert.match(error.message, /远程/);
      assert.match(error.message, /不会降级/);
      return true;
    });
  }
});

test("远程地址接受 HTTP(S)、IPv6、端口和路径前缀，拒绝含混或无效基地址", () => {
  assert.equal(typeof policy.validateRemoteServerAddress, "function");
  assert.equal(policy.validateRemoteServerAddress("http://[::1]:3000/"), "http://[::1]:3000");
  assert.equal(policy.validateRemoteServerAddress(" HTTPS://SERVER.EXAMPLE/game/// "), "https://server.example/game");
  for (const address of ["", "server.example", "/api", "//server.example", "ws://server.example", "ftp://server.example", "http://", "http:///server.example", "http://bad host", "http://server.example:99999", "https://user:secret@server.example", "https://server.example?x=1", "https://server.example#path", "https://server.example?", "https://server.example#", "https://server.example\\api"]) {
    assert.throws(() => policy.validateRemoteServerAddress(address), /Server 地址/);
  }
});
