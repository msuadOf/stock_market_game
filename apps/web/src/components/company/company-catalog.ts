export type PublicCompany = {
  readonly id: string;
  readonly name: string;
  readonly industry: string;
  readonly business: string;
  readonly stockCode: string | null;
};

export const publicCompanies: readonly PublicCompany[] = [
  { id: "C-600101", name: "稳健实业集团有限公司", industry: "装备制造", business: "工业制造与设备", stockCode: "600101" },
  { id: "C-002156", name: "芯片科技股份有限公司", industry: "半导体", business: "芯片设计与制造", stockCode: "002156" },
  { id: "C-300260", name: "短线题材文化传媒股份有限公司", industry: "文化传媒", business: "内容与传媒服务", stockCode: "300260" },
  { id: "C-600610", name: "人气妖股商贸股份有限公司", industry: "商贸零售", business: "商贸与零售", stockCode: "600610" },
  { id: "C-000812", name: "低价钢铁股份有限公司", industry: "钢铁", business: "钢铁生产与销售", stockCode: "000812" },
  { id: "C-TEST-IND", name: "测试工商实体（集团子公司）", industry: "装备制造", business: "工业测试实体", stockCode: null },
  { id: "C-TEST-BANK", name: "测试银行实体", industry: "银行", business: "存贷款与信用业务", stockCode: null },
  { id: "C-TEST-INS", name: "测试保险实体", industry: "保险", business: "保险合同服务", stockCode: null },
  { id: "C-TEST-RE", name: "测试地产实体", industry: "房地产开发", business: "地产开发与交付", stockCode: null },
];

export const listedPublicCompanies = publicCompanies.filter((company) => company.stockCode !== null);

export function publicCompanyById(id: string): PublicCompany | undefined {
  return publicCompanies.find((company) => company.id === id);
}

export function publicCompanyForStock(stockCode: string): PublicCompany | undefined {
  return listedPublicCompanies.find((company) => company.stockCode === stockCode);
}
