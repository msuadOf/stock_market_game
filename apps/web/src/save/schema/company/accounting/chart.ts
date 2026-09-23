import { SaveSchemaError, boolean, exact, integer, map, oneOf, record, string } from "../../primitives.ts"

const accountElements = ["Asset", "Liability", "Equity", "Revenue", "Expense"] as const

export type AccountElement = (typeof accountElements)[number]
export type AccountDefinition = {
  readonly name: string
  readonly element: AccountElement
  readonly is_cash: boolean
  readonly is_contra: boolean
}
export type ChartOfAccounts = {
  readonly version: number
  readonly accounts: Readonly<Record<string, AccountDefinition>>
}

function parseAccountDefinition(value: unknown, path: string): AccountDefinition {
  const parsed = record(value, path)
  exact(parsed, ["name", "element", "is_cash", "is_contra"], path)
  return {
    name: string(parsed.name, `${path}.name`),
    element: oneOf(parsed.element, `${path}.element`, accountElements),
    is_cash: boolean(parsed.is_cash, `${path}.is_cash`),
    is_contra: boolean(parsed.is_contra, `${path}.is_contra`),
  }
}

export function parseChartOfAccounts(value: unknown, path: string): ChartOfAccounts {
  const parsed = record(value, path)
  exact(parsed, ["version", "accounts"], path)
  const version = integer(parsed.version, `${path}.version`, 0)
  if (version > 4_294_967_295) throw new SaveSchemaError(`${path}.version`, "超出 u32 范围")
  return {
    version,
    accounts: map(parsed.accounts, `${path}.accounts`, string, parseAccountDefinition),
  }
}
