import assert from "node:assert/strict"
import test from "node:test"
import { decimal } from "./primitives.ts"

test("u64 十进制字符串拒绝前导零并保留最大值", { timeout: 10000 }, () => {
  assert.equal(decimal("18446744073709551615", "value"), "18446744073709551615")
  for (const invalid of ["", "00", "01", "18446744073709551616"]) {
    assert.throws(() => decimal(invalid, "value"), /value/)
  }
})
