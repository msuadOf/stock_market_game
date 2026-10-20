import type { PublicReportPage, PublicReportSummary } from "../types/engine.ts";

/**
 * Remote API 的报告归属索引只在当前会话时间线内有效。
 *
 * 公司身份使用 wire 形态：数值 CompanyId（ADR 0041，`PublicReportQuery.company_id`
 * 与服务端路由 `/api/companies/:company_id`（u32）一致）；代码串视图身份的翻译
 * 见 `config/company-ids.ts`，本层不感知。
 */
export class ReportQueryContext {
  private epoch = 0n;
  private readonly companies = new Map<string, number>();

  captureEpoch(): bigint { return this.epoch; }

  invalidate(): void {
    this.epoch += 1n;
    this.companies.clear();
  }

  assertCurrent(capturedEpoch: bigint, messageContext: string): void {
    if (capturedEpoch !== this.epoch) throw new Error(`远程公开报告查询已因会话时间线变更失效，${messageContext}`);
  }

  acceptPage(queryCompanyId: number, capturedEpoch: bigint, page: PublicReportPage): void {
    this.assertCurrent(capturedEpoch, "请重新查询");
    for (const report of page.reports) {
      const knownCompany = this.companies.get(report.id);
      if (report.company_id !== queryCompanyId || (knownCompany !== undefined && knownCompany !== queryCompanyId)) {
        throw new Error(`远程公开报告 ${report.id} 的公司 ${report.company_id} 与查询或已记录公司 ${queryCompanyId} 不匹配`);
      }
    }
    for (const report of page.reports) this.companies.set(report.id, report.company_id);
  }

  companyFor(reportId: string): number {
    const companyId = this.companies.get(reportId);
    if (companyId === undefined) throw new Error(`远程公开报告 ${reportId} 的公司归属未知，请先查询该公司的公开报告页`);
    return companyId;
  }

  validateReport(capturedEpoch: bigint, requestedCompanyId: number, requestedId: string, report: PublicReportSummary): void {
    this.assertCurrent(capturedEpoch, "请先查询新时间线的报告页");
    if (report.id !== requestedId || report.company_id !== requestedCompanyId) {
      throw new Error(`远程公开报告响应 ${report.company_id}/${report.id} 与请求 ${requestedCompanyId}/${requestedId} 不匹配`);
    }
  }
}
