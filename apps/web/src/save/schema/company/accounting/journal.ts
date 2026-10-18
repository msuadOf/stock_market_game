import { SaveSchemaError, array, civilDate, exact, integer, oneOf, record, string } from "../../primitives.ts"
import { parseAccountingPeriod, type AccountingPeriod } from "./period.ts"

const businessKinds = ["OpeningBalance", "LoanDisbursement", "LoanRepayment", "CashRevenue", "CreditSale", "ReceivableCollection", "CashExpense", "InterestAccrual", "InterestPayment", "TaxAccrual", "TaxPayment", "Depreciation", "CustomerDeposit", "CustomerWithdrawal", "LoanIssued", "LoanPrincipalCollected", "LoanInterestAccrued", "LoanInterestCollected", "DepositInterestAccrued", "DepositInterestPaid", "FeeAndCommissionEarned", "CreditImpairment", "LoanWriteOff", "WriteOffRecovery", "LandAcquisition", "DevelopmentCostIncurred", "PresaleCollection", "RealEstateDelivery", "FinalPaymentCollected", "BorrowingCostCapitalized", "DevelopmentImpairment", "InsurancePremiumAccrued", "InsurancePremiumCollected", "InsuranceServiceRevenue", "InsuranceFinance", "InsuranceLossComponent", "InsuranceClaimIncurred", "InsuranceClaimPaid"] as const
const cashFlows = ["Operating", "Investing", "Financing", "NonCash"] as const
const postingSides = ["Debit", "Credit"] as const
const AMOUNT = /^\s*[+-]?\d+(?:\.\d{1,2})?\s*$/

export type PostingSide = (typeof postingSides)[number]
export type JournalLine = { readonly account: string; readonly side: PostingSide; readonly amount: string }
export type JournalEntry = { readonly source: number; readonly date: string; readonly kind: (typeof businessKinds)[number]; readonly cash_flow: (typeof cashFlows)[number]; readonly lines: readonly JournalLine[] }
export type Journal = { readonly batches: readonly (readonly JournalEntry[])[]; readonly closed: readonly AccountingPeriod[] }

function parseAmount(value: unknown, path: string): string {
  const parsed = string(value, path)
  if (!AMOUNT.test(parsed)) throw new SaveSchemaError(path, "必须是两位小数的会计金额")
  return parsed
}

function parseJournalLine(value: unknown, path: string): JournalLine {
  const parsed = record(value, path)
  exact(parsed, ["account", "side", "amount"], path)
  return { account: string(parsed.account, `${path}.account`), side: oneOf(parsed.side, `${path}.side`, postingSides), amount: parseAmount(parsed.amount, `${path}.amount`) }
}

function parseJournalEntry(value: unknown, path: string): JournalEntry {
  const parsed = record(value, path)
  exact(parsed, ["source", "date", "kind", "cash_flow", "lines"], path)
  return { source: integer(parsed.source, `${path}.source`, 0), date: civilDate(parsed.date, `${path}.date`), kind: oneOf(parsed.kind, `${path}.kind`, businessKinds), cash_flow: oneOf(parsed.cash_flow, `${path}.cash_flow`, cashFlows), lines: array(parsed.lines, `${path}.lines`).map((line, index) => parseJournalLine(line, `${path}.lines[${index}]`)) }
}

export function parseJournal(value: unknown, path: string): Journal {
  const parsed = record(value, path)
  exact(parsed, ["batches", "closed"], path)
  return {
    batches: array(parsed.batches, `${path}.batches`).map((batch, batchIndex) => array(batch, `${path}.batches[${batchIndex}]`).map((entry, entryIndex) => parseJournalEntry(entry, `${path}.batches[${batchIndex}][${entryIndex}]`))),
    closed: array(parsed.closed, `${path}.closed`).map((period, index) => parseAccountingPeriod(period, `${path}.closed[${index}]`)),
  }
}
