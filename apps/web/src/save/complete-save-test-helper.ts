import assert from "node:assert/strict"
import { createHash } from "node:crypto"
import { after } from "node:test"
import { representativeCurrentSaveFixture } from "./representative-save-test-fixture.ts"

const MATURE_SAVE = representativeCurrentSaveFixture()
const MATURE_SAVE_SHA256 = createHash("sha256").update(JSON.stringify(MATURE_SAVE)).digest("hex")

after(() => {
  const current = createHash("sha256").update(JSON.stringify(MATURE_SAVE)).digest("hex")
  assert.equal(current, MATURE_SAVE_SHA256, "save parser and path-copy mutations must not change the shared fixture")
})

export function matureSave(): unknown {
  return MATURE_SAVE
}

export function mutate(path: readonly (string | number)[], value: unknown): unknown {
  const source = matureSave()
  assert.ok(typeof source === "object" && source !== null)
  const save: unknown = Array.isArray(source) ? [...source] : { ...source }
  let sourceCursor: unknown = source
  let cursor: unknown = save
  for (const segment of path.slice(0, -1)) {
    assert.ok(typeof sourceCursor === "object" && sourceCursor !== null)
    const sourceChild = Reflect.get(sourceCursor, segment)
    assert.ok(typeof sourceChild === "object" && sourceChild !== null)
    const clonedChild: unknown = Array.isArray(sourceChild) ? [...sourceChild] : { ...sourceChild }
    if (typeof segment === "number") {
      assert.ok(Array.isArray(cursor))
      cursor[segment] = clonedChild
    } else {
      assert.ok(typeof cursor === "object" && cursor !== null && !Array.isArray(cursor))
      Reflect.set(cursor, segment, clonedChild)
    }
    sourceCursor = sourceChild
    cursor = clonedChild
  }
  const last = path.at(-1)
  if (last === undefined) throw new Error("mutation path must not be empty")
  assert.ok(typeof cursor === "object" && cursor !== null)
  Reflect.set(cursor, last, value)
  return save
}
