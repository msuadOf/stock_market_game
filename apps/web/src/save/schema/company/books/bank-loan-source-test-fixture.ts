import { owners, books } from "./tax-owner-test-fixture.ts"

export function bankLoanSourceFixture() {
  const loan = { principal: "80.00", accrued_receivable: "1.00", carried: "0", last_accrual_date: "2030-01-04", start_date: "2030-01-01", maturity_date: "2030-01-03", rate_bp: 400, counterparty: "EXT-BOR", stage: "Stage1", allowance: "0.00", written_off: false, recoverable: "0.00", transfers: [] }
  const principal = { source: 1, date: "2030-01-01", kind: "LoanIssued", cash_flow: "Operating", lines: [{ account: "1301", side: "Debit", amount: "80.00" }, { account: "1003", side: "Credit", amount: "80.00" }] }
  const interest = { source: 2, date: "2030-01-04", kind: "LoanInterestAccrued", cash_flow: "NonCash", lines: [{ account: "1131", side: "Debit", amount: "1.00" }, { account: "6011", side: "Credit", amount: "1.00" }] }
  return { ...owners.Bank, books: { ...books, journal: { batches: [[principal, interest]], closed: [] } }, loans: { "LN-1": loan }, counterparties: { counterparties: { "EXT-BOR": { id: "EXT-BOR", kind: "Customer", name: "虚拟借款客户" } }, flows: [] }, loan_claim_sources: [{ loan: "LN-1", source: 1, kind: "Principal" }, { loan: "LN-1", source: 2, kind: "Interest" }], next_event_id: 3 }
}
