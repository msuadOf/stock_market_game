import assert from "node:assert/strict"
import test from "node:test"
import { parseCompanySpec } from "./spec.ts"

const spec = (issued_shares: unknown) => ({ id: "C-600101", name: "虚构上市公司600101", industry: "listed-simple", kind: "Industrial", listed_stock: "600101", issued_shares, group_parent: null })

test("CompanySpec 股本使用规范十进制字符串并精确保留 u64 边界", { timeout: 10000 }, () => {
  const maximum = "18446744073709551615"
  assert.equal(parseCompanySpec(spec(maximum), "company").issued_shares, maximum)
  for (const invalid of [0, 1, "0", "01", "18446744073709551616", "1.0"]) {
    assert.throws(() => parseCompanySpec(spec(invalid), "company"), /issued_shares/)
  }
})
