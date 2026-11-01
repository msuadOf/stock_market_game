import assert from "node:assert/strict";
import test from "node:test";
import { parseCivilInstant } from "./experience.ts";

test("CivilInstant 保持公历窗口及闰年边界", { timeout: 10000 }, () => {
  for (const date of ["1900-01-01", "2000-02-29", "2199-12-31"]) assert.equal(parseCivilInstant({ date, second_of_day: 0 }, "x").date, date);
  for (const date of ["1899-12-31", "2200-01-01", "1900-02-29", "2100-02-29", "2001-04-31"]) assert.throws(() => parseCivilInstant({ date, second_of_day: 0 }, "x"), /公历|日期/);
});

test("CivilInstant 不重复分配已验证日期的月天数字典", { timeout: 10000 }, () => {
  const original = globalThis.BigInt;
  globalThis.BigInt = (() => { throw new Error("重复大整数转换"); }) as unknown as typeof BigInt;
  try { assert.equal(parseCivilInstant({ date: "2000-02-29", second_of_day: 0 }, "x").date, "2000-02-29"); }
  finally { globalThis.BigInt = original; }
});
