import assert from "node:assert/strict";
import test from "node:test";
import { DEFAULT_SETUP } from "../config/defaults.ts";
import { parseStartDate, setupWithStartDate } from "./start-date.ts";

test("新游戏日期默认值和闰年日期在运行范围内可用", () => {
  assert.deepEqual(parseStartDate("2030-01-01"), { kind: "valid", value: "2030-01-01" });
  assert.deepEqual(parseStartDate("2000-02-29"), { kind: "valid", value: "2000-02-29" });
});

test("新游戏日期拒绝越界和不存在的公历日", () => {
  assert.deepEqual(parseStartDate(""), {
    kind: "invalid",
    message: "模拟起始日期不是有效公历日",
  });
  assert.deepEqual(parseStartDate("1999-12-31"), {
    kind: "invalid",
    message: "模拟起始日期必须在 2000-01-01 至 2099-12-31 之间",
  });
  assert.deepEqual(parseStartDate("2030-02-30"), {
    kind: "invalid",
    message: "模拟起始日期不是有效公历日",
  });
});

test("新游戏 setup 只替换经过校验的自然日，不改变默认证券", () => {
  const setup = setupWithStartDate(DEFAULT_SETUP, "2031-01-01");
  assert.equal(setup.start_date, "2031-01-01");
  assert.deepEqual(setup.stocks.map((stock) => stock.code), ["600101", "002156", "300260", "600610", "000812"]);
  assert.notEqual(setup, DEFAULT_SETUP);
});
