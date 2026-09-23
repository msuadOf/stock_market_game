import type { EngineHost } from "./engine-host.ts";
import type { PublicReportSummary } from "../types/engine.ts";
import type { NormalizedCivilUpdate, NormalizedTickFrame } from "./protocol/index.ts";
import type { UnknownAction } from "@reduxjs/toolkit";
import {
  advanceCompanyEventCoverage,
  installCompanyBaseline,
  recordCivilDateAdvanced,
  recordCompanyPage,
  recordCompanyReport,
  recordCompanyQueryFailure,
  reconcileCompanyPublicMetadata,
  unavailable,
  startCompanyQuery,
} from "../store/company-slice.ts";
import { normalizePublicReportById, normalizePublicReportPage } from "./serde-normalize.ts";

interface Baseline {
  civilDate: string | null;
  revision: string | null;
  seq: number;
}

interface CompanyQuery {
  companyId: string;
  cursor: string | null;
}

type Dispatch = (action: UnknownAction) => unknown;
type CompanyQueryHost = Pick<EngineHost, "capabilities" | "publicReportById" | "queryPublicReports">;

interface PublicMetadata {
  civilDate: string | null;
  revision: string | null;
}

function parsePage(value: unknown, companyId: string): { reports: PublicReportSummary[]; nextCursor: string | null } {
  const page = normalizePublicReportPage(value);
  if (page.reports.some((report) => report.company_id !== companyId)) {
    throw new TypeError("公开公司报告响应不符合公共 DTO 契约");
  }
  return { reports: page.reports, nextCursor: page.next_cursor };
}

function parseReport(value: unknown, companyId: string): PublicReportSummary {
  const report = normalizePublicReportById(value);
  if (report.company_id !== companyId) {
    throw new TypeError("公开公司报告响应不符合公共 DTO 契约");
  }
  return report;
}

export class CompanyQueryCoordinator {
  private generation = 0;
  private requestSequence = 0;
  private readonly activeRequests = new Map<string, number>();
  private readonly cachedCompanies = new Set<string>();
  private readonly host: CompanyQueryHost;
  private readonly dispatch: Dispatch;

  constructor(host: CompanyQueryHost, dispatch: Dispatch) {
    this.host = host;
    this.dispatch = dispatch;
  }

  installBaseline(baseline: Baseline): void {
    this.generation += 1;
    this.activeRequests.clear();
    this.cachedCompanies.clear();
    this.dispatch(installCompanyBaseline({ generation: this.generation, ...baseline }));
  }

  async query(query: CompanyQuery, force = false): Promise<void> {
    const generation = this.generation;
    const key = `${generation}\u0000${query.companyId}\u0000${query.cursor ?? "root"}`;
    if (!force && this.activeRequests.has(key)) return;
    const request = ++this.requestSequence;
    this.activeRequests.set(key, request);
    this.cachedCompanies.add(query.companyId);
    if (!this.host.capabilities.publicCompanyReports || !this.host.queryPublicReports) {
      this.dispatch(unavailable({ generation, ...query, message: "当前宿主不支持公开公司报告查询" }));
      this.activeRequests.delete(key);
      return;
    }
    this.dispatch(startCompanyQuery({ generation, ...query }));
    try {
      const response = parsePage(await this.host.queryPublicReports({
        company_id: query.companyId, cursor: query.cursor, page_size: null,
      }), query.companyId);
      if (this.isCurrentRequest(generation, key, request)) {
        this.dispatch(recordCompanyPage({ generation, ...query, ...response }));
      }
    } catch (error) {
      if (this.isCurrentRequest(generation, key, request)) {
        this.dispatch(recordCompanyQueryFailure({
          generation, ...query, message: error instanceof Error ? error.message : String(error),
        }));
      }
    } finally {
      if (this.isCurrentRequest(generation, key, request)) this.activeRequests.delete(key);
    }
  }

  async queryReportById(query: { companyId: string; reportId: string }): Promise<void> {
    const generation = this.generation;
    const key = `${generation}\u0000${query.companyId}\u0000report\u0000${query.reportId}`;
    if (this.activeRequests.has(key)) return;
    const request = ++this.requestSequence;
    this.activeRequests.set(key, request);
    this.cachedCompanies.add(query.companyId);
    if (!this.host.capabilities.publicCompanyReports || !this.host.publicReportById) {
      this.dispatch(unavailable({ generation, companyId: query.companyId, cursor: null, message: "当前宿主不支持公开公司报告查询" }));
      this.activeRequests.delete(key);
      return;
    }
    try {
      const report = parseReport(await this.host.publicReportById(query.reportId), query.companyId);
      if (this.isCurrentRequest(generation, key, request)) {
        this.dispatch(recordCompanyReport({ generation, companyId: query.companyId, report }));
      }
    } catch (error) {
      if (this.isCurrentRequest(generation, key, request)) {
        this.dispatch(recordCompanyQueryFailure({
          generation, companyId: query.companyId, cursor: null,
          message: error instanceof Error ? error.message : String(error),
        }));
      }
    } finally {
      if (this.isCurrentRequest(generation, key, request)) this.activeRequests.delete(key);
    }
  }

  acceptFrame(frame: NormalizedTickFrame, metadata: PublicMetadata = { civilDate: null, revision: null }): void {
    this.dispatch(advanceCompanyEventCoverage({ generation: this.generation, toSeq: frame.seqTo }));
    this.dispatch(reconcileCompanyPublicMetadata({ generation: this.generation, ...metadata }));
  }

  acceptCivil(update: NormalizedCivilUpdate, metadata: PublicMetadata = { civilDate: null, revision: null }): void {
    this.dispatch(recordCivilDateAdvanced({
      generation: this.generation,
      seq: update.update.seq_to,
      civilDate: update.update.civil_date,
      dayStatus: update.update.boundary.next_status,
    }));
    this.dispatch(advanceCompanyEventCoverage({ generation: this.generation, toSeq: update.update.seq_to }));
    this.dispatch(reconcileCompanyPublicMetadata({ generation: this.generation, ...metadata }));
    for (const companyId of this.cachedCompanies) void this.query({ companyId, cursor: null }, true);
  }

  private isCurrentRequest(generation: number, key: string, request: number): boolean {
    return generation === this.generation && this.activeRequests.get(key) === request;
  }
}
