import assert from "node:assert/strict"
import { createHash } from "node:crypto"
import { after } from "node:test"
import { representativeCurrentSaveFixture } from "./representative-save-test-fixture.ts"

const MATURE_SAVE = representativeCurrentSaveFixture()
const MATURE_SAVE_SHA256 = createHash("sha256").update(JSON.stringify(MATURE_SAVE)).digest("hex")

type JsonRecord = Record<string, unknown>

function record(value: unknown): JsonRecord {
  assert.ok(typeof value === "object" && value !== null && !Array.isArray(value))
  return value as JsonRecord
}

function cloneAlongPath(source: unknown, path: readonly (string | number)[], value: unknown): unknown {
  const save: unknown = Array.isArray(source) ? [...source] : { ...record(source) }
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

/**
 * `complete-save-schema-extra` 只验证根级与代表性个人/公司分支；公司账套事件、
 * 闭合账期和价格记忆的其余行会重复触发同一套严格校验。这里保留每类分支的
 * 非空代表样本，删除不影响这些断言的批量历史，避免 35 次全量解析反复遍历
 * 9.8MB fixture。
 */
function compactBoundaryFixture(): JsonRecord {
  const save = structuredClone(MATURE_SAVE) as JsonRecord
  const legacy = record(record(record(save.company_state).LegacyOperations))
  const operations = record(legacy.operations)
  const companies = record(operations.companies)
  const companyId = Object.keys(companies)[0]
  if (companyId === undefined) throw new Error("representative save must contain a company")
  const company = record(companies[companyId])
  operations.companies = { [companyId]: company }
  const catalog = record(operations.catalog)
  const metas = catalog.metas
  if (!Array.isArray(metas)) throw new Error("representative save must contain company catalog metas")
  catalog.metas = metas.filter((meta) => record(meta).code === record(company.spec).code)
  record(operations.scheduler).pending = []
  operations.payment_failures = {}
  for (const flowValue of Object.values(record(company.books))) {
    const flow = record(flowValue)
    flow.trade_counterparty_events = []
    flow.inventory_source_events = []
    record(flow.books).journal = { batches: [], closed: [] }
    record(flow.receivables).items = {}
    record(flow.counterparties).flows = []
  }
  record(legacy.closing).versions = []
  record(legacy.closing).restatements = []

  const current = record(save.runtime_state)
  current.retail_projection_seen = []
  save.public_library = { next_seq: 0, reports: [], announcements: [] }

  const plans = record(save.plans)
  const planRows = plans.plans
  if (!Array.isArray(planRows) || planRows.length === 0) throw new Error("representative save must contain a plan")
  plans.plans = [planRows[0]]
  const owner = String(record(record(planRows[0]).Investor).account)
  const ownerSequences = plans.owner_sequences
  if (!Array.isArray(ownerSequences)) throw new Error("representative save must contain plan owners")
  plans.owner_sequences = ownerSequences.filter((sequence) => record(sequence).owner === owner)

  const keepFirst = (value: unknown): JsonRecord => {
    const entries = record(value)
    const key = Object.keys(entries)[0]
    if (key === undefined) throw new Error("representative save must contain a non-empty branch")
    return { [key]: entries[key] }
  }
  save.information_states = keepFirst(save.information_states)
  save.watchlists = keepFirst(save.watchlists)
  const memories = record(save.price_memories)
  const memoryKey = Object.entries(memories).find(([, memory]) => Object.keys(record(record(memory).stocks)).length > 0)?.[0]
  if (memoryKey === undefined) throw new Error("representative save must contain a non-empty price memory")
  save.price_memories = { [memoryKey]: memories[memoryKey] }
  const beliefs = record(save.belief_books)
  const institution = Object.entries(beliefs).find(([, book]) => {
    const profile = record(book).profile
    return typeof profile === "object" && profile !== null && Object.hasOwn(profile, "Institution")
  })?.[0]
  if (institution === undefined) throw new Error("representative save must contain an institution belief")
  const firstBelief = Object.keys(beliefs)[0]
  save.belief_books = Object.fromEntries([firstBelief, institution].flatMap((key) => key === undefined ? [] : [[key, beliefs[key]]]))
  return save
}

const BOUNDARY_SAVE = compactBoundaryFixture()
const BOUNDARY_SAVE_SHA256 = createHash("sha256").update(JSON.stringify(BOUNDARY_SAVE)).digest("hex")

after(() => {
  const current = createHash("sha256").update(JSON.stringify(MATURE_SAVE)).digest("hex")
  const boundary = createHash("sha256").update(JSON.stringify(BOUNDARY_SAVE)).digest("hex")
  assert.equal(current, MATURE_SAVE_SHA256, "save parser and path-copy mutations must not change the shared fixture")
  assert.equal(boundary, BOUNDARY_SAVE_SHA256, "path-copy mutations must not change the compact boundary fixture")
})

export function matureSave(): unknown {
  return MATURE_SAVE
}

export function boundarySave(): unknown {
  return BOUNDARY_SAVE
}

export function mutate(path: readonly (string | number)[], value: unknown): unknown {
  return cloneAlongPath(matureSave(), path, value)
}

export function mutateBoundary(path: readonly (string | number)[], value: unknown): unknown {
  return cloneAlongPath(boundarySave(), path, value)
}
