import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import test from "node:test"
import { currentSaveFixture } from "./current-save-fixture.ts"
import { parseSaveSlot } from "./save-schema.ts"
import { parseInformationStates } from "./schema/personal/information.ts"
import { parseBeliefBooks } from "./schema/personal/beliefs.ts"

// 从真实 Rust producer 原档提取，字段和值均未投影；来源与原档哈希见工作记录。
const fragments = JSON.parse(readFileSync(new URL("./fixtures/current-producer-fragments.json", import.meta.url), "utf8")) as Record<string, unknown>

test("当前存档严格保留必填 desk_mirrored_resting，不混入默认值", { timeout: 10_000 }, () => {
  const current = currentSaveFixture()
  for (const desk_mirrored_resting of [false, true]) {
    const saved = { ...current, desk_mirrored_resting }
    assert.deepEqual(parseSaveSlot(saved), saved)
  }
  const { desk_mirrored_resting: _removed, ...missing } = current
  assert.throws(() => parseSaveSlot(missing), /desk_mirrored_resting.*必填/)
  for (const invalid of [0, "false", null, undefined]) {
    assert.throws(() => parseSaveSlot({ ...current, desk_mirrored_resting: invalid }), /desk_mirrored_resting/)
  }
})

test("真实 InformationState 和 BeliefBook 保留 AccountId 字符串与数值 CompanyId", { timeout: 10_000 }, () => {
  assert.deepEqual(parseInformationStates({ "1": fragments.information_state }), { "1": fragments.information_state })
  assert.deepEqual(parseBeliefBooks({ "3": fragments.belief_book }), { "3": fragments.belief_book })
})

test("个人存档拒绝非规范 AccountId、未知字段、错误 owner 和非数值 CompanyId", { timeout: 10_000 }, () => {
  const information = fragments.information_state as Record<string, unknown>
  const belief = fragments.belief_book as Record<string, unknown>
  for (const invalid of [1, "01", "+1", "-1", "18446744073709551616"]) {
    assert.throws(() => parseInformationStates({ "1": { ...information, owner: invalid } }), /owner/)
    assert.throws(() => parseBeliefBooks({ "3": { ...belief, npc: invalid } }), /npc/)
  }
  assert.throws(() => parseInformationStates({ "1": { ...information, owner: "2" } }), /owner/)
  assert.throws(() => parseBeliefBooks({ "3": { ...belief, npc: "2" } }), /npc/)
  assert.throws(() => parseInformationStates({ "1": { ...information, extra: true } }), /extra/)
  const entries = belief.entries as Record<string, Record<string, unknown>>
  for (const company of ["0", -1, 4_294_967_296, null]) {
    assert.throws(() => parseBeliefBooks({ "3": { ...belief, entries: { "0": { ...entries["0"], company } } } }), /company/)
  }
})
