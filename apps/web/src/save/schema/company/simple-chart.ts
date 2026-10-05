import type { CompanyKind } from "./accounting/spec.ts";
import type { AccountElement, ChartOfAccounts } from "./accounting/chart.ts";
import { exact, SaveSchemaError } from "../primitives.ts";

type Definition = readonly [string, string, AccountElement, ("cash" | "contra")?];

const industrial: readonly Definition[] = [
  ["1001", "库存现金", "Asset", "cash"], ["1002", "银行存款", "Asset", "cash"], ["1122", "应收账款", "Asset"],
  ["1601", "固定资产", "Asset"], ["1602", "累计折旧", "Asset", "contra"], ["2001", "短期借款", "Liability"],
  ["2202", "应付账款", "Liability"], ["2221", "应交税费", "Liability"], ["2231", "应付利息", "Liability"],
  ["4001", "实收资本", "Equity"], ["4103", "本年利润", "Equity"], ["6001", "主营业务收入", "Revenue"],
  ["6401", "主营业务成本", "Expense"], ["6602", "管理费用", "Expense"], ["6603", "财务费用", "Expense"],
  ["6801", "所得税费用", "Expense"], ["1231", "坏账准备", "Asset", "contra"], ["1403", "原材料", "Asset"],
  ["1405", "库存商品", "Asset"], ["1603", "固定资产减值准备", "Asset", "contra"], ["1811", "递延所得税资产", "Asset"],
  ["222101", "应交税费—应交增值税（销项税额）", "Liability"], ["222102", "应交税费—应交增值税（进项税额）", "Liability", "contra"],
  ["222104", "应交税费—应交所得税", "Liability"], ["2501", "长期借款", "Liability"], ["2901", "递延所得税负债", "Liability"],
  ["5001", "生产成本（在产品）", "Asset"], ["6601", "销售费用", "Expense"], ["660201", "管理费用—研发费用", "Expense"], ["6701", "资产减值损失", "Expense"],
];
const bank: readonly Definition[] = [
  ["1003", "存放中央银行款项", "Asset", "cash"], ["1131", "应收利息（贷款）", "Asset"], ["1301", "贷款——本金", "Asset"],
  ["1303", "贷款减值准备", "Asset", "contra"], ["2011", "吸收存款——短期", "Liability"], ["2601", "吸收存款——长期", "Liability"],
  ["2231", "应付利息（存款）", "Liability"], ["4001", "实收资本", "Equity"], ["1811", "递延所得税资产", "Asset"],
  ["222104", "应交企业所得税", "Liability"], ["6801", "所得税费用", "Expense"], ["4103", "本年利润", "Equity"],
  ["6011", "利息收入", "Revenue"], ["6021", "手续费及佣金收入", "Revenue"], ["6411", "利息支出", "Expense"], ["6701", "信用减值损失", "Expense"],
];
const insurance: readonly Definition[] = [
  ["1002", "银行存款", "Asset", "cash"], ["1122", "应收保费", "Asset"], ["2501", "未到期责任负债", "Liability"],
  ["2502", "已发生赔款负债", "Liability"], ["1811", "递延所得税资产", "Asset"], ["222104", "应交企业所得税", "Liability"],
  ["6801", "所得税费用", "Expense"], ["4001", "实收资本", "Equity"], ["4103", "本年利润", "Equity"],
  ["6051", "保险服务收入", "Revenue"], ["6451", "保险服务费用", "Expense"], ["6541", "保险财务损益", "Expense"],
];
const realEstate: readonly Definition[] = [
  ["1002", "银行存款", "Asset", "cash"], ["1122", "应收账款（交付尾款）", "Asset"], ["1541", "开发存货", "Asset"],
  ["1542", "开发存货减值准备", "Asset", "contra"], ["2001", "短期借款", "Liability"], ["2203", "合同负债（预售款）", "Liability"],
  ["2231", "应付利息", "Liability"], ["2501", "长期借款（项目借款）", "Liability"], ["4001", "实收资本", "Equity"],
  ["4103", "本年利润", "Equity"], ["6001", "主营业务收入", "Revenue"], ["6401", "主营业务成本", "Expense"],
  ["6603", "财务费用", "Expense"], ["6701", "资产减值损失", "Expense"], ["1811", "递延所得税资产", "Asset"],
  ["222104", "应交所得税", "Liability"], ["6801", "所得税费用", "Expense"],
];
const summary: readonly Definition[] = [
  ["simple_receivable", "Simple 汇总应收", "Asset"], ["simple_payable", "Simple 汇总应付", "Liability"],
  ["simple_revenue", "Simple 汇总营业收入", "Revenue"], ["simple_fixed_expense", "Simple 汇总固定费用", "Expense"],
  ["simple_variable_expense", "Simple 汇总变动费用", "Expense"],
];

export function simpleAccountChart(kind: CompanyKind): ChartOfAccounts {
  const profiles = { Industrial: [2, industrial], Bank: [3, bank], Insurance: [4, insurance], RealEstate: [5, realEstate] } as const;
  const [version, definitions] = profiles[kind];
  return { version, accounts: Object.fromEntries([...definitions, ...summary].map(([code, name, element, flag]) => [code, { name, element, is_cash: flag === "cash", is_contra: flag === "contra" }])) };
}

export function validateSimpleChart(chart: ChartOfAccounts, kind: CompanyKind, path: string): void {
  const expected = simpleAccountChart(kind);
  if (chart.version !== expected.version) throw new SaveSchemaError(`${path}.version`, "科目表与公司类别不一致");
  exact(chart.accounts, Object.keys(expected.accounts), `${path}.accounts`);
  for (const [code, definition] of Object.entries(expected.accounts)) {
    const actual = chart.accounts[code]!;
    if (actual.name !== definition.name || actual.element !== definition.element || actual.is_cash !== definition.is_cash || actual.is_contra !== definition.is_contra) throw new SaveSchemaError(`${path}.accounts.${code}`, "科目定义与公司类别的汇总科目表不一致");
  }
}
