import assert from "node:assert/strict";
import { test } from "node:test";
import { tradingFieldErrors } from "./trading-field-errors.ts";

test("即时字段错误分别关联价格和股数，符号限价与市价不校验禁用价格", { timeout: 10000 }, () => {
  assert.deepEqual(tradingFieldErrors("limit", "fixed", "1.234", "零股"), {
    price: "价格输入无效：金额输入必须是至多两位小数的规范元字符串", quantity: "数量必须是正整数股",
  });
  assert.deepEqual(tradingFieldErrors("limit", "highest", "", "50"), {});
  assert.deepEqual(tradingFieldErrors("market", "fixed", "", "100"), {});
  assert.deepEqual(tradingFieldErrors("limit", "fixed", "12.34", "100"), {});
});

test("即时数量上限消费当前证券/委托类型参数", { timeout: 10000 }, () => {
  assert.deepEqual(tradingFieldErrors("market", "fixed", "", "150100", 150000), { quantity: "该证券单笔数量不能超过 150000 股" });
  assert.deepEqual(tradingFieldErrors("limit", "highest", "", "150100", 300000), {});
});
