import assert from "node:assert/strict";
import { test } from "node:test";
import { MobileDetailFocus } from "./mobile-detail-focus.ts";

test("详情进入聚焦返回，切股不抢焦点，返回恢复原行且不改变滚动", { timeout: 10000 }, () => {
  const calls: string[] = [];
  const target = (name: string, connected = true) => ({ isConnected: connected, focus(options?: FocusOptions) { assert.equal(options?.preventScroll, true); calls.push(name); } });
  const owner = new MobileDetailFocus();
  const row = target("股票行");
  const back = target("返回");
  const list = target("列表");
  owner.apply(false, () => list);
  owner.remember(row);
  owner.apply(true, () => back);
  owner.apply(true, () => back);
  owner.apply(false, () => list);
  assert.deepEqual(calls, ["返回", "股票行"]);
  owner.remember(target("已移除", false));
  owner.apply(true, () => back);
  owner.apply(false, () => list);
  assert.deepEqual(calls, ["返回", "股票行", "返回", "列表"]);
});
