import { civilDate, exact, integer, map, record, string } from "../../primitives.ts"
import { parseBooks, type Books } from "../accounting/index.ts"
import { amount } from "../value.ts"
import { parseCounterpartyLedger, parseFraction, type CounterpartyLedger } from "./common.ts"

export type InsuranceBooks = { readonly books: Books; readonly groups: Readonly<Record<string, ContractGroupState>>; readonly counterparties: CounterpartyLedger; readonly discount: DiscountAssumption; readonly next_event_id: number }
type ContractGroupState = { readonly policyholder: string; readonly premium: string; readonly premium_collected: string; readonly expected_claims_remaining: string; readonly risk_adjustment_remaining: string; readonly csm: string; readonly loss_component: string; readonly finance_remaining: string; readonly units_total: number; readonly units_released: number; readonly coverage_start: string; readonly coverage_end: string; readonly day_one_loss: string; readonly released_revenue: string; readonly released_finance: string; readonly remeasure_finance: string; readonly remeasure_loss: string; readonly reestimated_csm: string; readonly carried_claims: string; readonly carried_risk_adjustment: string; readonly carried_csm: string; readonly carried_finance: string; readonly carried_loss: string; readonly claims: Readonly<Record<string, ClaimState>> }
type ClaimState = { readonly incurred: string; readonly paid: string; readonly date_incurred: string }
type DiscountAssumption = { readonly version: number; readonly rate_bp: number }

export function parseInsuranceBooks(value: unknown, path: string): InsuranceBooks {
  const parsed = record(value, path)
  exact(parsed, ["books", "groups", "counterparties", "discount", "next_event_id"], path)
  return { books: parseBooks(parsed.books, `${path}.books`), groups: map(parsed.groups, `${path}.groups`, stringKey, parseGroup), counterparties: parseCounterpartyLedger(parsed.counterparties, `${path}.counterparties`), discount: parseDiscount(parsed.discount, `${path}.discount`), next_event_id: integer(parsed.next_event_id, `${path}.next_event_id`, 0) }
}

function parseGroup(value: unknown, path: string): ContractGroupState {
  const parsed = record(value, path)
  exact(parsed, ["policyholder", "premium", "premium_collected", "expected_claims_remaining", "risk_adjustment_remaining", "csm", "loss_component", "finance_remaining", "units_total", "units_released", "coverage_start", "coverage_end", "day_one_loss", "released_revenue", "released_finance", "remeasure_finance", "remeasure_loss", "reestimated_csm", "carried_claims", "carried_risk_adjustment", "carried_csm", "carried_finance", "carried_loss", "claims"], path)
  return { policyholder: string(parsed.policyholder, `${path}.policyholder`), premium: amount(parsed.premium, `${path}.premium`), premium_collected: amount(parsed.premium_collected, `${path}.premium_collected`), expected_claims_remaining: amount(parsed.expected_claims_remaining, `${path}.expected_claims_remaining`), risk_adjustment_remaining: amount(parsed.risk_adjustment_remaining, `${path}.risk_adjustment_remaining`), csm: amount(parsed.csm, `${path}.csm`), loss_component: amount(parsed.loss_component, `${path}.loss_component`), finance_remaining: amount(parsed.finance_remaining, `${path}.finance_remaining`), units_total: integer(parsed.units_total, `${path}.units_total`), units_released: integer(parsed.units_released, `${path}.units_released`, 0), coverage_start: civilDate(parsed.coverage_start, `${path}.coverage_start`), coverage_end: civilDate(parsed.coverage_end, `${path}.coverage_end`), day_one_loss: amount(parsed.day_one_loss, `${path}.day_one_loss`), released_revenue: amount(parsed.released_revenue, `${path}.released_revenue`), released_finance: amount(parsed.released_finance, `${path}.released_finance`), remeasure_finance: amount(parsed.remeasure_finance, `${path}.remeasure_finance`), remeasure_loss: amount(parsed.remeasure_loss, `${path}.remeasure_loss`), reestimated_csm: amount(parsed.reestimated_csm, `${path}.reestimated_csm`), carried_claims: parseFraction(parsed.carried_claims, `${path}.carried_claims`), carried_risk_adjustment: parseFraction(parsed.carried_risk_adjustment, `${path}.carried_risk_adjustment`), carried_csm: parseFraction(parsed.carried_csm, `${path}.carried_csm`), carried_finance: parseFraction(parsed.carried_finance, `${path}.carried_finance`), carried_loss: parseFraction(parsed.carried_loss, `${path}.carried_loss`), claims: map(parsed.claims, `${path}.claims`, stringKey, parseClaim) }
}

function parseClaim(value: unknown, path: string): ClaimState {
  const parsed = record(value, path)
  exact(parsed, ["incurred", "paid", "date_incurred"], path)
  return { incurred: amount(parsed.incurred, `${path}.incurred`), paid: amount(parsed.paid, `${path}.paid`), date_incurred: civilDate(parsed.date_incurred, `${path}.date_incurred`) }
}

function parseDiscount(value: unknown, path: string): DiscountAssumption {
  const parsed = record(value, path)
  exact(parsed, ["version", "rate_bp"], path)
  return { version: integer(parsed.version, `${path}.version`, 0), rate_bp: integer(parsed.rate_bp, `${path}.rate_bp`) }
}
function stringKey(key: string): void { string(key, key) }
