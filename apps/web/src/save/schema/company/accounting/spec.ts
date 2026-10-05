import { SaveSchemaError, decimal, exact, oneOf, record, string } from "../../primitives.ts"

const companyKinds = ["Industrial", "Bank", "Insurance", "RealEstate"] as const

export type CompanyKind = (typeof companyKinds)[number]
export type CompanySpec = {
  readonly id: string
  readonly name: string
  readonly industry: string
  readonly kind: CompanyKind
  readonly listed_stock: string | null
  readonly issued_shares: string
  readonly group_parent: string | null
}

function nonEmpty(value: unknown, path: string): string {
  const parsed = string(value, path)
  if (parsed.trim().length === 0) throw new SaveSchemaError(path, "不能为空")
  return parsed
}

export function parseCompanySpec(value: unknown, path: string): CompanySpec {
  const parsed = record(value, path)
  exact(parsed, ["id", "name", "industry", "kind", "listed_stock", "issued_shares", "group_parent"], path)
  const issuedShares = decimal(parsed.issued_shares, `${path}.issued_shares`)
  if (BigInt(issuedShares) === 0n) throw new SaveSchemaError(`${path}.issued_shares`, "必须大于零")
  return {
    id: nonEmpty(parsed.id, `${path}.id`),
    name: nonEmpty(parsed.name, `${path}.name`),
    industry: nonEmpty(parsed.industry, `${path}.industry`),
    kind: oneOf(parsed.kind, `${path}.kind`, companyKinds),
    listed_stock: parsed.listed_stock === null ? null : string(parsed.listed_stock, `${path}.listed_stock`),
    issued_shares: issuedShares,
    group_parent: parsed.group_parent === null ? null : string(parsed.group_parent, `${path}.group_parent`),
  }
}
