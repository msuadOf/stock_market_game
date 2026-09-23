import { SaveSchemaError, exact, record } from "../../primitives.ts"
import { parseBankBooks, type BankBooks } from "./bank.ts"
import { parseIndustrialBooks, type IndustrialBooks } from "./industrial.ts"
import { parseInsuranceBooks, type InsuranceBooks } from "./insurance.ts"
import { parseRealEstateBooks, type RealEstateBooks } from "./real-estate.ts"

export type IndustryBooks = { readonly Industrial: IndustrialBooks } | { readonly Bank: BankBooks } | { readonly Insurance: InsuranceBooks } | { readonly RealEstate: RealEstateBooks }

export function parseIndustryBooks(value: unknown, path = "industry_books"): IndustryBooks {
  const parsed = record(value, path)
  const entries = Object.entries(parsed)
  if (entries.length !== 1 || entries[0] === undefined) throw new SaveSchemaError(path, "必须是单一行业账套变体")
  const [tag, body] = entries[0]
  switch (tag) {
    case "Industrial":
      exact(parsed, ["Industrial"], path)
      return { Industrial: parseIndustrialBooks(body, `${path}.Industrial`) }
    case "Bank":
      exact(parsed, ["Bank"], path)
      return { Bank: parseBankBooks(body, `${path}.Bank`) }
    case "Insurance":
      exact(parsed, ["Insurance"], path)
      return { Insurance: parseInsuranceBooks(body, `${path}.Insurance`) }
    case "RealEstate":
      exact(parsed, ["RealEstate"], path)
      return { RealEstate: parseRealEstateBooks(body, `${path}.RealEstate`) }
    default:
      throw new SaveSchemaError(path, "包含无效行业账套变体")
  }
}

export type { BankBooks, IndustrialBooks, InsuranceBooks, RealEstateBooks }
