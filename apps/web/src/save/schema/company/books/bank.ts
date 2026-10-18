import { array, boolean, civilDate, exact, integer, map, oneOf, record, string } from "../../primitives.ts"
import { parseBooks, type Books } from "../accounting/index.ts"
import { amount } from "../value.ts"
import { parseCounterpartyLedger, parseFraction, type CounterpartyLedger } from "./common.ts"

export type BankBooks = { readonly books: Books; readonly deposits: Readonly<Record<string, DepositState>>; readonly loans: Readonly<Record<string, BankLoanState>>; readonly counterparties: CounterpartyLedger; readonly ecl_policy: EclPolicy; readonly next_event_id: number }
type DepositState = { readonly principal: string; readonly accrued_payable: string; readonly carried: string; readonly last_accrual_date: string; readonly rate_bp: number; readonly counterparty: string; readonly start_date: string; readonly maturity_date: string }
type BankLoanState = { readonly principal: string; readonly accrued_receivable: string; readonly carried: string; readonly last_accrual_date: string; readonly rate_bp: number; readonly counterparty: string; readonly stage: EclStage; readonly allowance: string; readonly written_off: boolean; readonly recoverable: string; readonly transfers: readonly StageTransferRecord[] }
type EclStage = "Stage1" | "Stage2" | "Stage3"
type StageTransferRecord = { readonly date: string; readonly from_stage: EclStage; readonly to_stage: EclStage; readonly reason: string }
type EclPolicy = { readonly version: number; readonly stage1_default: readonly EclScenario[]; readonly lifetime_default: readonly EclScenario[] }
type EclScenario = { readonly weight_bp: number; readonly pd_bp: number; readonly lgd_bp: number }
const stages = ["Stage1", "Stage2", "Stage3"] as const

export function parseBankBooks(value: unknown, path: string): BankBooks {
  const parsed = record(value, path)
  exact(parsed, ["books", "deposits", "loans", "counterparties", "ecl_policy", "next_event_id"], path)
  return { books: parseBooks(parsed.books, `${path}.books`), deposits: map(parsed.deposits, `${path}.deposits`, stringKey, parseDeposit), loans: map(parsed.loans, `${path}.loans`, stringKey, parseLoan), counterparties: parseCounterpartyLedger(parsed.counterparties, `${path}.counterparties`), ecl_policy: parsePolicy(parsed.ecl_policy, `${path}.ecl_policy`), next_event_id: integer(parsed.next_event_id, `${path}.next_event_id`, 0) }
}

function parseDeposit(value: unknown, path: string): DepositState {
  const parsed = record(value, path)
  exact(parsed, ["principal", "accrued_payable", "carried", "last_accrual_date", "rate_bp", "counterparty", "start_date", "maturity_date"], path)
  return { principal: amount(parsed.principal, `${path}.principal`), accrued_payable: amount(parsed.accrued_payable, `${path}.accrued_payable`), carried: parseFraction(parsed.carried, `${path}.carried`), last_accrual_date: civilDate(parsed.last_accrual_date, `${path}.last_accrual_date`), rate_bp: integer(parsed.rate_bp, `${path}.rate_bp`), counterparty: string(parsed.counterparty, `${path}.counterparty`), start_date: civilDate(parsed.start_date, `${path}.start_date`), maturity_date: civilDate(parsed.maturity_date, `${path}.maturity_date`) }
}

function parseLoan(value: unknown, path: string): BankLoanState {
  const parsed = record(value, path)
  exact(parsed, ["principal", "accrued_receivable", "carried", "last_accrual_date", "rate_bp", "counterparty", "stage", "allowance", "written_off", "recoverable", "transfers"], path)
  return { principal: amount(parsed.principal, `${path}.principal`), accrued_receivable: amount(parsed.accrued_receivable, `${path}.accrued_receivable`), carried: parseFraction(parsed.carried, `${path}.carried`), last_accrual_date: civilDate(parsed.last_accrual_date, `${path}.last_accrual_date`), rate_bp: integer(parsed.rate_bp, `${path}.rate_bp`), counterparty: string(parsed.counterparty, `${path}.counterparty`), stage: oneOf(parsed.stage, `${path}.stage`, stages), allowance: amount(parsed.allowance, `${path}.allowance`), written_off: boolean(parsed.written_off, `${path}.written_off`), recoverable: amount(parsed.recoverable, `${path}.recoverable`), transfers: array(parsed.transfers, `${path}.transfers`).map((entry, index) => parseTransfer(entry, `${path}.transfers[${index}]`)) }
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
