import assert from "node:assert/strict"
import test from "node:test"
import { DEFAULT_SETUP } from "../../config/defaults.ts"
import { parseSetup } from "./market.ts"

test("Monthly必须用户显式选择完整schedule，不能用裸Monthly静默补10日", { timeout: 10000 }, () => {
  assert.equal(parseSetup(DEFAULT_SETUP, "setup").report_frequency, "Quarterly")
  for (const invalid of ["Monthly", { Monthly: {} }, { Monthly: { schedule: null } }]) assert.throws(() => parseSetup({ ...DEFAULT_SETUP, report_frequency: invalid }, "setup"), /report_frequency/)
})

test("Monthly预设自定义与可选均匀延迟保持完整事实契约", { timeout: 10000 }, () => {
  for (const schedule of [
    { Preset: { preset: "FirstDayEvening", delay: "None" } },
    { Preset: { preset: "TenthDayEvening", delay: { Uniform: { max_days: 2 } } } },
    { Custom: { day: 28, second_of_day: 86399, delay: { Uniform: { max_days: 1 } } } },
  ]) {
    const frequency = { Monthly: { schedule } }
    assert.deepEqual(parseSetup({ ...DEFAULT_SETUP, report_frequency: frequency }, "setup").report_frequency, frequency)
  }
  for (const schedule of [
    { Custom: { day: 29, second_of_day: 0, delay: "None" } },
    { Custom: { day: 0, second_of_day: 0, delay: "None" } },
    { Custom: { day: 1, second_of_day: -1, delay: "None" } },
    { Custom: { day: "1", second_of_day: 0, delay: "None" } },
    { Custom: { day: 1, second_of_day: 86400, delay: "None" } },
    { Preset: { preset: "TenthDayEvening", delay: { Uniform: { max_days: 0 } } } },
    { Preset: { preset: "TenthDayEvening", delay: { Uniform: { max_days: 32 } } } },
    { Preset: { preset: "TenthDayEvening", delay: "None", fallback: true } },
  ]) assert.throws(() => parseSetup({ ...DEFAULT_SETUP, report_frequency: { Monthly: { schedule } } }, "setup"), /report_frequency/)
})
