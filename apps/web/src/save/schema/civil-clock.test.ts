import assert from "node:assert/strict"
import test from "node:test"
import { currentSaveFixture } from "../current-save-fixture.ts"
import { parseCivilClock } from "./civil-clock.ts"

function clock(): Record<string, unknown> {
  const value = currentSaveFixture().civil_clock
  assert.ok(value !== null && typeof value === "object" && !Array.isArray(value))
  return structuredClone(value as Record<string, unknown>)
}

function policy(value: Record<string, unknown>): Record<string, unknown> {
  const nested = value.policy
  assert.ok(nested !== null && typeof nested === "object" && !Array.isArray(nested))
  return nested as Record<string, unknown>
}

const OFFICIAL_COVERAGE = {
  exchange: "sse",
  year: 2026,
  closed_ranges: [{ from: "2026-01-01", to: "2026-01-03" }],
  source_citation_id: "sse-2026-holiday-notice",
  source_digest: "a".repeat(64),
}

test("当前存档携带可识别的 policy content digest", () => {
  const parsed = parseCivilClock(clock())
  assert.equal(parsed.policy_digest, "0123456789abcdef")
})

test("升级前无 digest 且无官方覆盖的旧档可解析为冻结模拟政策", () => {
  const legacy = clock()
  delete legacy.policy_digest
  const parsed = parseCivilClock(legacy)
  assert.equal("policy_digest" in parsed, false)
  assert.deepEqual(parsed.policy.official_coverage, [])
})

test("policy_digest=null 与 Rust Option::None 同义，空官方覆盖旧档可解析", () => {
  const legacy = clock()
  legacy.policy_digest = null
  const parsed = parseCivilClock(legacy)
  assert.equal("policy_digest" in parsed, false)
})

test("policy_digest=null 且带官方覆盖仍必须拒绝", () => {
  const missing = clock()
  missing.policy_digest = null
  policy(missing).official_coverage = [OFFICIAL_COVERAGE]
  assert.throws(() => parseCivilClock(missing), /policy_digest.*content digest/)
})

test("官方覆盖存档缺少 policy digest 必须拒绝", () => {
  const missing = clock()
  delete missing.policy_digest
  policy(missing).official_coverage = [OFFICIAL_COVERAGE]
  assert.throws(() => parseCivilClock(missing), /policy_digest.*content digest/)
})

test("policy digest 必须是 16 位小写十六进制", () => {
  for (const invalid of ["A".repeat(16), "0".repeat(15), "not-a-digest"]) {
    const tampered = clock()
    tampered.policy_digest = invalid
    assert.throws(() => parseCivilClock(tampered), /16 位小写十六进制/)
  }
})

test("带官方覆盖的当前存档接受合法 policy digest", () => {
  const current = clock()
  policy(current).official_coverage = [OFFICIAL_COVERAGE]
  assert.equal(parseCivilClock(current).policy_digest, "0123456789abcdef")
})
