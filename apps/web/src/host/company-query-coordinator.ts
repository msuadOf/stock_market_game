import type { EngineHost } from "./engine-host.ts";
import type { EngineEvent, PublicReportSummary } from "../types/engine.ts";
import type { UnknownAction } from "@reduxjs/toolkit";
import {
  advanceCompanyEventCoverage,
  installCompanyBaseline,
  invalidateCompanyForDisclosure,
  recordCivilDateAdvanced,
  recordCompanyPage,
  recordCompanyReport,
  recordCompanyQueryFailure,
  reconcileCompanyPublicMetadata,
  unavailable,
  startCompanyQuery,
} from "../store/company-slice.ts";
import type { SeqCoverage } from "./host-update.ts";
import { hostEventSeq } from "./host-update.ts";
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

type CompanyStateEvent =
  | { readonly kind: "civil-date-advanced"; readonly event: Extract<EngineEvent, { CivilDateAdvanced: unknown }> }
  | { readonly kind: "disclosure-published"; readonly event: Extract<EngineEvent, { CompanyDisclosurePublished: unknown }> }
  | { readonly kind: "other"; readonly seq: number };

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

function classifyCompanyStateEvent(event: EngineEvent): CompanyStateEvent {
  if ("CivilDateAdvanced" in event) return { kind: "civil-date-advanced", event };
  if ("CompanyDisclosurePublished" in event) return { kind: "disclosure-published", event };
  return { kind: "other", seq: hostEventSeq(event) };
}

function assertNever(value: never): never {
  throw new Error(`未处理的公共公司状态事件：${JSON.stringify(value)}`);
}

export class CompanyQueryCoordinator {
  private generation = 0;
  private lastEventSeq = 0;
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
    this.lastEventSeq = baseline.seq;
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

  acceptEvents(
    events: readonly EngineEvent[],
    coverage: SeqCoverage,
    metadata: PublicMetadata = { civilDate: null, revision: null },
  ): void {
    this.assertEventCoverage(events, coverage);
    for (const event of events) {
      const companyEvent = classifyCompanyStateEvent(event);
      switch (companyEvent.kind) {
        case "civil-date-advanced":
          this.dispatch(recordCivilDateAdvanced({
            generation: this.generation, seq: companyEvent.event.CivilDateAdvanced.seq,
            civilDate: companyEvent.event.CivilDateAdvanced.next_date,
            dayStatus: companyEvent.event.CivilDateAdvanced.next_status,
          }));
          break;
        case "disclosure-published": {
          const disclosure = companyEvent.event.CompanyDisclosurePublished;
          this.dispatch(invalidateCompanyForDisclosure({
            generation: this.generation, seq: disclosure.seq, companyId: disclosure.company,
          }));
          if (this.cachedCompanies.has(disclosure.company)) {
            void this.query({ companyId: disclosure.company, cursor: null }, true);
          }
          break;
        }
        case "other":
          break;
        default:
          assertNever(companyEvent);
      }
    }
    this.lastEventSeq = coverage.toSeq;
    this.dispatch(advanceCompanyEventCoverage({ generation: this.generation, toSeq: coverage.toSeq }));
    this.dispatch(reconcileCompanyPublicMetadata({ generation: this.generation, ...metadata }));
  }

  private assertEventCoverage(events: readonly EngineEvent[], coverage: SeqCoverage): void {
    if (!Number.isSafeInteger(coverage.fromSeq) || !Number.isSafeInteger(coverage.toSeq)
      || coverage.fromSeq < 0 || coverage.toSeq < coverage.fromSeq) {
      throw new Error("公共公司状态事件覆盖区间无效");
    }
    if (coverage.fromSeq !== this.lastEventSeq + 1) {
      throw new Error(`公共公司状态需要宿主重同步：期望 seq ${this.lastEventSeq + 1}，收到 ${coverage.fromSeq}`);
    }
    let previousSeq = coverage.fromSeq - 1;
    for (const event of events) {
      const seq = this.eventSeq(event);
      if (seq <= previousSeq || seq < coverage.fromSeq || seq > coverage.toSeq) {
        throw new Error("公共公司状态事件与宿主 seq 覆盖区间不一致");
      }
      previousSeq = seq;
    }
  }

  private eventSeq(event: EngineEvent): number {
    if ("CivilDateAdvanced" in event) return event.CivilDateAdvanced.seq;
    if ("CompanyDisclosurePublished" in event) return event.CompanyDisclosurePublished.seq;
    return hostEventSeq(event);
  }

  private isCurrentRequest(generation: number, key: string, request: number): boolean {
    return generation === this.generation && this.activeRequests.get(key) === request;
  }
}
