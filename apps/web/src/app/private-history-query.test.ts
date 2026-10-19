import assert from "node:assert/strict";
import test from "node:test";
import { queryPrivateHistory } from "./private-history-query.ts";

test("本人交割历史缺席账户不能调用Host，真实fullu64本人可查询", { timeout: 10000 }, async () => {
  let calls = 0;
  const host = {};
  const query = async () => { calls++; return "private-result"; };
  await assert.rejects(queryPrivateHistory(() => ({ host, generation: "1", account: null }), query), /本人资金账户/);
  assert.equal(calls, 0);
  assert.equal(await queryPrivateHistory(() => ({ host, generation: "1", account: "18446744073709551615" }), query), "private-result");
  assert.equal(calls, 1);
});

test("本人交割历史成功和失败晚响应均按Host、generation、account丢弃", { timeout: 10000 }, async () => {
  for (const changed of ["host", "generation", "account"] as const) {
    for (const failed of [false, true]) {
      let state = { host: {}, generation: "1", account: "9007199254740993" as string | null };
      let complete!: () => void;
      const waiting = new Promise<void>(resolve => { complete = resolve; });
      const pending = queryPrivateHistory(() => state, async () => { await waiting; if (failed) throw new Error("旧本人私有错误"); return "旧本人私有结果"; });
      state = { ...state, [changed]: changed === "host" ? {} : changed === "generation" ? "2" : null };
      complete();
      await assert.rejects(pending, error => error instanceof Error && /已切换/.test(error.message) && !/旧本人私有/.test(error.message));
    }
  }
});

test("当前本人查询错误保留原始错误，不静默吞错", { timeout: 10000 }, async () => {
  const host = {}, failure = new Error("真实当前错误");
  await assert.rejects(queryPrivateHistory(() => ({ host, generation: "1", account: "1" }), async () => { throw failure; }), error => error === failure);
});
