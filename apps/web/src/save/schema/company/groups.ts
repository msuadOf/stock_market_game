import { SaveSchemaError, array, exact, integer, record } from "../primitives.ts"
import { u32 } from "../personal/common.ts"

export type GroupStructure = { readonly root: number; readonly holdings: readonly { readonly company: number; readonly parent_held_shares: number }[] }

export function parseGroups(value: unknown, path = "groups"): GroupStructure[] {
  const members = new Set<number>()
  return array(value, path).map((value, index) => {
    const itemPath = `${path}[${index}]`
    const item = record(value, itemPath)
    exact(item, ["root", "holdings"], itemPath)
    const root = u32(item.root, `${itemPath}.root`)
    if (members.has(root)) throw new SaveSchemaError(`${itemPath}.root`, "公司不得重复出现在 groups")
    members.add(root)
    const holdings = array(item.holdings, `${itemPath}.holdings`).map((value, holdingIndex) => {
      const holdingPath = `${itemPath}.holdings[${holdingIndex}]`
      const holding = record(value, holdingPath)
      exact(holding, ["company", "parent_held_shares"], holdingPath)
      const company = u32(holding.company, `${holdingPath}.company`)
      if (members.has(company)) throw new SaveSchemaError(`${holdingPath}.company`, "公司不得重复出现在 groups")
      members.add(company)
      return { company, parent_held_shares: integer(holding.parent_held_shares, `${holdingPath}.parent_held_shares`, 1) }
    })
    return { root, holdings }
  })
}
