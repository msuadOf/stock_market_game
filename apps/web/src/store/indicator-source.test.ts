import assert from "node:assert/strict";
import test from "node:test";
import { store, setIndicatorDataSource } from "./store.ts";

test("指标数据源默认前端且仅在当前客户端会话切换", () => {
  assert.equal(store.getState().settings.indicatorDataSource, "frontend");
  store.dispatch(setIndicatorDataSource("rust"));
  assert.equal(store.getState().settings.indicatorDataSource, "rust");
  store.dispatch(setIndicatorDataSource("frontend"));
  assert.equal(store.getState().settings.indicatorDataSource, "frontend");
});
