import { SaveSchemaError, array, exact, integer, record, string } from "../primitives.ts"

export type GroupStructure = { readonly root: string; readonly holdings: readonly { readonly company: string; readonly parent_held_shares: number }[] }

function companyId(value: unknown, path: string): string {
  const parsed = string(value, path)
  if (parsed.trim().length === 0) throw new SaveSchemaError(path, "不能为空")
  return parsed
}

export function parseGroups(value: unknown, path = "groups"): GroupStructure[] {
  const members = new Set<string>()
  return array(value, path).map((value, index) => {
    const itemPath = `${path}[${index}]`
    const item = record(value, itemPath)
    exact(item, ["root", "holdings"], itemPath)
    const root = companyId(item.root, `${itemPath}.root`)
    if (members.has(root)) throw new SaveSchemaError(`${itemPath}.root`, "公司不得重复出现在 groups")
    members.add(root)
    const holdings = array(item.holdings, `${itemPath}.holdings`).map((value, holdingIndex) => {
      const holdingPath = `${itemPath}.holdings[${holdingIndex}]`
      const holding = record(value, holdingPath)
      exact(holding, ["company", "parent_held_shares"], holdingPath)
      const company = companyId(holding.company, `${holdingPath}.company`)
      if (members.has(company)) throw new SaveSchemaError(`${holdingPath}.company`, "公司不得重复出现在 groups")
      members.add(company)
      return { company, parent_held_shares: integer(holding.parent_held_shares, `${holdingPath}.parent_held_shares`, 1) }
    })
    return { root, holdings }
  })
}
