import { u32 } from "../personal/common.ts"
import { SaveSchemaError, array, exact, integer, oneOf, record } from "../primitives.ts"
import { period } from "./value.ts"
import { parseReportSet, type ReportSet, type Scope } from "./reports.ts"

const reportKinds = ["Monthly", "Quarter", "HalfYear", "Annual"] as const

export type ClosingVersion = readonly [Scope, string, (typeof reportKinds)[number], readonly ReportSet[]]
export type Restatement = readonly [Scope, readonly (readonly [number, string])[]]
export type ClosingRegistry = { readonly versions: readonly ClosingVersion[]; readonly restatements: readonly Restatement[] }

function parseScope(value: unknown, path: string): Scope {
  const parsed = record(value, path)
  const entries = Object.entries(parsed)
  if (entries.length !== 1) throw new SaveSchemaError(path, "必须是单一范围变体")
  const entry = entries[0]
  if (entry === undefined) throw new SaveSchemaError(path, "必须是单一范围变体")
  if (entry[0] === "Standalone") return { Standalone: u32(entry[1], `${path}.Standalone`) }
  if (entry[0] === "Consolidated") return { Consolidated: u32(entry[1], `${path}.Consolidated`) }
  throw new SaveSchemaError(path, "包含无效范围变体")
}

export function parseClosingRegistry(value: unknown, path = "closing_registry"): ClosingRegistry {
  const parsed = record(value, path)
  exact(parsed, ["versions", "restatements"], path)
  const versions = array(parsed.versions, `${path}.versions`).map((entry, index) => {
    const tuple = array(entry, `${path}.versions[${index}]`)
    if (tuple.length !== 4) throw new SaveSchemaError(`${path}.versions[${index}]`, "必须是四元组")
    return [parseScope(tuple[0], `${path}.versions[${index}][0]`), period(tuple[1], `${path}.versions[${index}][1]`), oneOf(tuple[2], `${path}.versions[${index}][2]`, reportKinds), array(tuple[3], `${path}.versions[${index}][3]`).map((set, setIndex) => parseReportSet(set, `${path}.versions[${index}][3][${setIndex}]`))] as const
  })
  const restatements = array(parsed.restatements, `${path}.restatements`).map((entry, index) => {
    const tuple = array(entry, `${path}.restatements[${index}]`)
    if (tuple.length !== 2) throw new SaveSchemaError(`${path}.restatements[${index}]`, "必须是二元组")
    return [parseScope(tuple[0], `${path}.restatements[${index}][0]`), array(tuple[1], `${path}.restatements[${index}][1]`).map((mapping, mappingIndex) => { const pair = array(mapping, `${path}.restatements[${index}][1][${mappingIndex}]`); if (pair.length !== 2) throw new SaveSchemaError(`${path}.restatements[${index}][1][${mappingIndex}]`, "必须是二元组"); return [integer(pair[0], `${path}.restatements[${index}][1][${mappingIndex}][0]`, 0), period(pair[1], `${path}.restatements[${index}][1][${mappingIndex}][1]`)] as const })] as const
  })
  return { versions, restatements }
}
