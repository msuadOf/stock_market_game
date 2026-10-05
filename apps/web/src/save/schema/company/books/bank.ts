import { SaveSchemaError, array, boolean, civilDate, exact, integer, map, oneOf, record, string } from "../../primitives.ts"
import { accountingMinorUnits } from "../accounting/amount.ts"
import { parseBooks, type Books } from "../accounting/index.ts"
import { parseIncomeTaxPolicy, parseIncomeTaxPosition, type IncomeTaxPolicy, type IncomeTaxPosition } from "./income-tax.ts"
import { amount } from "../value.ts"
import { parseCounterpartyLedger, parseFraction, type CounterpartyLedger } from "./common.ts"

export type BankBooks = { readonly income_tax_policy: IncomeTaxPolicy; readonly income_tax_position: IncomeTaxPosition; readonly books: Books; readonly deposits: Readonly<Record<string, DepositState>>; readonly loans: Readonly<Record<string, BankLoanState>>; readonly counterparties: CounterpartyLedger; readonly ecl_policy: EclPolicy; readonly next_event_id: number; readonly loan_claim_sources: readonly BankLoanClaimSource[] }
type BankLoanClaimSource = { readonly loan: string; readonly source: number; readonly kind: "Principal" | "Interest" }
type DepositState = { readonly principal: string; readonly accrued_payable: string; readonly carried: string; readonly last_accrual_date: string; readonly rate_bp: number; readonly counterparty: string; readonly start_date: string; readonly maturity_date: string }
type BankLoanState = { readonly principal: string; readonly accrued_receivable: string; readonly carried: string; readonly last_accrual_date: string; readonly start_date: string; readonly maturity_date: string; readonly rate_bp: number; readonly counterparty: string; readonly stage: EclStage; readonly allowance: string; readonly written_off: boolean; readonly recoverable: string; readonly transfers: readonly StageTransferRecord[] }
type EclStage = "Stage1" | "Stage2" | "Stage3"
type StageTransferRecord = { readonly date: string; readonly from_stage: EclStage; readonly to_stage: EclStage; readonly reason: string }
type EclPolicy = { readonly version: number; readonly stage1_default: readonly EclScenario[]; readonly lifetime_default: readonly EclScenario[] }
type EclScenario = { readonly weight_bp: number; readonly pd_bp: number; readonly lgd_bp: number }
const stages = ["Stage1", "Stage2", "Stage3"] as const

export function parseBankBooks(value: unknown, path: string): BankBooks {
  const parsed = record(value, path)
  exact(parsed, ["income_tax_policy", "income_tax_position", "books", "deposits", "loans", "counterparties", "ecl_policy", "next_event_id", "loan_claim_sources"], path)
  const books = parseBooks(parsed.books, `${path}.books`)
  const policy = parseIncomeTaxPolicy(parsed.income_tax_policy, `${path}.income_tax_policy`)
  const bank = { books, income_tax_policy: policy, income_tax_position: parseIncomeTaxPosition(parsed.income_tax_position, `${path}.income_tax_position`, policy, books), deposits: map(parsed.deposits, `${path}.deposits`, stringKey, parseDeposit), loans: map(parsed.loans, `${path}.loans`, stringKey, parseLoan), counterparties: parseCounterpartyLedger(parsed.counterparties, `${path}.counterparties`), ecl_policy: parsePolicy(parsed.ecl_policy, `${path}.ecl_policy`), next_event_id: integer(parsed.next_event_id, `${path}.next_event_id`, 0), loan_claim_sources: array(parsed.loan_claim_sources, `${path}.loan_claim_sources`).map((fact, index) => parseClaimSource(fact, `${path}.loan_claim_sources[${index}]`)) }
  validateClaimSources(bank, path)
  return bank
}

function parseDeposit(value: unknown, path: string): DepositState {
  const parsed = record(value, path)
  exact(parsed, ["principal", "accrued_payable", "carried", "last_accrual_date", "rate_bp", "counterparty", "start_date", "maturity_date"], path)
  return { principal: amount(parsed.principal, `${path}.principal`), accrued_payable: amount(parsed.accrued_payable, `${path}.accrued_payable`), carried: parseFraction(parsed.carried, `${path}.carried`), last_accrual_date: civilDate(parsed.last_accrual_date, `${path}.last_accrual_date`), rate_bp: integer(parsed.rate_bp, `${path}.rate_bp`), counterparty: string(parsed.counterparty, `${path}.counterparty`), start_date: civilDate(parsed.start_date, `${path}.start_date`), maturity_date: civilDate(parsed.maturity_date, `${path}.maturity_date`) }
}

function parseLoan(value: unknown, path: string): BankLoanState {
  const parsed = record(value, path)
  exact(parsed, ["principal", "accrued_receivable", "carried", "last_accrual_date", "start_date", "maturity_date", "rate_bp", "counterparty", "stage", "allowance", "written_off", "recoverable", "transfers"], path)
  const loan = { principal: amount(parsed.principal, `${path}.principal`), accrued_receivable: amount(parsed.accrued_receivable, `${path}.accrued_receivable`), carried: parseFraction(parsed.carried, `${path}.carried`), last_accrual_date: civilDate(parsed.last_accrual_date, `${path}.last_accrual_date`), start_date: civilDate(parsed.start_date, `${path}.start_date`), maturity_date: civilDate(parsed.maturity_date, `${path}.maturity_date`), rate_bp: integer(parsed.rate_bp, `${path}.rate_bp`, 0), counterparty: string(parsed.counterparty, `${path}.counterparty`), stage: oneOf(parsed.stage, `${path}.stage`, stages), allowance: amount(parsed.allowance, `${path}.allowance`), written_off: boolean(parsed.written_off, `${path}.written_off`), recoverable: amount(parsed.recoverable, `${path}.recoverable`), transfers: array(parsed.transfers, `${path}.transfers`).map((entry, index) => parseTransfer(entry, `${path}.transfers[${index}]`)) }
  if (loan.start_date >= loan.maturity_date || loan.last_accrual_date < loan.start_date) throw new SaveSchemaError(path, "贷款原始期限或计息日期非法")
  return loan
}

function parseClaimSource(value: unknown, path: string): BankLoanClaimSource {
  const parsed = record(value, path)
  exact(parsed, ["loan", "source", "kind"], path)
  const loan = string(parsed.loan, `${path}.loan`)
  if (loan.trim().length === 0) throw new SaveSchemaError(`${path}.loan`, "贷款身份不能为空")
  return { loan, source: integer(parsed.source, `${path}.source`, 0), kind: oneOf(parsed.kind, `${path}.kind`, ["Principal", "Interest"] as const) }
}

function validateClaimSources(bank: BankBooks, path: string): void {
  const journal = bank.books.journal.batches.flat()
  const entries = new Map(journal.map(entry => [entry.source, entry]))
  if (entries.size !== journal.length) throw new SaveSchemaError(`${path}.books.journal`, "实际Journal来源重复")
  if (journal.some(entry => entry.source >= bank.next_event_id)) throw new SaveSchemaError(`${path}.next_event_id`, "事件游标不得落在已过账来源之内")
  const expected = new Set([...entries.values()].filter(entry => entry.kind === "LoanIssued" || entry.kind === "LoanInterestAccrued").map(entry => entry.source))
  const covered = new Set<number>()
  const principals = new Set<string>()
  for (const [index, fact] of bank.loan_claim_sources.entries()) {
    const factPath = `${path}.loan_claim_sources[${index}]`
    if (covered.has(fact.source) || !Object.hasOwn(bank.loans, fact.loan)) throw new SaveSchemaError(factPath, "来源重复或关联未知贷款")
    covered.add(fact.source)
    const loan = bank.loans[fact.loan]
    const entry = entries.get(fact.source)
    if (!entry) throw new SaveSchemaError(factPath, "来源没有实际过账凭证")
    const principal = fact.kind === "Principal"
    if (principal) {
      if (principals.has(fact.loan) || entry.date !== loan.start_date) throw new SaveSchemaError(factPath, "本金来源重复或日期不是实际发放日")
      principals.add(fact.loan)
    }
    const debit = entry.lines.find(line => line.account === (principal ? "1301" : "1131") && line.side === "Debit")
    const credit = entry.lines.find(line => line.account === (principal ? "1003" : "6011") && line.side === "Credit")
    if (entry.kind !== (principal ? "LoanIssued" : "LoanInterestAccrued") || entry.cash_flow !== (principal ? "Operating" : "NonCash") || entry.date < loan.start_date || entry.date > loan.last_accrual_date || entry.lines.length !== 2 || !debit || !credit || accountingMinorUnits(debit.amount, factPath) <= 0n || accountingMinorUnits(debit.amount, factPath) !== accountingMinorUnits(credit.amount, factPath)) throw new SaveSchemaError(factPath, "来源与真实业务种类／日期／正额等额科目不符")
  }
  if (covered.size !== expected.size || [...covered].some(source => !expected.has(source)) || principals.size !== Object.keys(bank.loans).length) throw new SaveSchemaError(`${path}.loan_claim_sources`, "真实贷款本金／利息凭证必须唯一完整覆盖")
  for (const [id, loan] of Object.entries(bank.loans)) {
    if (id.trim().length === 0 || !Object.hasOwn(bank.counterparties.counterparties, loan.counterparty)) throw new SaveSchemaError(`${path}.loans.${id}`, "贷款身份为空或借款人未注册")
  }
}

function parseTransfer(value: unknown, path: string): StageTransferRecord {
  const parsed = record(value, path)
  exact(parsed, ["date", "from_stage", "to_stage", "reason"], path)
  return { date: civilDate(parsed.date, `${path}.date`), from_stage: oneOf(parsed.from_stage, `${path}.from_stage`, stages), to_stage: oneOf(parsed.to_stage, `${path}.to_stage`, stages), reason: string(parsed.reason, `${path}.reason`) }
}

function parsePolicy(value: unknown, path: string): EclPolicy {
  const parsed = record(value, path)
  exact(parsed, ["version", "stage1_default", "lifetime_default"], path)
  return { version: integer(parsed.version, `${path}.version`, 0), stage1_default: array(parsed.stage1_default, `${path}.stage1_default`).map((entry, index) => parseScenario(entry, `${path}.stage1_default[${index}]`)), lifetime_default: array(parsed.lifetime_default, `${path}.lifetime_default`).map((entry, index) => parseScenario(entry, `${path}.lifetime_default[${index}]`)) }
}

function parseScenario(value: unknown, path: string): EclScenario {
  const parsed = record(value, path)
  exact(parsed, ["weight_bp", "pd_bp", "lgd_bp"], path)
  return { weight_bp: integer(parsed.weight_bp, `${path}.weight_bp`), pd_bp: integer(parsed.pd_bp, `${path}.pd_bp`), lgd_bp: integer(parsed.lgd_bp, `${path}.lgd_bp`) }
}
function stringKey(key: string): void { string(key, key) }
