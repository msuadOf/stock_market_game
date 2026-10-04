import assert from "node:assert/strict";
import test from "node:test";
import { RemoteRequestScope } from "./remote-request-scope.ts";

test("RemoteRequestScope 生命周期中断拒绝未完成 HTTP 请求，迟到响应不能二次完成", { timeout: 10000 }, async () => {
  const scope = new RemoteRequestScope();
  let complete: ((response: Response) => void) | null = null;
  const fetchFn = (() => new Promise<Response>((resolve) => { complete = resolve; })) as typeof fetch;
  const pending = scope.request(fetchFn, "https://remote.example/api/load", { method: "POST" });
  const rejected = assert.rejects(pending, /结果未知/);
  scope.interrupt("远程会话已销毁，结果未知");
  await rejected;
  complete!(Response.json({ accepted: true }));
  await new Promise((resolve) => setImmediate(resolve));
});

test("RemoteRequestScope 无响应请求在 5000ms 显式结果未知而非永久等待", { timeout: 10000 }, async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const scope = new RemoteRequestScope();
  const fetchFn = (() => new Promise<Response>(() => {})) as typeof fetch;
  const pending = scope.request(fetchFn, "https://remote.example/api/save", { method: "POST" });
  const rejected = assert.rejects(pending, /5000ms.*结果未知/);
  context.mock.timers.tick(5000);
  await rejected;
});
