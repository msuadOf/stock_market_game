import { companyCivilDate as civilDate, canonicalU64 as decimal, nonnegativeMoney as money, boundedInteger, ownerAccount, nonempty } from "./company-contract-views.ts";
import { array, boolean, integer, nullable, oneOf, record, string, exact } from "../save/schema/primitives.ts";

const TAX_MODES = ["FlatWithholding", "AShareIndividual", "Exempt"] as const;
const IDENTITIES = ["Personal", "NonIndividualPending"] as const;
const STOCK_TAX_STATUSES = ["FlatWithholding", "IndividualPublicMarket", "TreatmentNotConfigured"] as const;

/** 宿主股息税状态查询的严格 parser：只接受 Engine 导出的当前契约形状。 */
export function parseAccountDividendTaxStatus(value: unknown, path = "dividend_tax_status") {
  const view = record(value, path);
  exact(view, ["mode", "identity", "stocks"], path);
  const stocks = array(view.stocks, `${path}.stocks`).map((item, index) => {
    const stock = record(item, `${path}.stocks[${index}]`);
    exact(stock, ["stock", "status"], `${path}.stocks[${index}]`);
    const code = nonempty(stock.stock, `${path}.stocks[${index}].stock`);
    if (code.trim() === "") throw new Error(`${path}.stocks[${index}].stock 证券代码不能为空`);
    return {
      stock: code,
      status: oneOf(stock.status, `${path}.stocks[${index}].status`, STOCK_TAX_STATUSES),
    };
  });
  return {
    mode: oneOf(view.mode, `${path}.mode`, TAX_MODES),
    identity: oneOf(view.identity, `${path}.identity`, IDENTITIES),
    stocks,
  };
}

/** 简税（FlatWithholding）代扣回执（owner 查询面）；与 engine
 * `FlatWithholdingReceipt` 的 serde 形态及存档 schema 同构。 */
export interface FlatWithholdingReceiptView {
  readonly payment_id: string;
  readonly plan_id: string;
  readonly account: string;
  readonly paid_on: string;
  readonly gross: string;
  readonly rate_bp: number;
  readonly withheld: string;
}

/** 简税代扣回执严格 parser（owner 过滤后仅含本人回执）。 */
export function parseOwnerFlatWithholdingReceipts(value: unknown, owner: string, path = "flat_withholding_receipts"): readonly FlatWithholdingReceiptView[] {
  ownerAccount(owner, `${path}.owner`);
  return array(value, path).map((item, index) => {
    const rowPath = `${path}[${index}]`;
    const receipt = record(item, rowPath);
    exact(receipt, ["payment_id", "plan_id", "account", "paid_on", "gross", "rate_bp", "withheld"], rowPath);
    const payment_id = string(receipt.payment_id, `${rowPath}.payment_id`);
    const plan_id = string(receipt.plan_id, `${rowPath}.plan_id`);
    const account = ownerAccount(receipt.account, `${rowPath}.account`);
    if (account !== owner) throw new Error(`${rowPath}.account 必须属于查询本人`);
    const gross = money(receipt.gross, `${rowPath}.gross`);
    const withheld = money(receipt.withheld, `${rowPath}.withheld`);
    if (!payment_id.trim() || !plan_id.trim()) throw new Error(`${rowPath} 回执身份不能为空`);
    if (!/^(0|[1-9]\d*)$/.test(account)) throw new Error(`${rowPath}.account 必须是规范账户十进制字符串`);
    if (BigInt(gross) < 0n || BigInt(withheld) < 0n) throw new Error(`${rowPath} 回执金额非法`);
    return {
      payment_id,
      plan_id,
      account,
      paid_on: civilDate(receipt.paid_on, `${rowPath}.paid_on`),
      gross,
      rate_bp: boundedInteger(receipt.rate_bp, `${rowPath}.rate_bp`, 0, 10000),
      withheld,
    };
  });
}

function parseExactAmount(value: unknown, path: string) {
  const row = record(value, path);
  exact(row, ["numerator", "denominator"], path);
  const numerator = string(row.numerator, `${path}.numerator`);
  if (!/^(0|[1-9]\d*)$/.test(numerator) || numerator.length > 39 || BigInt(numerator) >= (1n << 127n)) throw new Error(`${path}.numerator 必须是 i128 范围规范非负字符串`);
  const denominator = decimal(row.denominator, `${path}.denominator`, true);
  let left = BigInt(numerator), right = BigInt(denominator);
  while (right !== 0n) [left, right] = [right, left % right];
  if (left !== 1n) throw new Error(`${path} 必须是已约简的非负分数`);
  return { numerator, denominator };
}
function parseCollection(value: unknown, path: string) {
  const row = record(value, path);
  exact(row, ["operation_seq", "event_id", "day", "available_cash", "collected", "remaining_cash", "outstanding", "needs_funds"], path);
  return { operation_seq: integer(row.operation_seq, `${path}.operation_seq`, 0), event_id: nonempty(row.event_id, `${path}.event_id`), day: civilDate(row.day, `${path}.day`), available_cash: money(row.available_cash, `${path}.available_cash`), collected: money(row.collected, `${path}.collected`), remaining_cash: money(row.remaining_cash, `${path}.remaining_cash`), outstanding: parseExactAmount(row.outstanding, `${path}.outstanding`), needs_funds: boolean(row.needs_funds, `${path}.needs_funds`) };
}
export function parseDividendTaxOutstandingViews(value: unknown, owner: string, path = "dividend_tax_outstanding") {
  ownerAccount(owner, `${path}.owner`);
  return array(value, path).map((item, index) => {
    const rowPath = `${path}[${index}]`;
    const row = record(item, rowPath);
    exact(row, ["account", "stock", "outstanding", "latest_collection"], rowPath);
    const account = ownerAccount(row.account, `${rowPath}.account`);
    if (account !== owner) throw new Error(`${rowPath}.account 必须属于查询本人`);
    return { account, stock: nonempty(row.stock, `${rowPath}.stock`), outstanding: parseExactAmount(row.outstanding, `${rowPath}.outstanding`), latest_collection: nullable(row.latest_collection, `${rowPath}.latest_collection`, parseCollection) };
  });
}
export type FlatWithholdingReceipt = FlatWithholdingReceiptView;
export type AccountDividendTaxStatusView = ReturnType<typeof parseAccountDividendTaxStatus>;
export type DividendTaxOutstandingView = ReturnType<typeof parseDividendTaxOutstandingViews>[number];
