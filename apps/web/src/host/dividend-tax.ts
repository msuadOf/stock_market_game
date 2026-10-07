import type { AccountDividendTaxStatusView } from "../types/generated/AccountDividendTaxStatusView";
import type { DividendTaxOutstandingView } from "../types/generated/DividendTaxOutstandingView";
import { parseDividendTaxOutstandingView } from "../save/schema/corporate-actions.ts";
import { array, oneOf, record, string, exact } from "../save/schema/primitives.ts";

const TAX_MODES = ["FlatWithholding", "AShareIndividual", "Exempt"] as const;
const IDENTITIES = ["Personal", "NonIndividualPending"] as const;
const STOCK_TAX_STATUSES = ["FlatWithholding", "IndividualPublicMarket", "TreatmentNotConfigured"] as const;

/** 宿主股息税状态查询的严格 parser：只接受 Engine 导出的当前契约形状。 */
export function parseAccountDividendTaxStatusView(value: unknown, path = "dividend_tax_status"): AccountDividendTaxStatusView {
  const view = record(value, path);
  exact(view, ["mode", "identity", "stocks"], path);
  const stocks = array(view.stocks, `${path}.stocks`).map((item, index) => {
    const stock = record(item, `${path}.stocks[${index}]`);
    exact(stock, ["stock", "status"], `${path}.stocks[${index}]`);
    const code = string(stock.stock, `${path}.stocks[${index}].stock`);
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

export function parseDividendTaxOutstandingViews(value: unknown, path = "dividend_tax_outstanding"): readonly DividendTaxOutstandingView[] {
  return array(value, path).map((item, index) => parseDividendTaxOutstandingView(item, `${path}[${index}]`));
}
